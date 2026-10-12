//! New-version check against the GitHub releases of the app's repository, and
//! download and signature verification of the release executable.

use crate::logging::Reason;
use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use serde::Serialize;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Duration;

/// `owner/name` of the repository whose releases are checked.
pub const REPOSITORY: &str = "Tariolle/LeagueAccounts";
/// The release asset that replaces the running executable, and its signature.
const EXECUTABLE_ASSET: &str = "LeagueAccounts.exe";
const SIGNATURE_ASSET: &str = "LeagueAccounts.exe.sig";
/// Base64 minisign public key written by `npx tauri signer generate` (the
/// `.pub` file). Automatic installs stay off while it is empty.
const PUBLIC_KEY: &str = include_str!("../leagueaccounts.key.pub");
const MAX_EXECUTABLE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SIGNATURE_BYTES: u64 = 4096;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub name: String,
    pub url: String,
    pub notes: String,
    pub published_at: String,
    /// The signed executable, when the release carries one. Not sent to the UI.
    #[serde(skip)]
    pub executable: Option<Download>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Download {
    pub url: String,
    pub signature_url: String,
    pub size: u64,
}

#[derive(Debug)]
pub enum UpdateError {
    Request(reqwest::Error),
    Io(io::Error),
    /// Not the release, asset or size that was announced.
    Response,
    /// Missing, malformed or untrusted signature.
    Signature,
    /// Validly signed, but for a different version than the release.
    Version,
}

impl UpdateError {
    /// The log category; never the error text, which can contain URLs or paths.
    pub fn reason(&self) -> Reason {
        match self {
            Self::Request(error) => Reason::from_request(error),
            Self::Io(error) => Reason::from_error(error),
            Self::Response => Reason::ResponseBody,
            Self::Signature => Reason::Signature,
            Self::Version => Reason::VersionMismatch,
        }
    }
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(error) => error.fmt(f),
            Self::Io(error) => error.fmt(f),
            Self::Response => f.write_str("unexpected release download"),
            Self::Signature => f.write_str("the download is not signed by this app's key"),
            Self::Version => f.write_str("the download is signed for another version"),
        }
    }
}

impl From<reqwest::Error> for UpdateError {
    fn from(error: reqwest::Error) -> Self {
        Self::Request(error)
    }
}

impl From<io::Error> for UpdateError {
    fn from(error: io::Error) -> Self {
        // Body reads wrap the request error (a timeout, a reset) in io::Error.
        if !error.get_ref().is_some_and(|inner| inner.is::<reqwest::Error>()) {
            return Self::Io(error);
        }
        match error.into_inner().map(|inner| inner.downcast::<reqwest::Error>()) {
            Some(Ok(inner)) => Self::Request(*inner),
            _ => Self::Response,
        }
    }
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

/// Executables are downloaded only from `REPOSITORY`'s release assets.
fn is_asset_url(url: &str) -> bool {
    if url.starts_with(&format!("https://github.com/{REPOSITORY}/releases/download/")) {
        return true;
    }
    #[cfg(debug_assertions)]
    if let Some(feed) = debug_override("LEAGUEACCOUNTS_UPDATE_FEED") {
        return feed.rsplit_once('/').is_some_and(|(base, _)| url.starts_with(&format!("{base}/")));
    }
    false
}

/// Debug builds only: `LEAGUEACCOUNTS_UPDATE_FEED` (a releases/latest JSON URL)
/// and `LEAGUEACCOUNTS_UPDATE_KEY` let a local server stand in for GitHub.
#[cfg(debug_assertions)]
fn debug_override(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.trim().is_empty())
}

/// Debug builds replace their own executable only when pointed at a test feed.
pub fn can_install() -> bool {
    #[cfg(debug_assertions)]
    if debug_override("LEAGUEACCOUNTS_UPDATE_FEED").is_none() {
        return false;
    }
    !public_key().trim().is_empty()
}

fn public_key() -> String {
    #[cfg(debug_assertions)]
    if let Some(key) = debug_override("LEAGUEACCOUNTS_UPDATE_KEY") {
        return key;
    }
    PUBLIC_KEY.to_owned()
}

fn client(timeout: Duration) -> Result<reqwest::blocking::Client, UpdateError> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("LeagueAccounts/", env!("CARGO_PKG_VERSION")))
        .timeout(timeout)
        .build()?)
}

/// The latest published (non-draft, non-prerelease) release, if any.
pub fn latest_release() -> Result<Option<Release>, UpdateError> {
    let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
    #[cfg(debug_assertions)]
    let url = debug_override("LEAGUEACCOUNTS_UPDATE_FEED").unwrap_or(url);
    let response = client(Duration::from_secs(15))?
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()?;
    // 404: the repository has no published release yet.
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let body = response.error_for_status()?.text()?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|_| UpdateError::Response)?;
    Ok(parse_release(&json))
}

fn parse_release(json: &serde_json::Value) -> Option<Release> {
    let tag = json["tag_name"].as_str().unwrap_or_default();
    let url = json["html_url"].as_str().unwrap_or_default();
    if parse_version(tag).is_none() || !is_release_url(url) {
        return None;
    }
    Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_owned(),
        name: json["name"].as_str().unwrap_or(tag).to_owned(),
        url: url.to_owned(),
        notes: json["body"].as_str().unwrap_or_default().chars().take(2000).collect(),
        published_at: json["published_at"].as_str().unwrap_or_default().to_owned(),
        executable: executable_download(json),
    })
}

fn executable_download(json: &serde_json::Value) -> Option<Download> {
    let assets = json["assets"].as_array()?;
    let asset = |name: &str| {
        assets.iter().find(|asset| {
            asset["name"].as_str() == Some(name)
                && asset["state"].as_str().is_none_or(|state| state == "uploaded")
        })
    };
    let (executable, signature) = (asset(EXECUTABLE_ASSET)?, asset(SIGNATURE_ASSET)?);
    let url = executable["browser_download_url"].as_str().filter(|url| is_asset_url(url))?;
    let signature_url = signature["browser_download_url"].as_str().filter(|url| is_asset_url(url))?;
    let size = executable["size"]
        .as_u64()
        .filter(|size| (1..=MAX_EXECUTABLE_BYTES).contains(size))?;
    Some(Download {
        url: url.to_owned(),
        signature_url: signature_url.to_owned(),
        size,
    })
}

/// Stream the executable into `file`, reporting the bytes received so far.
pub fn download(download: &Download, file: &mut File, mut progress: impl FnMut(u64)) -> Result<(), UpdateError> {
    // Per read, not for the whole transfer: slow connections still finish.
    let mut response = client(Duration::from_secs(30))?
        .get(&download.url)
        .send()?
        .error_for_status()?;
    if response.content_length().is_some_and(|length| length != download.size) {
        return Err(UpdateError::Response);
    }
    let mut buffer = vec![0; 64 * 1024];
    let mut received = 0;
    loop {
        let read = response.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        received += read as u64;
        if received > download.size {
            return Err(UpdateError::Response);
        }
        file.write_all(&buffer[..read])?;
        progress(received);
    }
    if received != download.size {
        return Err(UpdateError::Response);
    }
    file.sync_all()?;
    Ok(())
}

/// The `.sig` asset: base64 text around a minisign signature.
pub fn signature(download: &Download) -> Result<String, UpdateError> {
    let response = client(Duration::from_secs(15))?
        .get(&download.signature_url)
        .send()?
        .error_for_status()?;
    let mut text = String::new();
    response.take(MAX_SIGNATURE_BYTES + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_SIGNATURE_BYTES {
        return Err(UpdateError::Signature);
    }
    Ok(text)
}

/// Check the file on disk, the bytes that will be installed, against the
/// embedded key, and that it was signed for `version` (`tauri signer sign
/// --app-version`), so a release can never install an older or mislabeled build.
pub fn verify_file(path: &Path, signature: &str, version: &str) -> Result<(), UpdateError> {
    verify(File::open(path)?, signature, &public_key(), version)
}

fn verify(mut data: impl Read, signature: &str, public_key: &str, version: &str) -> Result<(), UpdateError> {
    let key = decode_base64(public_key)
        .and_then(|text| PublicKey::decode(&text).ok())
        .ok_or(UpdateError::Signature)?;
    let signature = decode_base64(signature)
        .and_then(|text| Signature::decode(&text).ok())
        .ok_or(UpdateError::Signature)?;
    let mut verifier = key.verify_stream(&signature).map_err(|_| UpdateError::Signature)?;
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = data.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        verifier.update(&buffer[..read]);
    }
    // Also verifies the trusted comment, which carries the signed version.
    verifier.finalize().map_err(|_| UpdateError::Signature)?;
    if signed_version(signature.trusted_comment()) != Some(version.trim_start_matches(['v', 'V'])) {
        return Err(UpdateError::Version);
    }
    Ok(())
}

fn decode_base64(text: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(text.trim()).ok()?;
    String::from_utf8(bytes).ok()
}

/// "timestamp:1791820593\tfile:LeagueAccounts.exe\tversion:3.1.0" → "3.1.0".
fn signed_version(trusted_comment: &str) -> Option<&str> {
    trusted_comment
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"))
        .map(|version| version.trim_start_matches(['v', 'V']))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TEST_KEY: &str = include_str!("../tests/fixtures/update/test-key.pub");
    const OTHER_KEY: &str = include_str!("../tests/fixtures/update/other-key.pub");
    const PAYLOAD: &[u8] = include_bytes!("../tests/fixtures/update/payload.bin");
    /// `tauri signer sign --app-version 9.9.9` of PAYLOAD with the test key.
    const SIGNATURE: &str = include_str!("../tests/fixtures/update/payload.bin.sig");

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

    fn asset(name: &str, url: &str, size: u64) -> serde_json::Value {
        json!({ "name": name, "state": "uploaded", "size": size, "browser_download_url": url })
    }

    fn release(assets: Vec<serde_json::Value>) -> serde_json::Value {
        json!({
            "tag_name": "v3.1.0",
            "name": "v3.1.0",
            "html_url": format!("https://github.com/{REPOSITORY}/releases/tag/v3.1.0"),
            "body": "Notes",
            "published_at": "2026-10-12T00:00:00Z",
            "assets": assets,
        })
    }

    #[test]
    fn selects_the_signed_executable_from_own_release_assets() {
        let base = format!("https://github.com/{REPOSITORY}/releases/download/v3.1.0");
        let parsed = parse_release(&release(vec![
            asset("LeagueAccounts-setup.exe", &format!("{base}/LeagueAccounts-setup.exe"), 9),
            asset(SIGNATURE_ASSET, &format!("{base}/{SIGNATURE_ASSET}"), 400),
            asset(EXECUTABLE_ASSET, &format!("{base}/{EXECUTABLE_ASSET}"), 12_000_000),
        ]))
        .unwrap();
        assert_eq!(parsed.version, "3.1.0");
        assert_eq!(
            parsed.executable,
            Some(Download {
                url: format!("{base}/{EXECUTABLE_ASSET}"),
                signature_url: format!("{base}/{SIGNATURE_ASSET}"),
                size: 12_000_000,
            })
        );
        // The UI never receives download URLs.
        assert!(!serde_json::to_string(&parsed).unwrap().contains("download"));
    }

    #[test]
    fn releases_without_a_trusted_signed_executable_only_notify() {
        let base = format!("https://github.com/{REPOSITORY}/releases/download/v3.1.0");
        let exe = asset(EXECUTABLE_ASSET, &format!("{base}/{EXECUTABLE_ASSET}"), 12_000_000);
        let sig = asset(SIGNATURE_ASSET, &format!("{base}/{SIGNATURE_ASSET}"), 400);
        let elsewhere = asset(EXECUTABLE_ASSET, "https://evil.example/LeagueAccounts.exe", 12_000_000);
        let mut uploading = exe.clone();
        uploading["state"] = json!("new");
        let empty = asset(EXECUTABLE_ASSET, &format!("{base}/{EXECUTABLE_ASSET}"), 0);
        for assets in [vec![exe.clone()], vec![sig.clone()], vec![elsewhere, sig.clone()], vec![uploading, sig.clone()], vec![empty, sig]] {
            let parsed = parse_release(&release(assets)).unwrap();
            assert_eq!(parsed.version, "3.1.0");
            assert_eq!(parsed.executable, None);
        }
    }

    #[test]
    fn accepts_only_the_embedded_key_and_the_signed_version() {
        verify(PAYLOAD, SIGNATURE, TEST_KEY, "9.9.9").unwrap();
        verify(PAYLOAD, SIGNATURE, TEST_KEY, "v9.9.9").unwrap();

        let mut tampered = PAYLOAD.to_vec();
        tampered[0] ^= 1;
        assert!(matches!(verify(&tampered[..], SIGNATURE, TEST_KEY, "9.9.9"), Err(UpdateError::Signature)));
        assert!(matches!(verify(PAYLOAD, SIGNATURE, OTHER_KEY, "9.9.9"), Err(UpdateError::Signature)));
        assert!(matches!(verify(PAYLOAD, SIGNATURE, TEST_KEY, "9.9.10"), Err(UpdateError::Version)));
        assert!(matches!(verify(PAYLOAD, "not base64", TEST_KEY, "9.9.9"), Err(UpdateError::Signature)));
        assert!(matches!(verify(PAYLOAD, SIGNATURE, "", "9.9.9"), Err(UpdateError::Signature)));
    }

    /// Run by the release workflow on the signed build, before it is uploaded.
    #[test]
    #[ignore = "needs a signed release build"]
    fn release_artifact_verifies_with_the_embedded_key() {
        let exe = std::env::var("LEAGUEACCOUNTS_RELEASE_EXE").expect("LEAGUEACCOUNTS_RELEASE_EXE is not set");
        let signature = std::fs::read_to_string(format!("{exe}.sig")).unwrap();
        assert!(!PUBLIC_KEY.trim().is_empty(), "leagueaccounts.key.pub is empty");
        verify(File::open(&exe).unwrap(), &signature, PUBLIC_KEY, env!("CARGO_PKG_VERSION")).unwrap();
    }

    #[test]
    fn a_signature_without_a_version_is_rejected() {
        assert_eq!(signed_version("timestamp:1\tfile:LeagueAccounts.exe\tversion:3.1.0"), Some("3.1.0"));
        assert_eq!(signed_version("timestamp:1\tfile:LeagueAccounts.exe"), None);
    }
}
