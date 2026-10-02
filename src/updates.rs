//! New-version check against the GitHub releases of the app's repository.

use serde::Serialize;
use std::time::Duration;

/// `owner/name` of the repository whose releases are checked.
pub const REPOSITORY: &str = "FurkanZhlp/LeagueAccounts";

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub name: String,
    pub url: String,
    pub notes: String,
    pub published_at: String,
}

/// Parse "v3.1.0" / "3.1" / "3.1.0-beta" into comparable numbers.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.trim().trim_start_matches(['v', 'V']);
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let major = parts.next()??;
    let minor = parts.next().flatten().unwrap_or(0);
    let patch = parts.next().flatten().unwrap_or(0);
    Some((major, minor, patch))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

/// Only release pages of `REPOSITORY` may be opened from update notices.
pub fn is_release_url(url: &str) -> bool {
    url.starts_with(&format!("https://github.com/{REPOSITORY}/releases/"))
}

/// The latest published (non-draft, non-prerelease) release, if any.
pub fn latest_release() -> Result<Option<Release>, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("LeagueAccounts/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"))
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .map_err(|error| error.to_string())?;
    // 404: the repository has no published release yet.
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let body = response
        .error_for_status()
        .map_err(|error| error.to_string())?
        .text()
        .map_err(|error| error.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|error| error.to_string())?;
    let tag = json["tag_name"].as_str().unwrap_or_default();
    let url = json["html_url"].as_str().unwrap_or_default();
    if parse_version(tag).is_none() || !is_release_url(url) {
        return Ok(None);
    }
    Ok(Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_owned(),
        name: json["name"].as_str().unwrap_or(tag).to_owned(),
        url: url.to_owned(),
        notes: json["body"].as_str().unwrap_or_default().chars().take(2000).collect(),
        published_at: json["published_at"].as_str().unwrap_or_default().to_owned(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v3.1.0", "3.0.0"));
        assert!(is_newer("3.0.1", "3.0.0"));
        assert!(is_newer("4", "3.9.9"));
        assert!(!is_newer("v3.0.0", "3.0.0"));
        assert!(!is_newer("2.0.3", "3.0.0"));
        assert!(!is_newer("3.1.0-beta", "3.1.0"));
        assert!(!is_newer("latest", "3.0.0"));
    }

    #[test]
    fn only_own_release_pages_are_allowed() {
        assert!(is_release_url(&format!("https://github.com/{REPOSITORY}/releases/tag/v3.1.0")));
        assert!(!is_release_url("https://github.com/someone/else/releases/tag/v9"));
        assert!(!is_release_url("https://evil.example/https://github.com/"));
    }
}
