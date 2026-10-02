//! Riot Client integration: locate and open the client, read its sign-in
//! state from the local API, type credentials into its login window, and
//! launch a game once sign-in is confirmed.
//!
//! Credentials are only typed after the auth service is ready, has reported
//! "signed out" continuously for `SIGNED_OUT_STABLE`, and the client's main
//! window is visible and focused. While the client starts, a session kept by
//! the background service can briefly look signed out before it is restored,
//! so a single "signed out" reading is never enough. The game is launched
//! only after the API confirms the intended account is signed in.

use crate::autotype::type_credentials;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const LOGIN_TIMEOUT: Duration = Duration::from_secs(60);
/// After typing, how long to wait for sign-in (2FA or captcha may need the user).
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(90);
const SIGNED_OUT_STABLE: Duration = Duration::from_secs(4);
const WINDOW_STABLE: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Game {
    League,
    Tft,
}

impl Game {
    fn product(self) -> &'static str {
        match self {
            Game::League => "league_of_legends",
            Game::Tft => "teamfighttactics",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginOutcome {
    /// Credentials were typed and the account is now signed in.
    SignedIn,
    /// Credentials were typed but sign-in was not confirmed in time
    /// (for example the user still has to enter a 2FA code).
    Typed,
    /// This account was already signed in; nothing was typed.
    AlreadySignedIn,
    /// A different account is signed in; nothing was typed.
    OtherAccount,
}

/// Progress reported to the UI while signing in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginStep {
    CloseLeague,
    OpenClient,
    WaitAuth,
    SignOut,
    FindWindow,
    Focus,
    Type,
    Confirm,
    LaunchGame,
    Done,
}

impl LoginStep {
    pub fn code(self) -> &'static str {
        match self {
            LoginStep::CloseLeague => "closeLeague",
            LoginStep::OpenClient => "openClient",
            LoginStep::WaitAuth => "waitAuth",
            LoginStep::SignOut => "signOut",
            LoginStep::FindWindow => "findWindow",
            LoginStep::Focus => "focus",
            LoginStep::Type => "type",
            LoginStep::Confirm => "confirm",
            LoginStep::LaunchGame => "launchGame",
            LoginStep::Done => "done",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginError {
    ClientMissing,
    LaunchFailed,
    /// The login window or sign-in state never became observable.
    Timeout,
    /// Signing out of the current session is not available.
    SignOutFailed,
    TypingFailed,
    /// Cancelled from the UI.
    Cancelled,
    /// A League/TFT client or match is open for another account and closing
    /// it was not confirmed.
    LeagueRunning,
    /// The League/TFT client did not close.
    CloseFailed,
    /// Signed in, but the game client did not start.
    GameLaunchFailed,
}

impl LoginError {
    pub fn code(self) -> &'static str {
        match self {
            LoginError::ClientMissing => "riot_client_missing",
            LoginError::LaunchFailed => "riot_launch_failed",
            LoginError::Timeout => "riot_timeout",
            LoginError::SignOutFailed => "riot_sign_out_failed",
            LoginError::TypingFailed => "autotype_failed",
            LoginError::Cancelled => "login_cancelled",
            LoginError::LeagueRunning => "league_running",
            LoginError::CloseFailed => "league_close_failed",
            LoginError::GameLaunchFailed => "game_launch_failed",
        }
    }
}

fn program_data() -> PathBuf {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
}

/// Path of `RiotClientServices.exe` from Riot's install registry file.
pub fn client_path() -> Option<PathBuf> {
    let installs = program_data().join(r"Riot Games\RiotClientInstalls.json");
    let from_file = std::fs::read_to_string(installs)
        .ok()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|value| {
            ["rc_default", "rc_live"]
                .iter()
                .find_map(|key| value[*key].as_str().map(PathBuf::from))
        })
        .filter(|path| path.exists());
    from_file.or_else(|| {
        let fallback = PathBuf::from(r"C:\Riot Games\Riot Client\RiotClientServices.exe");
        fallback.exists().then_some(fallback)
    })
}

/// Whether Riot's metadata lists the product's live patchline as installed.
pub fn game_installed(game: Game) -> bool {
    program_data()
        .join(r"Riot Games\Metadata")
        .join(format!("{}.live", game.product()))
        .exists()
}

fn open_client(path: &PathBuf) -> bool {
    std::process::Command::new(path).spawn().is_ok()
}

/// Process names that show a game client has started.
fn game_processes(game: Game) -> &'static [&'static str] {
    match game {
        Game::League => &["leagueclient.exe", "leagueclientux.exe", "league of legends.exe"],
        Game::Tft => &["tftclient.exe", "leagueclient.exe", "leagueclientux.exe"],
    }
}

fn game_started(game: Game) -> bool {
    let processes = running_processes();
    game_processes(game).iter().any(|name| processes.contains(*name))
}

/// Start `game` for the signed-in account and wait until its client process
/// appears. A running Riot Client only selects the product for
/// `--launch-product` ("willAutoLaunch=false"), so the local API is used;
/// the command line remains a last resort.
fn launch_game(path: &PathBuf, game: Game, cancel: &AtomicBool) -> bool {
    if game_started(game) {
        return true;
    }
    let mut requested = false;
    for attempt in 0..40 {
        if cancel.load(Ordering::SeqCst) {
            return false;
        }
        if !requested && attempt % 2 == 0 {
            requested = LocalApi::read().is_some_and(|api| api.launch(game));
            if !requested && attempt == 10 {
                // API refused repeatedly: try the command line once.
                let _ = std::process::Command::new(path)
                    .arg(format!("--launch-product={}", game.product()))
                    .arg("--launch-patchline=live")
                    .spawn();
            }
        }
        if game_started(game) {
            return true;
        }
        thread::sleep(Duration::from_secs(1));
    }
    game_started(game)
}

/// Launch the requested game; TFT falls back to the League client, which
/// also hosts TFT, when the standalone TFT client cannot start.
fn start_game(path: &PathBuf, game: Game, cancel: &AtomicBool) -> Result<(), LoginError> {
    if launch_game(path, game, cancel) || (game == Game::Tft && launch_game(path, Game::League, cancel)) {
        Ok(())
    } else if cancel.load(Ordering::SeqCst) {
        Err(LoginError::Cancelled)
    } else {
        Err(LoginError::GameLaunchFailed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Session {
    SignedIn,
    SignedOut,
    Unknown,
}

/// The Riot Client's local HTTPS API, described by its lockfile.
struct LocalApi {
    base: String,
    password: String,
    client: reqwest::blocking::Client,
}

impl LocalApi {
    fn read() -> Option<Self> {
        let lockfile = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)?
            .join(r"Riot Games\Riot Client\Config\lockfile");
        let contents = std::fs::read_to_string(lockfile).ok()?;
        // name:pid:port:password:protocol
        let parts: Vec<&str> = contents.trim().split(':').collect();
        let [_, _, port, password, _] = parts[..] else {
            return None;
        };
        let client = reqwest::blocking::Client::builder()
            // The API is bound to loopback with a self-signed certificate.
            .danger_accept_invalid_certs(true)
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .ok()?;
        Some(Self {
            base: format!("https://127.0.0.1:{}", port.parse::<u16>().ok()?),
            password: password.to_owned(),
            client,
        })
    }

    fn functions(&self) -> Option<HashSet<String>> {
        let body = self
            .client
            .get(format!("{}/help", self.base))
            .basic_auth("riot", Some(&self.password))
            .send()
            .ok()?
            .text()
            .ok()?;
        let help: serde_json::Value = serde_json::from_str(&body).ok()?;
        Some(help["functions"].as_object()?.keys().cloned().collect())
    }

    fn get(&self, path: &str) -> Option<(u16, serde_json::Value)> {
        let response = self
            .client
            .get(format!("{}{path}", self.base))
            .basic_auth("riot", Some(&self.password))
            .send()
            .ok()?;
        let status = response.status().as_u16();
        let body = response.text().ok().unwrap_or_default();
        Some((status, serde_json::from_str(&body).unwrap_or_default()))
    }

    fn session(&self) -> Session {
        // Until the auth service reports ready, "not authorized" may only
        // mean the stored session has not been restored yet.
        let ready = self
            .get("/rso-auth/configuration/v3/ready-state")
            .is_some_and(|(status, body)| status == 200 && body["ready"] == true);
        if !ready {
            return Session::Unknown;
        }
        let Some((authorization, _)) = self.get("/rso-auth/v1/authorization") else {
            return Session::Unknown;
        };
        let session_type = self
            .get("/rso-auth/v1/session")
            .and_then(|(_, body)| body["type"].as_str().map(str::to_owned));
        match (authorization, session_type.as_deref()) {
            (200, _) | (_, Some("authenticated")) => Session::SignedIn,
            (404, _) => Session::SignedOut,
            _ => Session::Unknown,
        }
    }

    /// Ask the client to launch a product; true when the request is accepted.
    fn launch(&self, game: Game) -> bool {
        self.client
            .post(format!(
                "{}/product-launcher/v1/products/{}/patchlines/live",
                self.base,
                game.product()
            ))
            .basic_auth("riot", Some(&self.password))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body("{}")
            .send()
            .is_ok_and(|response| response.status().is_success())
    }

    /// Username of the signed-in account, if any.
    fn username(&self) -> Option<String> {
        let (status, body) = self.get("/rso-auth/v1/authorization/userinfo")?;
        if status != 200 {
            return None;
        }
        // `userInfo` is itself a JSON document encoded as a string.
        let info: serde_json::Value = match &body["userInfo"] {
            serde_json::Value::String(text) => serde_json::from_str(text).ok()?,
            other => other.clone(),
        };
        info["username"].as_str().map(str::to_owned)
    }

    fn sign_out(&self) -> bool {
        if !self
            .functions()
            .is_some_and(|functions| functions.contains("DeleteRsoAuthV1Session"))
        {
            return false;
        }
        self.client
            .delete(format!("{}/rso-auth/v1/session", self.base))
            .basic_auth("riot", Some(&self.password))
            .send()
            .is_ok_and(|response| response.status().is_success())
    }
}

/// League/TFT client processes (closing them only ends the lobby).
const CLIENT_PROCESSES: [&str; 4] = [
    "leagueclient.exe",
    "leagueclientux.exe",
    "leagueclientuxrender.exe",
    "tftclient.exe",
];
/// The in-match game process (closing it abandons the match).
const GAME_PROCESS: &str = "league of legends.exe";

fn running_processes() -> HashSet<String> {
    let mut command = std::process::Command::new("tasklist");
    command.args(["/FO", "CSV", "/NH"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let Ok(output) = command.output() else {
        return HashSet::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split("\",\"").next())
        .map(|name| name.trim_matches('"').to_ascii_lowercase())
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameStatus {
    pub client_open: bool,
    pub in_game: bool,
    /// Username signed in to the Riot Client, when the API answers.
    pub signed_in_user: Option<String>,
}

impl GameStatus {
    pub fn league_running(&self) -> bool {
        self.client_open || self.in_game
    }

    pub fn signed_in_as(&self, account_id: &str) -> bool {
        self.signed_in_user
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(account_id))
    }
}

pub fn status() -> GameStatus {
    let processes = running_processes();
    let signed_in_user = LocalApi::read()
        .filter(|api| api.session() == Session::SignedIn)
        .and_then(|api| api.username());
    GameStatus {
        client_open: CLIENT_PROCESSES.iter().any(|name| processes.contains(*name)),
        in_game: processes.contains(GAME_PROCESS),
        signed_in_user,
    }
}

/// Close the League/TFT client and game: politely first, then forcefully.
fn close_league() -> bool {
    let names: Vec<&str> = CLIENT_PROCESSES.iter().copied().chain([GAME_PROCESS]).collect();
    let taskkill = |force: bool| {
        let mut command = std::process::Command::new("taskkill");
        if force {
            command.arg("/F");
        }
        for name in &names {
            command.args(["/IM", name]);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let _ = command.output();
    };
    let closed = || {
        let processes = running_processes();
        !names.iter().any(|name| processes.contains(*name))
    };
    taskkill(false);
    for attempt in 0..20 {
        thread::sleep(Duration::from_millis(500));
        if closed() {
            return true;
        }
        if attempt == 10 {
            taskkill(true);
        }
    }
    closed()
}

/// Open the Riot Client and sign in with the credentials, then launch `game`
/// (when given) once the account is confirmed signed in.
///
/// If another account is signed in, nothing is typed and `OtherAccount` is
/// returned, unless `switch_account` asks to sign it out first.
#[allow(clippy::too_many_arguments)]
pub fn login(
    account_id: &str,
    password: &str,
    game: Option<Game>,
    switch_account: bool,
    close_running: bool,
    progress: &mut dyn FnMut(LoginStep),
    cancel: &AtomicBool,
) -> Result<LoginOutcome, LoginError> {
    let cancelled = || cancel.load(Ordering::SeqCst);
    let path = client_path().ok_or(LoginError::ClientMissing)?;

    // Never sign another account in under an open League client or match.
    let current = status();
    if current.league_running() && !current.signed_in_as(account_id) {
        if !close_running {
            return Err(LoginError::LeagueRunning);
        }
        progress(LoginStep::CloseLeague);
        if !close_league() {
            return Err(LoginError::CloseFailed);
        }
    }
    progress(LoginStep::OpenClient);
    if !open_client(&path) {
        return Err(LoginError::LaunchFailed);
    }
    progress(LoginStep::WaitAuth);
    let mut reported = LoginStep::WaitAuth;
    let mut report = |step: LoginStep, progress: &mut dyn FnMut(LoginStep)| {
        if step != reported {
            reported = step;
            progress(step);
        }
    };
    let is_this_account =
        |api: &LocalApi| api.username().is_some_and(|name| name.eq_ignore_ascii_case(account_id));

    let started = Instant::now();
    let mut sign_out_sent: Option<Instant> = None;
    let mut signed_out_since: Option<Instant> = None;
    let mut window_since: Option<Instant> = None;
    let window = loop {
        if cancelled() {
            return Err(LoginError::Cancelled);
        }
        if started.elapsed() > LOGIN_TIMEOUT {
            return Err(LoginError::Timeout);
        }
        // The lockfile is rewritten when the client restarts; re-read it.
        let api = LocalApi::read();
        let session = api.as_ref().map_or(Session::Unknown, LocalApi::session);
        if session != Session::SignedOut {
            signed_out_since = None;
        }
        match (session, api) {
            (Session::SignedIn, Some(api)) if is_this_account(&api) => {
                if let Some(game) = game {
                    report(LoginStep::LaunchGame, progress);
                    start_game(&path, game, cancel)?;
                }
                progress(LoginStep::Done);
                return Ok(LoginOutcome::AlreadySignedIn);
            }
            (Session::SignedIn, _) if !switch_account => return Ok(LoginOutcome::OtherAccount),
            (Session::SignedIn, api) => match sign_out_sent {
                None => {
                    report(LoginStep::SignOut, progress);
                    if !api.is_some_and(|api| api.sign_out()) {
                        return Err(LoginError::SignOutFailed);
                    }
                    sign_out_sent = Some(Instant::now());
                }
                Some(sent) if sent.elapsed() > Duration::from_secs(15) => {
                    return Err(LoginError::SignOutFailed);
                }
                Some(_) => {}
            },
            (Session::SignedOut, _) => {
                report(LoginStep::FindWindow, progress);
                let since = *signed_out_since.get_or_insert_with(Instant::now);
                match windows::find_client_window() {
                    Some(window) => {
                        let shown = *window_since.get_or_insert_with(Instant::now);
                        if since.elapsed() >= SIGNED_OUT_STABLE && shown.elapsed() >= WINDOW_STABLE {
                            break window;
                        }
                    }
                    None => window_since = None,
                }
            }
            (Session::Unknown, _) => {}
        }
        thread::sleep(POLL);
    };

    progress(LoginStep::Focus);
    if !windows::focus(window) {
        return Err(LoginError::TypingFailed);
    }
    // Let the login form take focus, then confirm nothing changed meanwhile.
    thread::sleep(Duration::from_millis(1200));
    let still_signed_out = LocalApi::read().is_some_and(|api| api.session() == Session::SignedOut);
    if cancelled() {
        return Err(LoginError::Cancelled);
    }
    if !still_signed_out || !windows::foreground_is(window) {
        return Err(LoginError::TypingFailed);
    }
    progress(LoginStep::Type);
    if !type_credentials(account_id, password) {
        return Err(LoginError::TypingFailed);
    }
    progress(LoginStep::Confirm);

    // Launch the game only once this account is confirmed signed in.
    let typed = Instant::now();
    while typed.elapsed() < SIGN_IN_TIMEOUT {
        if cancelled() {
            return Ok(LoginOutcome::Typed);
        }
        thread::sleep(Duration::from_secs(1));
        if let Some(api) = LocalApi::read() {
            if api.session() == Session::SignedIn && is_this_account(&api) {
                if let Some(game) = game {
                    progress(LoginStep::LaunchGame);
                    start_game(&path, game, cancel)?;
                }
                progress(LoginStep::Done);
                return Ok(LoginOutcome::SignedIn);
            }
        }
    }
    Ok(LoginOutcome::Typed)
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYEVENTF_KEYUP, VK_MENU,
    };
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow,
        SW_RESTORE,
    };

    /// Executable names of the Riot Client UI across client generations.
    const CLIENT_UI: [&str; 2] = ["riot client.exe", "riotclientux.exe"];

    fn process_name(window: HWND) -> Option<String> {
        let mut pid = 0u32;
        // SAFETY: `window` comes from EnumWindows; `pid` is a valid out pointer.
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if pid == 0 {
            return None;
        }
        // SAFETY: plain query handle, closed below.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        // SAFETY: buffer and length describe a writable UTF-16 buffer.
        let ok = unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) };
        // SAFETY: handle opened above.
        unsafe { CloseHandle(process) };
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        path.rsplit('\\').next().map(str::to_ascii_lowercase)
    }

    unsafe extern "system" fn collect(window: HWND, found: LPARAM) -> i32 {
        // SAFETY: `found` is the &mut Option<HWND> passed to EnumWindows.
        let found = unsafe { &mut *(found as *mut Option<HWND>) };
        // SAFETY: window handles from EnumWindows are valid for these queries.
        let visible = unsafe { IsWindowVisible(window) != 0 && GetWindowTextLengthW(window) > 0 };
        // Skip splash and helper windows: the login UI is a large window
        // (or minimized, in which case focus() restores it).
        let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        // SAFETY: rect is a valid out pointer.
        let large = unsafe {
            IsIconic(window) != 0
                || (GetWindowRect(window, &mut rect) != 0
                    && rect.right - rect.left >= 600
                    && rect.bottom - rect.top >= 400)
        };
        if visible && large && process_name(window).is_some_and(|name| CLIENT_UI.contains(&name.as_str())) {
            *found = Some(window);
            return 0;
        }
        1
    }

    pub fn find_client_window() -> Option<HWND> {
        let mut found: Option<HWND> = None;
        // SAFETY: the callback only writes through the pointer we pass in.
        unsafe { EnumWindows(Some(collect), &mut found as *mut _ as LPARAM) };
        found
    }

    pub fn focus(window: HWND) -> bool {
        // SAFETY: window is a live top-level handle. A tap of Alt satisfies
        // Windows' foreground-lock rules for SetForegroundWindow.
        unsafe {
            if IsIconic(window) != 0 {
                ShowWindow(window, SW_RESTORE);
            }
            keybd_event(VK_MENU as u8, 0, 0, 0);
            keybd_event(VK_MENU as u8, 0, KEYEVENTF_KEYUP, 0);
            SetForegroundWindow(window) != 0
        }
    }

    pub fn foreground_is(window: HWND) -> bool {
        // SAFETY: read-only query.
        unsafe { GetForegroundWindow() == window }
    }
}

#[cfg(not(windows))]
mod windows {
    pub type HWND = usize;
    pub fn find_client_window() -> Option<HWND> {
        None
    }
    pub fn focus(_: HWND) -> bool {
        false
    }
    pub fn foreground_is(_: HWND) -> bool {
        false
    }
}
