//! Shared types, matching the original CLI's runtime data model.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Price {
    pub median: f64,
    #[serde(rename = "medianSamples")]
    pub median_samples: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemQuality {
    Unique,
    Set,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterEntry {
    #[serde(rename = "type")]
    pub kind: ItemQuality,
    #[serde(rename = "uniqueId", default)]
    pub unique_id: Option<u64>,
}

pub type Roster = BTreeMap<String, RosterEntry>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseCodeCache {
    #[serde(default)]
    pub uniques: BTreeMap<String, String>,
    #[serde(default)]
    pub sets: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PricedItem {
    Unique {
        base_code: String,
        price: Price,
    },
    Set {
        base_code: String,
        price: Price,
    },
    Fixed {
        condition: String,
        category: String,
        price: Price,
    },
}
