//! Curated fixed-item definitions, with no catalog or provider inference.
use crate::types::{Price, PricedItem};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixedItemDef {
    pub name: String,
    pub condition: String,
    pub category: String,
}

pub fn parse_fixed_items(bytes: &[u8]) -> Result<Vec<FixedItemDef>, String> {
    let raw: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if !raw.is_array() {
        return Err("expected a JSON array of fixed items".into());
    }
    serde_json::from_value(raw)
        .map_err(|_| "every entry needs string name, condition, category".into())
}
pub fn resolve_fixed_items(
    defs: &[FixedItemDef],
    prices: &BTreeMap<String, Price>,
) -> Vec<PricedItem> {
    defs.iter()
        .filter_map(|def| {
            prices.get(&def.name).map(|price| PricedItem::Fixed {
                condition: def.condition.clone(),
                category: def.category.clone(),
                price: price.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_defs_with_prices_and_skips_unpriced_without_deduplicating_defs() {
        let defs = parse_fixed_items(br#"[{"name":"Ber Rune","condition":"RUNE=30","category":"rune"},{"name":"Key of Terror","condition":"pk1","category":"uber"},{"name":"Ber Rune","condition":"r30","category":"alternate"}]"#).unwrap();
        let price = Price {
            median: 3.0,
            median_samples: 10,
        };
        let prices = [("Ber Rune".into(), price.clone())].into_iter().collect();
        assert_eq!(
            resolve_fixed_items(&defs, &prices),
            vec![
                PricedItem::Fixed {
                    condition: "RUNE=30".into(),
                    category: "rune".into(),
                    price: price.clone()
                },
                PricedItem::Fixed {
                    condition: "r30".into(),
                    category: "alternate".into(),
                    price
                },
            ]
        );
        assert!(resolve_fixed_items(&defs, &BTreeMap::new()).is_empty());
    }

    #[test]
    fn loads_valid_lists_and_requires_an_array_of_string_fields() {
        let defs =
            parse_fixed_items(br#"[{"name":"Ber Rune","condition":"RUNE=30","category":"rune"}]"#)
                .unwrap();
        assert_eq!(
            defs,
            vec![FixedItemDef {
                name: "Ber Rune".into(),
                condition: "RUNE=30".into(),
                category: "rune".into()
            }]
        );
        assert!(
            parse_fixed_items(br#"{"not":"an array"}"#)
                .unwrap_err()
                .contains("array")
        );
        for invalid in [
            "[null]",
            "[{}]",
            "[1]",
            r#"[{"name":7,"condition":"r30","category":"rune"}]"#,
            r#"[{"name":"Ber","condition":null,"category":"rune"}]"#,
            r#"[{"name":"Ber","condition":"r30","category":false}]"#,
            "[",
        ] {
            assert!(parse_fixed_items(invalid.as_bytes()).is_err(), "{invalid}");
        }
        assert!(parse_fixed_items(b"[]").unwrap().is_empty());
        // Empty strings and extra fields are accepted by the original validator.
        assert!(
            parse_fixed_items(br#"[{"name":"","condition":"","category":"","extra":1}]"#).is_ok()
        );
    }
}
