//! Roster (unique/set name list) discovery from the aggregator snapshot.
use crate::pricing::Client;
use crate::types::{ItemQuality, Roster, RosterEntry};
use serde_json::Value;

pub const ROSTER_URL: &str = "https://pd2-aggregator.vercel.app/price-snapshot.json";

/// Keep uniques and sets only, preserving `uniqueId` and dropping runewords.
pub fn parse_roster(raw: &Value) -> Result<Roster, String> {
    let items = raw
        .get("items")
        .and_then(Value::as_object)
        .ok_or("Invalid snapshot: missing 'items' field")?;
    let mut roster = Roster::new();
    for (name, item) in items {
        let object = item
            .as_object()
            .ok_or_else(|| format!("Invalid snapshot: entry '{name}' must be an object"))?;
        let kind = match object.get("type").and_then(Value::as_str) {
            Some("Unique") => ItemQuality::Unique,
            Some("Set") => ItemQuality::Set,
            _ => continue,
        };
        let unique_id = match object.get("uniqueId") {
            None | Some(Value::Null) => None,
            Some(value) => Some(value.as_u64().ok_or_else(|| {
                format!("Invalid snapshot: uniqueId for '{name}' must be a nonnegative integer")
            })?),
        };
        roster.insert(name.clone(), RosterEntry { kind, unique_id });
    }
    Ok(roster)
}

impl Client {
    pub fn fetch_roster(&self) -> Result<Roster, String> {
        let raw = self.get_json(ROSTER_URL, &[])?;
        parse_roster(&raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::test_http;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn keeps_uniques_and_sets_dropping_runewords() {
        let roster = parse_roster(&json!({"items": {
            "Shako": {"type": "Unique", "uniqueId": 1, "medianHr": 5},
            "Aldur's Advance": {"type": "Set", "medianHr": 0.1},
            "Spirit": {"type": "Runeword", "medianHr": 4},
            "Broken": {"type": "Unique"}
        }}))
        .unwrap();
        assert_eq!(roster.len(), 3);
        assert_eq!(
            roster["Shako"],
            RosterEntry {
                kind: ItemQuality::Unique,
                unique_id: Some(1)
            }
        );
        assert_eq!(
            roster["Aldur's Advance"],
            RosterEntry {
                kind: ItemQuality::Set,
                unique_id: None
            }
        );
        assert_eq!(
            roster["Broken"],
            RosterEntry {
                kind: ItemQuality::Unique,
                unique_id: None
            }
        );
    }

    #[test]
    fn rejects_invalid_snapshot_shapes() {
        for raw in [
            json!({}),
            json!(null),
            json!([]),
            json!({"items": []}),
            json!({"items": {"Shako": {"type": "Unique", "uniqueId": "1"}}}),
            json!({"items": {"Shako": {"type": "Unique", "uniqueId": -1}}}),
            json!({"items": {"Shako": 3}}),
            json!({"items": {"Shako": []}}),
            json!({"items": {"Shako": null}}),
        ] {
            assert!(parse_roster(&raw).is_err(), "accepted {raw}");
        }
    }

    #[test]
    fn http_roster_fails_descriptively_on_error_statuses_and_malformed_bodies() {
        for (status, body) in [
            (404, "missing"),
            (500, "outage"),
            (200, "not json"),
            (200, "{}"),
        ] {
            let (base, request, server) = test_http::serve(status, body.into(), Duration::ZERO);
            let result = Client::fixture(&base, Duration::from_secs(2)).fetch_roster();
            assert!(result.is_err(), "accepted {status} {body}");
            assert!(
                request
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                    .starts_with("GET /price-snapshot.json ")
            );
            server.join().unwrap();
        }
    }

    #[test]
    fn http_roster_parses_a_snapshot() {
        let body =
            r#"{"items":{"Shako":{"type":"Unique","uniqueId":1},"Spirit":{"type":"Runeword"}}}"#;
        let (base, _, server) = test_http::serve(200, body.into(), Duration::ZERO);
        let roster = Client::fixture(&base, Duration::from_secs(2))
            .fetch_roster()
            .unwrap();
        assert_eq!(roster.len(), 1);
        assert_eq!(roster["Shako"].unique_id, Some(1));
        server.join().unwrap();
    }
}
