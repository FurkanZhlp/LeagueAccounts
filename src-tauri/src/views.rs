//! Data shapes sent to the web interface. Passwords never appear here.

use leagueaccounts::history::{QueueHistory, RankHistory, Snapshot};
use leagueaccounts::models::{Account, AccountKey};
use serde::{Deserialize, Serialize};

/// Points of the card sparkline, newest last.
const TREND_POINTS: usize = 40;

/// A command failure the UI translates: a stable `code` plus an optional
/// untranslated detail (for example an OS error message).
#[derive(Clone, Debug, Serialize)]
pub struct AppError {
    pub code: &'static str,
    pub detail: Option<String>,
}

pub fn fail(code: &'static str) -> AppError {
    AppError { code, detail: None }
}

pub fn fail_with(code: &'static str, detail: impl std::fmt::Display) -> AppError {
    AppError {
        code,
        detail: Some(detail.to_string()),
    }
}

pub type CommandResult<T> = Result<T, AppError>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Key {
    pub account_id: String,
    pub region: String,
}

impl From<Key> for AccountKey {
    fn from(key: Key) -> Self {
        Self {
            account_id: key.account_id,
            region: key.region,
        }
    }
}

impl From<&Account> for Key {
    fn from(account: &Account) -> Self {
        Self {
            account_id: account.account_id.clone(),
            region: account.region.clone(),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LolView {
    tier: String,
    division: String,
    lp: String,
    reached_last_season: String,
    finished_last_season: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TftView {
    tier: String,
    division: String,
    lp: String,
    last_set: String,
}

/// `[unix_ms, score]` pairs per queue.
#[derive(Clone, Default, Serialize)]
pub struct TrendView {
    lol: Vec<(u64, i32)>,
    tft: Vec<(u64, i32)>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    account_id: String,
    name: String,
    riot_id_not_found: bool,
    region: String,
    region_display: String,
    description: String,
    level: String,
    has_password: bool,
    lol: LolView,
    tft: TftView,
    trend: TrendView,
}

fn trend(series: &[Snapshot]) -> Vec<(u64, i32)> {
    let start = series.len().saturating_sub(TREND_POINTS);
    series[start..]
        .iter()
        .filter_map(|point| point.score().map(|score| (point.t, score)))
        .collect()
}

pub fn account_view(account: &Account, history: &RankHistory) -> AccountView {
    let queues = history.get(&account.key());
    AccountView {
        account_id: account.account_id.clone(),
        name: account.name.clone(),
        riot_id_not_found: account.riot_id_not_found,
        region: account.region.clone(),
        region_display: account.region_display.clone(),
        description: account.description.clone(),
        level: account.level.clone(),
        has_password: !account.password.is_empty(),
        lol: LolView {
            tier: account.tier.clone(),
            division: account.division.clone(),
            lp: account.lp.clone(),
            reached_last_season: account.reached_last_season.clone(),
            finished_last_season: account.finished_last_season.clone(),
        },
        tft: TftView {
            tier: account.tft.tier.clone(),
            division: account.tft.division.clone(),
            lp: account.tft.lp.clone(),
            last_set: account.tft.last_set.clone(),
        },
        trend: queues
            .map(|queues| TrendView {
                lol: trend(&queues.lol),
                tft: trend(&queues.tft),
            })
            .unwrap_or_default(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PointView {
    t: u64,
    tier: String,
    division: String,
    lp: i32,
    score: i32,
}

#[derive(Default, Serialize)]
pub struct HistoryView {
    lol: Vec<PointView>,
    tft: Vec<PointView>,
}

fn points(series: &[Snapshot]) -> Vec<PointView> {
    series
        .iter()
        .filter_map(|point| {
            Some(PointView {
                t: point.t,
                tier: point.tier.clone(),
                division: point.division.clone(),
                lp: point.lp,
                score: point.score()?,
            })
        })
        .collect()
}

impl From<&QueueHistory> for HistoryView {
    fn from(history: &QueueHistory) -> Self {
        Self {
            lol: points(&history.lol),
            tft: points(&history.tft),
        }
    }
}
