# Automatic updates

How League Accounts replaces itself. The user-facing behavior is in the [README](../README.md#updates); releasing is under [Releasing](../README.md#releasing).

## Why the app replaces itself

Releases are a single portable `LeagueAccounts.exe`. Tauri's updater plugin installs only NSIS or MSI bundles on Windows, so the app downloads, verifies and swaps its own executable instead. Distribution and installation stay as they are.

## Flow

1. **Check** (`src/updates.rs`). `latest_release()` reads GitHub's `releases/latest` (published, not prerelease). A release is installed automatically only if it has both `LeagueAccounts.exe` and `LeagueAccounts.exe.sig`, uploaded under `https://github.com/Tariolle/LeagueAccounts/releases/download/`. The first check runs at launch, then every 6 hours, or 15 minutes after a failed check.
2. **Download.** The file is written to `<exe>.download` in the executable's folder: the same volume, so the swap is a rename. Its size must match the size GitHub announced.
3. **Verify.** The file on disk is checked against the minisign key in `leagueaccounts.key.pub`, compiled into the app. The signature's trusted comment must say `version:<release version>`: the release workflow signs with `tauri signer sign --app-version`. An older or mislabeled build is refused, even when signed.
4. **Swap** (`src/self_update.rs`). The running `<name>.exe` is renamed to `<name>.old.exe` (Windows allows renaming a running executable), then the download takes its name. If the second rename fails, the first is undone. The swap happens right after verification, so declining the restart, closing the app or a crash all still start the new version next time.
5. **Restart** (`src-tauri/src/updater.rs`).
   - **At launch:** right away from the update screen.
   - **Otherwise:** once the UI has no dialog, menu, login or rank refresh in progress and `update_idle` reports no login, refresh or `League of Legends.exe`, after a 10 s notice.
   - **Login safety:** `LoginControl::close_if_idle` closes login admission and refuses while a login runs, so a restart never interrupts a sign-in.
   - **Hand-over:** on `RunEvent::Exit`, after login cleanup, the app starts the new executable with its original arguments (keeping `--autostart`), plus `--updated-from <pid> --updated-to <version>`. It also passes `--minimized` when the window was minimized or not focused, so a restart never comes up in front of another app.
6. **Hand-over in the new process.** Before anything else, it waits up to 30 s for the previous process to exit, so the account files and the WebView2 profile are free. It then deletes `<name>.old.exe` and any `.download` older than an hour. If it reports a version older than `--updated-to`, the release was mislabeled: it logs `update_failed reason=version_mismatch` and only offers that release through the banner, so it cannot update in a loop.

## UI

- **Status.** `update_status` returns the state, and every change is an `update-status` event carrying the full state and an increasing `seq`. The UI subscribes before reading, so it misses nothing.
- **Launch hold.** The loading screen waits up to 3 s after page load for the launch check. When an update is downloading, the hextech loader stays up with "UPDATING LEAGUE ACCOUNTS" and a 200×16 progress bar (after the client's `lol-loading-screen`). *Continue in the background* shows the app; the restart notice follows later.
- **Banner fallback.** `available` (automatic updates off, no signed asset, or no public key yet) and `failed` (read-only folder, network, signature) show the existing "Update available" banner with a link to the release page.

## Settings

`autoUpdate`, on by default, replaces `checkUpdates`. The checks always run; turning `autoUpdate` off leaves only the banner. Settings saved with `checkUpdates: false` load as `autoUpdate: false`.

## Logging

The log events carry fixed codes only, never URLs or paths:

- `update_check_failed`
- `update_downloaded`
- `update_verification_failed`, with reason `signature` or `version_mismatch`
- `update_installed`
- `update_failed`
- `update_restarted`

## Keys

- **Private key:** `TAURI_SIGNING_PRIVATE_KEY` and its password are repository secrets, used only by `.github/workflows/release.yml`.
- **Public key:** `leagueaccounts.key.pub` is committed and compiled into the app. While it is empty, the app only shows the banner and the release workflow refuses to run.
- **Rotation:** a new key means apps built with the old one reject the next release, so users download it manually once.

## Testing

- **Unit tests.** `cargo test` covers:
  - asset selection;
  - signature, key and version checks, against fixtures signed with a throwaway test key whose private half is not kept;
  - swap, rollback and cleanup in temporary folders;
  - restart arguments;
  - waiting for a process to exit.
- **UI tests.** `tests/update.test.mjs` drives the browser mock through the launch screen, the restart notice and the banner.
- **Release check.** The release workflow verifies the signed build with the app's own code and embedded key before uploading it.
- **End to end on Windows.** Debug builds read two overrides, both compiled out of release builds:
  - `LEAGUEACCOUNTS_UPDATE_FEED`: a `releases/latest`-style JSON URL whose assets live next to it;
  - `LEAGUEACCOUNTS_UPDATE_KEY`: the base64 public key.

  Without the feed override, a debug build never replaces itself. To run it:
  1. Build with `npx tauri build --debug --no-bundle`.
  2. Sign a copy with a test key and `--app-version`, and serve it with a matching JSON from `127.0.0.1`.
  3. Run the build with both overrides, plus `APPDATA` and `WEBVIEW2_USER_DATA_FOLDER` pointed at a scratch folder, so real accounts stay untouched.
