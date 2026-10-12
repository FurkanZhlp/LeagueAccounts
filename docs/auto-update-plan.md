# Automatic updates: plan and open questions

**Status:** research done, implementation not started. Answer the [open questions](#open-questions), then [resume](#resuming).

## Goal

Replace "Update available → download from the release page" with Steam-like self-updating: when a new release is detected, League Accounts downloads it, verifies it, replaces its own executable and restarts into the new version, with no steps for the user.

## What exists today

- **Distribution.** A single portable `LeagueAccounts.exe` (built with `npx tauri build --no-bundle`), downloaded from GitHub Releases and run from any folder. There is no installer; the NSIS target in `tauri.conf.json` is not used for releases.
- **Releases are manual.** A version-bump commit (for example `chore: prepare v3.0.3 release`: `Cargo.toml`, `src-tauri/Cargo.toml`, `tauri.conf.json`, `package.json` and locks), then a GitHub release with `LeagueAccounts.exe` attached. `.github/workflows/rust.yml` only builds and tests; there is no release workflow.
- **`src/updates.rs`.** `latest_release()` reads the GitHub API `releases/latest` (published, non-prerelease). It also provides `is_newer()`, `is_release_url()` and `Release { version, name, url, notes, published_at }`. Assets are not read yet.
- **`src-tauri/src/refresh.rs`.** `spawn_update_checker()` runs a first check after 20 s, then every 6 h. It emits `update-available` when `Settings.check_updates` is on.
- **`src-tauri/src/main.rs`.**
  - The `check_update` command backs Settings › Check now, and `open_release` opens the release page.
  - The autostart entry passes `--autostart`; with Start minimized, the window then starts minimized.
  - There is no single-instance lock.
- **`src-tauri/src/login_control.rs`.** `LoginControl` exit admission blocks new logins and waits for a running one before the app exits. The restart should reuse it.
- **UI** (`ui/src/main.ts`).
  - `showUpdate()` shows the banner with Download and Later; dismissal is stored per version in `localStorage`.
  - `checkUpdateNow()` runs from Settings.
  - The `?scene=update` dev scene and `ui/src/mock.ts` provide the browser preview.
- **#17** adds the client's hextech Lottie loader (`ui/src/loader.ts`), which the update screen can reuse.

## Constraint

Tauri's official updater (`tauri-plugin-updater`) installs only NSIS or MSI bundles on Windows; it cannot replace a portable executable. The app has to replace itself. Switching distribution to an installer would also work, but it changes how everyone installs, and current users would still need one manual migration.

## Proposed design

1. **Release assets.** Each release carries `LeagueAccounts.exe` and `LeagueAccounts.exe.sig`, a minisign signature made with `npx tauri signer sign`; the key comes from `npx tauri signer generate`. The public key is compiled into the app; the private key never is.
2. **Check.** `latest_release()` also returns the executable and signature download URLs. Only `https://github.com/Tariolle/LeagueAccounts/releases/download/…` is accepted.
3. **Download.** Stream to `<exe folder>/<name>.download`: the same volume, so the final rename is atomic. Emit progress events, using the asset size from the API.
4. **Verify.** Check the file with `minisign-verify` against the embedded public key; Tauri's `.sig` is base64 around a minisign signature. Optionally run the new exe with `--version` and compare versions before swapping.
5. **Swap.** Rename the running `<name>.exe` to `<name>.old.exe` (Windows allows renaming a running executable), then rename `.download` to `<name>.exe`. If the second step fails, undo the first. Use the current executable's own name; users may have renamed it.
6. **Restart.**
   - Close login admission through `LoginControl`.
   - Start the new exe with the same arguments (which keeps `--autostart` and Start minimized) plus `--updated-from <pid>`, then exit.
   - The new process waits for that pid to end before loading accounts and settings, then deletes `<name>.old.exe`.
7. **At launch (Steam-like).** Check right away, without the current 20 s delay.
   - **Update found:** show an update screen, then restart. It combines the hextech loader with "UPDATING LEAGUE ACCOUNTS" and a progress bar, styled after the client's own loading screen (`lol-loading-screen`: Beaufort 18px caps in `#c8aa6e`, 200×16 progress bar).
   - **Offline or timeout:** start normally.
8. **Fallbacks.** On a read-only folder, a failed download or a failed verification: keep the current version, log a fixed event code, and show the existing "Update available" banner for a manual download.
9. **UI wiring.** An `update_status` command returns the current state on load, and `update-progress` events follow, so an event sent before the UI subscribes is not missed.
10. **Logging.** Add new `Event`s with fixed codes only, never URLs or paths, as the log-privacy tests require: update check failed, downloaded, verification failed, installed, failed.

## Open questions

1. **An update is found while the app is open** (during the background check every 6 h):
   - **A. Restart when idle (recommended).**
     - Download silently.
     - Restart automatically once no login, dialog or refresh is in progress.
     - First show a short notice, "Restarting to update in 10 s", with *Restart now* and *Later*. *Later* applies the update at the next launch.
   - **B. Apply at the next launch only.** The update screen appears at startup; an open session is never interrupted.
   - **C. Restart immediately.** A running login still finishes first.
2. **Release pipeline and signing:**
   - **A. GitHub Actions on a `vX.Y.Z` tag (recommended).**
     - Builds and signs with a key stored in repository secrets (`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`).
     - Creates a draft release with both files; you write the notes and publish.
   - **B. Local `npm run release` script.** Builds and signs on your PC, so the key never leaves it; you upload both files.
3. **Opt-out:**
   - **A. Setting, on by default (recommended).** "Check for updates automatically" becomes "Update automatically"; when off, only the banner appears.
   - **B. Always on.** The setting is removed.

## Steps only the maintainer can do

- **Generate the signing key** and keep the private key secret and backed up. If it is lost, users must download one release manually.
  ```
  npx tauri signer generate -w "%USERPROFILE%\.tauri\leagueaccounts.key"
  ```
- **With 2A:** store the private key and its password as the two repository secrets above, for example with `gh secret set`.
- **Share the public key** (`leagueaccounts.key.pub`), which the implementation commits and embeds.

## Verification plan

- **Rust unit tests:**
  - asset selection from release JSON;
  - signature verification with a test key pair and fixtures;
  - swap and rollback in a temporary folder;
  - `.old` cleanup;
  - waiting for `--updated-from`.
- **End to end on Windows.** A debug-only feed override, compiled out of release builds, serves a signed test build with a higher version from `127.0.0.1`. Check:
  - download → verify → swap → restart → cleanup;
  - offline, bad signature, read-only folder and login-in-progress cases.
- **UI tests:**
  - an `?scene=updating` browser scene with simulated progress;
  - the Settings label and toggle;
  - the banner fallback.
- **First release.** Users on v3.0.3 or earlier still update manually once, because their version only notifies. Automatic updates apply from the following release.

## Resuming

- **Branch.** `claude/auto-update-plan`, stacked on #17 (itself on #16), because the update screen reuses #17's loader. Retarget to `main` once #16 and #17 merge.
- **Prompt.** Ask Claude: *"Resume the auto-update plan in `docs/auto-update-plan.md`. Answers: 1A, 2A, 3A"* (or your choices).
- **Afterwards.** The implementation PR removes this file, or turns it into a short design note.
- **Unrelated, still open from the #16 review.** The packaged app's CSP drops inline `style` attributes once a `<style>` block is present (`ui/index.html`).
