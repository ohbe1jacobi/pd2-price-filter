//! Base-code discovery via the official market listing API.
use crate::pricing::Client;
use crate::types::{BaseCodeCache, ItemQuality, Roster};
use std::collections::BTreeSet;

pub const MARKET_URL: &str = "https://api.projectdiablo2.com/market/listing";
const PACE: std::time::Duration = std::time::Duration::from_millis(200);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MissingItems {
    pub unique_ids: Vec<u64>,
    pub set_names: Vec<String>,
}

/// Items whose base code is not yet cached. Uniques are keyed by id, sets by name.
pub fn missing_items(roster: &Roster, cache: &BaseCodeCache) -> MissingItems {
    let mut unique_ids = Vec::new();
    let mut set_names = Vec::new();
    let mut seen = BTreeSet::new();
    for (name, entry) in roster {
        match entry.kind {
            ItemQuality::Unique => {
                if let Some(id) = entry.unique_id {
                    let key = id.to_string();
                    if !seen.contains(&id) && !cache.uniques.contains_key(&key) {
                        seen.insert(id);
                        unique_ids.push(id);
                    }
                }
            }
            ItemQuality::Set => {
                if !cache.sets.contains_key(name) {
                    set_names.push(name.clone());
                }
            }
        }
    }
    MissingItems {
        unique_ids,
        set_names,
    }
}

pub fn refresh_cache(
    roster: &Roster,
    cache: &mut BaseCodeCache,
    client: &Client,
    mut log: impl FnMut(&str),
) -> Result<(), String> {
    let MissingItems {
        unique_ids,
        set_names,
    } = missing_items(roster, cache);
    let total = unique_ids.len() + set_names.len();
    if total == 0 {
        log("Base codes: cache is up to date.");
        return Ok(());
    }
    log(&format!(
        "Base codes: harvesting {total} missing item(s)..."
    ));
    let mut done = 0;
    for id in unique_ids {
        let code = client.fetch_unique_base_code(id)?;
        cache.uniques.insert(id.to_string(), code);
        done += 1;
        if done % 25 == 0 {
            log(&format!("  {done}/{total}"));
        }
        std::thread::sleep(PACE);
    }
    for name in set_names {
        let code = client.fetch_set_base_code(&name)?;
        cache.sets.insert(name, code);
        done += 1;
        if done % 25 == 0 {
            log(&format!("  {done}/{total}"));
        }
        std::thread::sleep(PACE);
    }
    Ok(())
}

impl Client {
    pub fn fetch_unique_base_code(&self, id: u64) -> Result<String, String> {
        let id = id.to_string();
        self.base_code(&[("$limit", "1"), ("item.unique.id", &id)])
    }
    pub fn fetch_set_base_code(&self, name: &str) -> Result<String, String> {
        self.base_code(&[
            ("$limit", "1"),
            ("item.quality.name", "Set"),
            ("item.name", name),
        ])
    }
    fn base_code(&self, params: &[(&str, &str)]) -> Result<String, String> {
        let data = self.get_json(MARKET_URL, params)?;
        Ok(data
            .get("data")
            .and_then(|d| d.get(0))
            .and_then(|l| l.pointer("/item/base/id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> Roster {
        let mut roster = Roster::new();
        for (name, kind, unique_id) in [
            ("Shako", ItemQuality::Unique, Some(1)),
            ("The Shako", ItemQuality::Unique, Some(1)),
            ("NoId", ItemQuality::Unique, None),
            ("Aldur's Advance", ItemQuality::Set, None),
        ] {
            roster.insert(name.into(), crate::types::RosterEntry { kind, unique_id });
        }
        roster
    }

    #[test]
    fn lists_uncached_uniques_by_deduplicated_id_and_sets_by_name() {
        let missing = missing_items(&roster(), &BaseCodeCache::default());
        assert_eq!(missing.unique_ids, vec![1]);
        assert_eq!(missing.set_names, vec!["Aldur's Advance".to_string()]);
    }

    #[test]
    fn skips_items_already_cached_including_empty_mappings() {
        let mut cache = BaseCodeCache::default();
        cache.uniques.insert("1".into(), "ush".into());
        cache.sets.insert("Aldur's Advance".into(), String::new());
        assert_eq!(missing_items(&roster(), &cache), MissingItems::default());
    }

    #[test]
    fn parses_first_listing_base_code_and_treats_absent_listing_as_empty() {
        use crate::pricing::test_http;
        use std::time::Duration;
        for (body, expected) in [
            (r#"{"data":[{"item":{"base":{"id":"ush"}}}]}"#, "ush"),
            (r#"{"data":[]}"#, ""),
            (r#"{"data":[{}]}"#, ""),
            ("{}", ""),
        ] {
            let (base, request, server) = test_http::serve(200, body.into(), Duration::ZERO);
            let code = Client::fixture(&base, Duration::from_secs(2))
                .fetch_unique_base_code(7)
                .unwrap();
            assert_eq!(code, expected);
            let request = request.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(
                request.starts_with("GET /market/listing?%24limit=1&item.unique.id=7 "),
                "{request}"
            );
            server.join().unwrap();
        }
        for (status, body) in [(404, "no"), (500, "outage"), (200, "not json")] {
            let (base, _, server) = test_http::serve(status, body.into(), Duration::ZERO);
            assert!(
                Client::fixture(&base, Duration::from_secs(2))
                    .fetch_unique_base_code(7)
                    .is_err()
            );
            server.join().unwrap();
        }
    }

    #[test]
    fn set_query_encodes_name_and_filters_on_set_quality() {
        use crate::pricing::test_http;
        use std::time::Duration;
        let (base, request, server) = test_http::serve(
            200,
            r#"{"data":[{"item":{"base":{"id":"xtb"}}}]}"#.into(),
            Duration::ZERO,
        );
        assert_eq!(
            Client::fixture(&base, Duration::from_secs(2))
                .fetch_set_base_code("Aldur's & Advance")
                .unwrap(),
            "xtb"
        );
        let request = request.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(request.contains("item.quality.name=Set"), "{request}");
        assert!(
            request.contains("item.name=Aldur%27s%20%26%20Advance"),
            "{request}"
        );
        server.join().unwrap();
    }

    #[test]
    fn refresh_harvests_missing_items_and_persists_empty_mappings() {
        use crate::pricing::test_http;
        use std::time::Duration;
        let responses = vec![
            (200, r#"{"data":[{"item":{"base":{"id":"ush"}}}]}"#.into()),
            (200, r#"{"data":[]}"#.into()),
        ];
        let (base, _, server) = test_http::serve_seq(responses, Duration::ZERO);
        let mut cache = BaseCodeCache::default();
        let mut logs = Vec::new();
        refresh_cache(
            &roster(),
            &mut cache,
            &Client::fixture(&base, Duration::from_secs(5)),
            |m| logs.push(m.to_string()),
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(cache.uniques.get("1").unwrap(), "ush");
        assert_eq!(cache.sets.get("Aldur's Advance").unwrap(), "");
        assert!(
            logs.iter().any(|m| m.contains("harvesting 2 missing")),
            "{logs:?}"
        );
        assert!(missing_items(&roster(), &cache) == MissingItems::default());
    }

    #[test]
    fn refresh_reports_http_failures_and_keeps_up_to_date_cache_untouched() {
        use crate::pricing::test_http;
        use std::time::Duration;
        let (base, _, server) = test_http::serve(500, "outage".into(), Duration::ZERO);
        let mut cache = BaseCodeCache::default();
        let error = refresh_cache(
            &roster(),
            &mut cache,
            &Client::fixture(&base, Duration::from_secs(5)),
            |_| {},
        )
        .unwrap_err();
        assert!(error.contains("500"), "{error}");
        server.join().unwrap();
        assert!(!cache.uniques.contains_key("1"));
        let mut cache = BaseCodeCache::default();
        cache.uniques.insert("1".into(), "ush".into());
        cache.sets.insert("Aldur's Advance".into(), "xtb".into());
        let before = cache.clone();
        refresh_cache(
            &roster(),
            &mut cache,
            &Client::fixture("http://127.0.0.1:1", Duration::from_millis(100)),
            |_| {},
        )
        .unwrap();
        assert_eq!(cache, before);
    }

    #[test]
    fn refresh_paces_sequential_requests() {
        use crate::pricing::test_http;
        use std::time::Duration;
        let responses = vec![
            (200, r#"{"data":[]}"#.into()),
            (200, r#"{"data":[]}"#.into()),
        ];
        let (base, _, server) = test_http::serve_seq(responses, Duration::ZERO);
        let mut cache = BaseCodeCache::default();
        let started = std::time::Instant::now();
        refresh_cache(
            &roster(),
            &mut cache,
            &Client::fixture(&base, Duration::from_secs(5)),
            |_| {},
        )
        .unwrap();
        server.join().unwrap();
        assert!(started.elapsed() >= PACE * 2, "requests were not paced");
    }
}
