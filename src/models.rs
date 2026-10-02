use serde::{Deserialize, Serialize};

/// One League account tracked by the application.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub account_id: String,
    pub name: String,
    /// Set only when both OP.GG profiles report that the saved Riot ID is missing.
    #[serde(default)]
    pub riot_id_not_found: bool,
    pub region: String,
    pub region_display: String,
    /// Passwords are never written to the normal accounts file. They are kept
    /// in Windows Credential Manager and only held here while the app runs.
    #[serde(skip_serializing, default)]
    pub password: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_tier")]
    pub tier: String,
    #[serde(default)]
    pub division: String,
    #[serde(default)]
    pub lp: String,
    #[serde(default)]
    pub level: String,
    #[serde(default = "default_history")]
    pub reached_last_season: String,
    #[serde(default = "default_history")]
    pub finished_last_season: String,
    /// Teamfight Tactics ranked standing for the same Riot account.
    #[serde(default)]
    pub tft: TftRank,
}

/// Ranked TFT standing: the current set and the previous set's final rank.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TftRank {
    #[serde(default = "default_tier")]
    pub tier: String,
    #[serde(default)]
    pub division: String,
    #[serde(default)]
    pub lp: String,
    #[serde(default = "default_history")]
    pub last_set: String,
}

impl Default for TftRank {
    fn default() -> Self {
        Self {
            tier: default_tier(),
            division: String::new(),
            lp: String::new(),
            last_set: default_history(),
        }
    }
}

impl TftRank {
    pub fn error() -> Self {
        Self {
            tier: "Error".to_owned(),
            division: String::new(),
            lp: String::new(),
            last_set: "...".to_owned(),
        }
    }
}

fn default_tier() -> String {
    "Unranked".to_owned()
}

fn default_history() -> String {
    "N/A".to_owned()
}

impl Account {
    pub fn key(&self) -> AccountKey {
        AccountKey {
            account_id: self.account_id.clone(),
            region: self.region.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AccountKey {
    pub account_id: String,
    pub region: String,
}

/// Values returned by the rank provider.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RankInfo {
    /// `None` when a lookup cannot verify the Riot ID (for example a timeout).
    pub riot_id_not_found: Option<bool>,
    pub tier: String,
    pub division: String,
    pub lp: String,
    pub level: String,
    pub reached_last_season: String,
    pub finished_last_season: String,
    /// `None` when the TFT profile could not be fetched.
    pub tft: Option<TftRank>,
}

impl RankInfo {
    pub fn unranked() -> Self {
        Self {
            riot_id_not_found: None,
            tier: "Unranked".to_owned(),
            division: String::new(),
            lp: String::new(),
            level: String::new(),
            reached_last_season: "Unranked".to_owned(),
            finished_last_season: "Unranked".to_owned(),
            tft: Some(TftRank::default()),
        }
    }

    pub fn error() -> Self {
        Self {
            riot_id_not_found: None,
            tier: "Error".to_owned(),
            division: String::new(),
            lp: String::new(),
            level: String::new(),
            reached_last_season: "...".to_owned(),
            finished_last_season: "...".to_owned(),
            tft: None,
        }
    }

    pub fn apply_to(&self, account: &mut Account) {
        if let Some(not_found) = self.riot_id_not_found {
            account.riot_id_not_found = not_found;
        }
        account.tier = if self.tier.is_empty() {
            "Unranked".to_owned()
        } else {
            self.tier.clone()
        };
        account.division = self.division.clone();
        account.lp = self.lp.clone();
        account.level = self.level.clone();
        account.reached_last_season = if self.reached_last_season.is_empty() {
            "N/A".to_owned()
        } else {
            self.reached_last_season.clone()
        };
        account.finished_last_season = if self.finished_last_season.is_empty() {
            "N/A".to_owned()
        } else {
            self.finished_last_season.clone()
        };
        account.tft = self.tft.clone().unwrap_or_else(TftRank::error);
    }
}
