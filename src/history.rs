//! Rank/LP history per account, used for the LP tracking charts.
//!
//! Snapshots are appended after each successful fetch when the rank changed,
//! or as a heartbeat when the last point is older than `HEARTBEAT_MS`, so a
//! flat line still reaches "now" without storing every 30-minute refresh.

use crate::logging::{self, Event, Reason};
use crate::models::{AccountKey, RankInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

const HEARTBEAT_MS: u64 = 6 * 60 * 60 * 1000;
const MAX_POINTS: usize = 1500;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Snapshot {
    /// Unix time in milliseconds.
    pub t: u64,
    pub tier: String,
    #[serde(default)]
    pub division: String,
    #[serde(default)]
    pub lp: i32,
}

impl Snapshot {
    fn same_rank(&self, other: &Snapshot) -> bool {
        self.tier == other.tier && self.division == other.division && self.lp == other.lp
    }

    pub fn score(&self) -> Option<i32> {
        score(&self.tier, &self.division, self.lp)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct QueueHistory {
    #[serde(default)]
    pub lol: Vec<Snapshot>,
    #[serde(default)]
    pub tft: Vec<Snapshot>,
}

/// A single number that orders every rank: 400 points per tier (100 per
/// division) below Master, then Master+ LP on one shared ladder.
pub fn score(tier: &str, division: &str, lp: i32) -> Option<i32> {
    const TIERS: [&str; 7] = [
        "iron", "bronze", "silver", "gold", "platinum", "emerald", "diamond",
    ];
    let tier = tier.trim().to_ascii_lowercase();
    let lp = lp.max(0);
    if matches!(tier.as_str(), "master" | "grandmaster" | "challenger") {
        return Some(2800 + lp);
    }
    let index = TIERS.iter().position(|candidate| *candidate == tier)? as i32;
    let division = match division.trim().to_ascii_uppercase().as_str() {
        "I" | "1" => 3,
        "II" | "2" => 2,
        "III" | "3" => 1,
        "IV" | "4" => 0,
        _ => 0,
    };
    Some(index * 400 + division * 100 + lp.min(100))
}

fn snapshot(t: u64, tier: &str, division: &str, lp: &str) -> Option<Snapshot> {
    let lp = lp.trim().parse::<i32>().unwrap_or(0);
    score(tier, division, lp)?;
    Some(Snapshot {
        t,
        tier: tier.to_owned(),
        division: division.to_owned(),
        lp,
    })
}

fn push(series: &mut Vec<Snapshot>, point: Snapshot) -> bool {
    if let Some(last) = series.last() {
        if point.t <= last.t {
            return false;
        }
        if last.same_rank(&point) && point.t - last.t < HEARTBEAT_MS {
            return false;
        }
    }
    series.push(point);
    if series.len() > MAX_POINTS {
        let excess = series.len() - MAX_POINTS;
        series.drain(..excess);
    }
    true
}

pub fn history_key(key: &AccountKey) -> String {
    format!("{}:{}", key.region, key.account_id)
}

pub struct RankHistory {
    path: PathBuf,
    entries: HashMap<String, QueueHistory>,
}

impl RankHistory {
    /// Load history; a missing or unreadable file starts an empty history
    /// rather than blocking the app.
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let entries = match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|error| {
                logging::record(Event::HistoryLoadFailed, Reason::from_error(&error));
                HashMap::new()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => {
                logging::record(Event::HistoryLoadFailed, Reason::from_error(&error));
                HashMap::new()
            }
        };
        Self { path, entries }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let write = || {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let json = serde_json::to_string(&self.entries).map_err(std::io::Error::other)?;
            // Write then rename so a crash never leaves a truncated history file.
            let temporary = self.path.with_extension("json.tmp");
            std::fs::write(&temporary, json)?;
            std::fs::rename(&temporary, &self.path)
        };
        write().inspect_err(|error| {
            logging::record(Event::HistorySaveFailed, Reason::from_error(error));
        })
    }

    /// Record a fetch result. Returns whether anything was appended.
    pub fn record(&mut self, key: &AccountKey, info: &RankInfo, now_ms: u64) -> bool {
        let entry = self.entries.entry(history_key(key)).or_default();
        let mut changed = false;
        if let Some(point) = snapshot(now_ms, &info.tier, &info.division, &info.lp) {
            changed |= push(&mut entry.lol, point);
        }
        if let Some(tft) = &info.tft {
            if let Some(point) = snapshot(now_ms, &tft.tier, &tft.division, &tft.lp) {
                changed |= push(&mut entry.tft, point);
            }
        }
        changed
    }

    pub fn get(&self, key: &AccountKey) -> Option<&QueueHistory> {
        self.entries.get(&history_key(key))
    }

    pub fn remove(&mut self, key: &AccountKey) {
        self.entries.remove(&history_key(key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TftRank;

    fn key() -> AccountKey {
        AccountKey {
            account_id: "acc".into(),
            region: "euw".into(),
        }
    }

    fn info(tier: &str, division: &str, lp: &str) -> RankInfo {
        RankInfo {
            tier: tier.into(),
            division: division.into(),
            lp: lp.into(),
            tft: Some(TftRank::default()),
            ..RankInfo::default()
        }
    }

    #[test]
    fn score_orders_ranks() {
        assert_eq!(score("Iron", "IV", 0), Some(0));
        assert_eq!(score("Gold", "II", 50), Some(3 * 400 + 200 + 50));
        assert!(score("Diamond", "I", 99) < score("Master", "", 0));
        assert_eq!(score("Challenger", "", 1200), Some(4000));
        assert_eq!(score("Unranked", "", 0), None);
        assert_eq!(score("Error", "", 0), None);
    }

    #[test]
    fn records_changes_and_heartbeats_only() {
        let directory = tempfile::tempdir().unwrap();
        let mut history = RankHistory::load(directory.path().join("history.json"));
        assert!(history.record(&key(), &info("Gold", "II", "50"), 1_000));
        // Same rank shortly after: skipped.
        assert!(!history.record(&key(), &info("Gold", "II", "50"), 2_000));
        // LP change: recorded.
        assert!(history.record(&key(), &info("Gold", "II", "70"), 3_000));
        // Same rank after the heartbeat interval: recorded.
        assert!(history.record(&key(), &info("Gold", "II", "70"), 3_000 + HEARTBEAT_MS));
        // Errors and unranked never create points; TFT unranked is skipped.
        assert!(!history.record(&key(), &info("Error", "", ""), 10 * HEARTBEAT_MS));
        let series = &history.get(&key()).unwrap();
        assert_eq!(series.lol.len(), 3);
        assert!(series.tft.is_empty());

        history.save().unwrap();
        let reloaded = RankHistory::load(directory.path().join("history.json"));
        assert_eq!(reloaded.get(&key()).unwrap().lol, series.lol);
    }

    #[test]
    fn corrupt_file_starts_empty() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.json");
        std::fs::write(&path, "{not json").unwrap();
        assert!(RankHistory::load(path).get(&key()).is_none());
    }
}
