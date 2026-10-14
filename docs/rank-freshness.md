# Rank freshness investigation and implementation

Observed on Windows on 2026-10-14. These are bounded experiments, not a load test
or a guarantee about every region/account. Riot API was not used directly.

## Findings

The original path was Account → OP.GG LoL GET → HTML/meta/React parser → RankInfo,
followed by a separate TFT GET/parser. Both manual and scheduled refreshes used
four account workers and never invoked OP.GG's Update button. A successful GET
could therefore store a profile cached by OP.GG for days. There was no per-source
timestamp, and fetch failures replaced the last known standing with Error.

| Option | Observed freshness | Force update | Several accounts | Access | Decision |
|---|---|---|---|---|---|
| OP.GG GET | Explicit source timestamp; two profiles over a day old | No | Ordinary GET works | No challenge observed | Keep existing parsing |
| OP.GG Update | New timestamps verified in subsequent HTML for 3/3 profiles | Yes, subject to renewableAt | Two updates concurrently worked | No cookies/login/CSRF token needed in this test | Implement |
| Official OP.GG MCP | Profile/level/solo rank/LP/W-L and timestamps returned | No renewal tool in live tools/list | Not load-tested | No authorization required in this test | No freshness benefit; don't add |
| LeagueOfGraphs | Unverifiable: profile GETs returned a challenge | Unverifiable | Three bounded profile requests returned 403 | Cloudflare "Just a moment..." | Not a usable automatic fallback here |

Official MCP source: https://github.com/opgginc/opgg-mcp, inspected at commit
`039904bf655927402c28717c12bb51fe949e2d61`. The live server at https://mcp-api.op.gg/mcp
listed 29 tools, including `lol_get_summoner_profile` and
`lol_list_summoner_matches`, but none for renewal. Calling historical
`lol-summoner-renewal` returned JSON-RPC -32601 (tool not found), despite HTTP 200.
Profile calls took 3.69–4.20 seconds and returned compact class-like text,
not plain profile JSON. No rate-limit ceiling was established.

## OP.GG renewal contract

The live public site uses Next.js server actions, not the historical REST URL.
POST the **same LoL profile URL** with:

- `Next-Action`: action reference discovered from the page's own JavaScript build;
- `Origin: https://op.gg`;
- `Accept: text/x-component`;
- `Content-Type: text/plain;charset=UTF-8`;
- JSON body `[{"region":"kr","puuid":"FULL_PROFILE_PUUID","isPremiumPrimary":false}]`.

The `renewalStatus` action takes `[{"region":"kr","puuid":"FULL_PROFILE_PUUID"}]`.
Its response is a React Flight stream; the result row contains `status`, possibly
`delay` in milliseconds, `lastUpdatedAt` and `renewableAt`. Observed flow:
`RENEWING` → poll status → `RENEWAL_FINISH` → GET profile → verify timestamp.
Other documented client statuses include RENEWAL_FAILED, REQUEST_FAILED,
TOO_MANY_RENEWALS and UNKNOWN. HTTP 200 alone proves nothing.

Reference hashes in the captured build were
`405a04669583947dc03eb8c7f367adf28c8f714e86` (renewal) and
`400c02bdfd8c90756a329b312a7455e73880ad43ec` (status).
They appear only in test fixtures/research, **not pinned in production code**.
Production discovers the references and caches them once per release build,
shared across account workers. A changed build triggers discovery again.
Initial discovery required 16 ordinary asset GETs in the Python experiment;
subsequent renewals in the same process reuse the references.

Use the PUUID in the **Update component** beside `initRenewableAt`/`initUpdatedAt`.
A generic first-PUUID search initially picked a short/other occurrence and
returned application status UNKNOWN/404. The probe was corrected before the
successful runs, and an offline regression protects this distinction.

Corrected experiment, at most two simultaneous renewal sequences:

| Public Riot ID | Before, source time (+09:00) | After, source time (+09:00) | POST + polling |
|---|---|---|---|
| Hide on bush#KR1 | Oct 14 15:49:57 | Oct 14 15:52:49 | 2.969 s |
| Gumayusi#KR1 | Oct 12 23:45:06 | Oct 14 15:52:47 | 2.969 s |
| Oner#KR1 | Oct 11 22:06:57 | Oct 14 15:52:54 | 2.156 s |

Repeated profile GETs retained the new timestamps. No 403/429 was observed on
OP.GG. Observed renewableAt was about two minutes after lastUpdatedAt. This is
not proof of an account-wide rate limit or a supported public API guarantee.
An additional Agurin#EUW request returned 404 and was not counted as a successful
renewal. LeagueOfGraphs returned challenges for all three URLs tested.

The actual Rust `reqwest` implementation was also exercised on the three valid
profiles concurrently: total 7.467 s including LoL and TFT. Two profiles renewed
and produced verified fresh timestamps. The third was within OP.GG's renewableAt
cooldown and correctly skipped renewal even in Force mode. Ranked and unranked
profiles and independent TFT parsing succeeded. This verifies the network path
used by the application, not just the Python experiment.

## Chosen architecture and changes

Account → LoL GET → inspect age/cooldown → optionally discover actions and renew
→ bounded status polling → new LoL GET → existing parser → RankInfo/provenance.
TFT keeps a separate GET/parser and separate provenance; its source age remains
unknown. LoL renewal does not establish TFT freshness.

Policies are named constants in `src/rank_fetcher/renewal.rs`: manual max age
2 minutes, automatic max age 15 minutes, two simultaneous renewal sequences,
20-second renewal budget including discovery/slot wait/status polling. Polling
uses server delay with bounded backoff. Each page GET has the existing
20-second HTTP timeout; the complete account job can take longer than 20 seconds.
Force skips the local age threshold but respects renewableAt, duplicate attempts
and provider rate-limit cooldowns. Failed attempts are deduplicated for a minute.

Freshness is based on source time, never fetchedAt. FINISH must be followed by
HTML whose source time is at least the reported completion time and within the
age threshold. Otherwise the result remains stale. Renewal failures retain the
parsed cached profile; GET/parsing failures retain the saved standing. No
unreachable provider is silently treated as Unranked. Missing profiles, rate
limits, blocked access, unavailable providers, parsing failures and timeouts are
separate error categories. Confirmed Unranked still replaces an old ranked result.

`models.rs` persists optional, backward-compatible LoL/TFT provenance (source,
fetch/source timestamps, freshness, renewal attempted/succeeded, error).
`rank_fetcher.rs` preserves the existing rank/history/level parser, adds the
refresh sequence, and prevents an explicit Lv./Unranked description from taking
another player's or historical tier. Invalid TFT payloads become parsing failures.
`refresh.rs`, `main.rs` and `views.rs` pass refresh mode and safe provenance to
the UI. UI changes are one status line per card, six translations, and force
refresh via Ctrl+Shift+R / Shift+F5. Login credentials never go to these providers.
Offline fixtures/regressions and opt-in Python/Rust diagnostic tools are included.

## Reproduce the network experiment

From the repository root, on public profiles only:

```powershell
py -3 tools/provider_probe.py --renew --parallel 2 --mcp --leagueofgraphs
cargo run --locked --example opgg_probe -- --force "kr:Hide on bush#KR1" "kr:Gumayusi#KR1" "kr:Oner#KR1"
```

The Python tool saves raw responses, HTTP statuses/types/durations, challenge
detection and a summary under `target/provider-probe`; override `--output` to
retain a separate run. Neither tool reads the app account file or passwords.
Do not turn these into high-volume load tests. Ordinary unit tests are offline.

## Remaining risks and manual test on five accounts

Server actions are an internal web contract. A deployment can change the Flight
format, reference discovery, arguments or profile timestamps. Such failures
produce cached/stale data and an error, rather than fabricated Unranked. Limits
beyond this small sample, non-KR renewal behavior, TFT force renewal and freshness
relative to an independently verified just-completed match remain unverified.
LeagueOfGraphs fallback is deliberately absent because normal HTTP access was
blocked, and MCP supplies no verified refresh capability.

For a local test, use `target/release/LeagueAccounts.exe`; the original executable
beside the repository has not been replaced. **Disable Settings → Updates →
Update automatically while testing this branch**: the inherited updater still
points to Tariolle/LeagueAccounts, and a later upstream release could replace the
test build. Changing release distribution is separate from rank fetching.

1. Add/open five accounts with correct Riot IDs and regions, then Ctrl+R.
2. Verify rank, LP, level and previous season against each OP.GG page, and inspect
   the source timestamp/status line. An unranked account must stay Unranked.
3. After playing a ranked match, wait until OP.GG allows renewal (about two
   minutes observed), then Ctrl+Shift+R. Check LP and source time against the site.
4. Repeat immediately: fresh profiles must not cause unnecessary renewals, and
   force must respect OP.GG cooldown. Check that normal login still works.
5. Disconnect the network and refresh: last known ranks remain visible with an
   error/stale state. Reconnect and refresh to recover. Switch to TFT: current and
   previous-set ranks remain available, with freshness explicitly unknown.
6. Restart and confirm ranks/provenance persist. Let one scheduled refresh run.

This five-account interactive check has not been performed on the user's accounts.

## Validation

- `cargo fmt --all` executed; unrelated pre-existing formatting changes were
  removed from the diff. Changed parser/model/test modules are rustfmt formatted.
- `cargo check --locked --workspace --all-targets`: passed.
- `cargo test --locked --workspace --all-targets`: final run passed, 98 tests.
  A sequential run with `-- --test-threads=1` also passed all 98.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `npm ci`, `npm run build` (assets, TypeScript, Vite), `npm test`: passed.
- `npm run test:ui`: final run passed all 24 checks with `CHROME_PATH` set to
  the installed Brave executable. Chrome is not installed at the default path.
- `npx tauri build --no-bundle`: Windows release executable built successfully.
- Card/list screenshots inspected with synthetic freshness/error states;
  labels fit and the list did not overflow horizontally.
- Both opt-in network probes exercised the actual public services as above.

Earlier runs exposed one timing failure in the existing launch-update browser
test and one existing Windows credential-migration test failure. Isolated/serial
reruns and the final default-concurrency suites passed; their causes were not
established and those unrelated systems were not changed. An initial loopback
test flake was fixed by explicitly making accepted Windows sockets blocking.
No CAPTCHA/Cloudflare challenge was solved or bypassed. The original app and
the user's saved accounts were not used for the network experiments.
