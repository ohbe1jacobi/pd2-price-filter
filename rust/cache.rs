//! Original per-mode price-cache format, with atomic persistence and safe refresh.
use crate::types::Price;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
use time::OffsetDateTime;

pub type PriceCache = BTreeMap<String, CacheEntry>;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CacheEntry {
    Price {
        median: f64,
        #[serde(rename = "medianSamples")]
        median_samples: u64,
        #[serde(rename = "fetchedAt")]
        fetched_at: String,
    },
    NoData {
        #[serde(rename = "noData")]
        no_data: bool,
        #[serde(rename = "fetchedAt")]
        fetched_at: String,
    },
}
#[derive(Debug)]
pub struct Gathered {
    pub prices: BTreeMap<String, Price>,
    pub failures: usize,
}

fn timestamp(entry: &CacheEntry) -> &str {
    match entry {
        CacheEntry::Price { fetched_at, .. } | CacheEntry::NoData { fetched_at, .. } => fetched_at,
    }
}

pub fn is_fresh(entry: Option<&CacheEntry>, now: OffsetDateTime) -> bool {
    let Some(entry) = entry else { return false };
    let Ok(fetched) = OffsetDateTime::parse(
        timestamp(entry),
        &time::format_description::well_known::Rfc3339,
    ) else {
        return false;
    };
    let age = now - fetched;
    age >= time::Duration::ZERO && age < time::Duration::hours(24)
}

pub fn entry_price(entry: &CacheEntry) -> Option<Price> {
    match entry {
        CacheEntry::Price {
            median,
            median_samples,
            ..
        } => Some(Price {
            median: *median,
            median_samples: *median_samples,
        }),
        CacheEntry::NoData { .. } => None,
    }
}

pub fn load_price_cache(path: &Path) -> Result<PriceCache, String> {
    if let Err(e) = std::fs::metadata(path) {
        if e.kind() == std::io::ErrorKind::NotFound {
            return Ok(PriceCache::new());
        }
        return Err(format!("cache {}: {e}", path.display()));
    }
    let raw = crate::io::read_bounded(path, 8 * 1024 * 1024)?;
    let cache: PriceCache = serde_json::from_slice(&raw).map_err(|e| {
        format!(
            "invalid price cache {}: {e}; move it aside explicitly to recover",
            path.display()
        )
    })?;
    validate(&cache)?;
    Ok(cache)
}

fn validate(cache: &PriceCache) -> Result<(), String> {
    for (name, entry) in cache {
        OffsetDateTime::parse(
            timestamp(entry),
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|e| format!("invalid cached timestamp for {name}: {e}"))?;
        match entry {
            CacheEntry::Price { median, .. } if !median.is_finite() || *median < 0.0 => {
                return Err(format!("invalid cached price for {name}"));
            }
            CacheEntry::NoData { no_data: false, .. } => {
                return Err(format!("invalid noData marker for {name}"));
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn save_price_cache(path: &Path, cache: &PriceCache) -> Result<(), String> {
    validate(cache)?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("cache directory: {e}"))?;
    }
    let mut bytes = serde_json::to_vec_pretty(cache).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    crate::io::atomic_write(path, &bytes)
}

pub fn gather_prices(
    names: &[String],
    cache: &mut PriceCache,
    refresh: bool,
    now: OffsetDateTime,
    mut fetch: impl FnMut(&str) -> Result<Option<Price>, String>,
    mut pace: impl FnMut(),
) -> Gathered {
    let mut gathered = Gathered {
        prices: BTreeMap::new(),
        failures: 0,
    };
    let fetched_at = now
        .format(&time::format_description::well_known::Rfc3339)
        .expect("representable current timestamp");
    for name in names {
        let price = if !refresh && is_fresh(cache.get(name), now) {
            cache.get(name).and_then(entry_price)
        } else {
            let result = fetch(name);
            pace();
            match result {
                Ok(Some(p)) if p.median.is_finite() && p.median >= 0.0 => {
                    cache.insert(
                        name.clone(),
                        CacheEntry::Price {
                            median: p.median,
                            median_samples: p.median_samples,
                            fetched_at: fetched_at.clone(),
                        },
                    );
                    Some(p)
                }
                Ok(None) => {
                    cache.insert(
                        name.clone(),
                        CacheEntry::NoData {
                            no_data: true,
                            fetched_at: fetched_at.clone(),
                        },
                    );
                    None
                }
                _ => {
                    gathered.failures += 1;
                    None
                }
            }
        };
        if let Some(price) =
            price.filter(|p| format!("{:.2}", p.median).parse::<f64>().unwrap_or(0.0) > 0.0)
        {
            gathered.prices.insert(name.clone(), price);
        }
    }
    gathered
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap()
    }
    fn entry() -> CacheEntry {
        CacheEntry::Price {
            median: 3.0,
            median_samples: 20,
            fetched_at: "2027-01-15T08:00:00Z".into(),
        }
    }
    fn dir() -> tempfile::TempDir {
        tempfile::tempdir_in(
            std::env::var_os("TMPDIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(std::env::temp_dir),
        )
        .unwrap()
    }
    #[test]
    fn freshness_checks_ttl_missing_and_future() {
        let e = entry();
        let t = time::OffsetDateTime::parse(
            "2027-01-15T08:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        assert!(is_fresh(Some(&e), t + time::Duration::hours(2)));
        assert!(!is_fresh(Some(&e), t + time::Duration::hours(30)));
        assert!(!is_fresh(None, t));
        assert!(!is_fresh(Some(&e), t - time::Duration::seconds(1)));
    }
    #[test]
    fn extracts_price_but_not_negative_result() {
        assert_eq!(
            entry_price(&entry()),
            Some(Price {
                median: 3.0,
                median_samples: 20
            })
        );
        assert_eq!(
            entry_price(&CacheEntry::NoData {
                no_data: true,
                fetched_at: "2027-01-15T08:00:00Z".into()
            }),
            None
        );
    }
    #[test]
    fn missing_cache_is_empty_and_roundtrips_original_schema() {
        let d = dir();
        let p = d.path().join("nested/cache.json");
        assert!(load_price_cache(&p).unwrap().is_empty());
        let c = PriceCache::from([("Ber Rune".into(), entry())]);
        save_price_cache(&p, &c).unwrap();
        assert_eq!(load_price_cache(&p).unwrap(), c);
        assert!(fs::read_to_string(&p).unwrap().contains("medianSamples"));
    }
    #[test]
    fn corrupt_or_unreadable_cache_is_not_silently_discarded() {
        let d = dir();
        let p = d.path().join("bad.json");
        fs::write(&p, b"{").unwrap();
        assert!(load_price_cache(&p).is_err());
        assert!(load_price_cache(d.path()).is_err());
    }
    #[test]
    fn refresh_failure_keeps_previous_success_without_silent_stale_use() {
        let mut c = PriceCache::from([("Ber Rune".into(), entry())]);
        let old = c.clone();
        let g = gather_prices(
            &["Ber Rune".into()],
            &mut c,
            true,
            now(),
            |_| Err("HTTP 500".into()),
            || {},
        );
        assert_eq!(c, old);
        assert_eq!(g.failures, 1);
        assert!(g.prices.is_empty());
    }
    #[test]
    fn fresh_rerun_makes_no_requests_including_negative_cache() {
        let t = OffsetDateTime::parse(
            "2027-01-15T08:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        let mut c = PriceCache::from([
            ("Ber Rune".into(), entry()),
            (
                "Missing".into(),
                CacheEntry::NoData {
                    no_data: true,
                    fetched_at: "2027-01-15T08:00:00Z".into(),
                },
            ),
        ]);
        let g = gather_prices(
            &["Ber Rune".into(), "Missing".into()],
            &mut c,
            false,
            t,
            |_| panic!("fresh cache performed network request"),
            || panic!("fresh cache paced a nonexistent request"),
        );
        assert_eq!(g.prices.len(), 1);
        assert_eq!(g.failures, 0);
    }
    #[test]
    fn refresh_replaces_only_successes_and_caches_no_data() {
        let mut c = PriceCache::new();
        let mut requests = 0;
        let g = gather_prices(
            &["Ber Rune".into(), "Missing".into()],
            &mut c,
            true,
            now(),
            |n| {
                requests += 1;
                Ok(if n == "Ber Rune" {
                    Some(Price {
                        median: 3.0,
                        median_samples: 20,
                    })
                } else {
                    None
                })
            },
            || {},
        );
        assert_eq!(requests, 2);
        assert_eq!(g.prices.len(), 1);
        assert_eq!(c.len(), 2);
        assert!(matches!(c["Missing"], CacheEntry::NoData { .. }));
    }
    #[test]
    fn validates_cache_values_and_timestamps() {
        for raw in [
            r#"{"X":{"median":-1,"medianSamples":2,"fetchedAt":"2026-10-30T00:00:00Z"}}"#,
            r#"{"X":{"noData":false,"fetchedAt":"2026-10-30T00:00:00Z"}}"#,
            r#"{"X":{"median":1,"medianSamples":2,"fetchedAt":"bad"}}"#,
        ] {
            let d = dir();
            let p = d.path().join("cache.json");
            fs::write(&p, raw).unwrap();
            assert!(load_price_cache(&p).is_err());
        }
    }
}
