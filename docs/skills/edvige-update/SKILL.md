---
name: edvige-update
description: Update the vincenzopalazzo/edvige fork of goose — rebase edvige-beta onto aaif-goose/main, resolve the known fork conflicts, build a release on this host, and install the CLI and desktop app. Use when the user asks to update/rebase/sync the goose fork, refresh edvige-beta with upstream, or build and install a new goose release locally.
---

# edvige fork update playbook

Fork: `vincenzopalazzo/edvige`, branch `edvige-beta` (default). Upstream: `aaif-goose/goose`, branch `main`.
Working tree: `~/.codex/worktrees/ba7e/goose`. Rebase-only — never merge upstream.

## 0. Environment (always)

```bash
cd ~/.codex/worktrees/ba7e/goose
source bin/activate-hermit   # hermit cargo/rustc. The system toolchain has a broken cc crate.
```

Remotes: `aaif-goose` (upstream), `fork` (vincenzopalazzo/edvige), `origin` (block/goose, ignore).

## 1. Fetch and fast-forward to the fork first

```bash
git fetch aaif-goose main && git fetch fork edvige-beta
git merge --ff-only fork/edvige-beta     # pick up fork-only commits made elsewhere
git rebase aaif-goose/main               # never merge
```

## 2. Known conflicts and the resolution that keeps both sides

| File | Resolution |
|---|---|
| `ui/desktop/src/utils/githubUpdater.ts` | Keep upstream's atomic staging (`fs.mkdtemp` + `fs.realpath(os.tmpdir())`) AND the fork's `ensureCacheDir()` download path. Upstream refactors this file often. |
| `ui/desktop/src/main.ts` + `utils/settings.ts` | `validSettingKeys` and the `Settings` interface are a UNION: keep fork keys `useLegacyAgentLoop`, `customUpdateOwner`, `customUpdateRepo`, `customUpdateBundleName` plus upstream's new keys (e.g. `GOOSE_DEFAULT_IDE`, theme keys). |
| `ui/desktop/src/i18n/messages/*.json` | Keep both sides' keys. When a conflict ends at an object entry, the closing `},` needs a comma before the next block. Validate every file with `json.loads` before `git add`. |
| `crates/goose/src/agents/extension_manager/` | Upstream split the old `extension_manager.rs` into `mod.rs` / `stdio.rs` / `streamable_http.rs`. NEVER resolve by restoring the monolith. Port the fork's OAuth logic into `streamable_http.rs`: `presented_access_token()` helper, `presented_access_token: RwLock<Option<String>>` field on `OAuthStepUpClient`, third `new()` arg, `rejected_access_token` passed to `oauth_flow_with_challenge`, refresh the field after `step_up_reconnect`. |
| `ui/goose-acp-client/src/generated/*` | Generated client moved from `ui/sdk`. For `export type { ... }` lists: keep upstream (HEAD) list + append fork-only names (`SessionActivity*`, `SetSessionStatusRequest_unstable`). For the big `ExtRequest`/`ExtResponse` union lines: take HEAD and splice in the fork types (`SessionActivityRequest_unstable` etc.). Keep `.js` suffix on `from './types.gen.js'`. In `zod.gen.ts` keep HEAD and append the `zSessionActivity*` blocks. |
| `ui/desktop/src/App.tsx` / `Hub.tsx` | Keep the fork's Hub-Enter-navigates-immediately flow (`handleSubmit` → `setView('pair', ...)` without awaiting `createSession`; Live Voice still creates a session first via `createHubSession`). Wrap in upstream's newer structure (Live Voice controller prop, recipe consent, `formatAcpError`). `PairRouteWrapper` keeps upstream's `unmountedRef` reset + fork's `isRecipeDeclined` handling. |
| `ui/desktop/src/utils/timeUtils.ts` | Keep the fork's `formatClockDisplay(date, locale)` Intl version (the old one mishandled 24h locales). |
| `crates/goose/src/agents/agent.rs` import list | Union of both sides (fork adds `has_unapplied_tool_confirmation_response`, `pending_tool_confirmations`, `persist_tool_confirmation_decision`; upstream may add new operations like `OutputLimitRecoveryOperation`). |
| `crates/goose/src/agents/state_machine/ops_output_limit.rs` | Old `GooseEffect::ReplaceConversation { conversation, usage }` is gone. Use `GooseEffect::CompactConversation { conversation, usage }` (records usage + replaces history + emits `HistoryReplaced`). |
| `crates/goose-provider-types/.../message.rs` | Fork's `is_visible_output()` must cover upstream's new enum variants. `Document` counts as visible output (`=> true`). |

After the rebase: `cargo fmt --all` (NOT `cargo fmt -- <file>` — the bare-file form skips the
edition config and emits 2024-style import/macro formatting that pollutes the commit), then
`cargo fmt --all --check` must be clean, then compile-check.

## 3. Compile checks

```bash
cargo build --release -p goose-cli --bin goose     # full CLI build
cargo clippy -p goose -p goose-cli --lib --bins    # NOTE: clippy -p goose ALONE fails on
                                                   # process-wrap ProcessSession (feature unifies
                                                   # only with goose-cli in the set)
cd ui/desktop && pnpm typecheck                    # desktop
```

## 4. Build the release

```bash
# CLI (also copies the binary into ui/desktop/src/bin for packaging)
just release-binary

# Desktop — bake the fork update feed
pnpm install -C ui/desktop          # first time in a fresh worktree (postinstall regenerates
pnpm install -C ui/goose-acp-client # the ACP client from acp-schema.json)
GITHUB_OWNER=vincenzopalazzo GITHUB_REPO=edvige GOOSE_BUNDLE_NAME=Goose just make-ui
```

Artifacts: `ui/desktop/out/Goose-darwin-arm64/Goose.zip` (forge collapses the unpacked .app into the zip) and `ui/desktop/out/make/zip/darwin/arm64/`.

## 5. Install (never overwrite a live binary)

```bash
# Quit the running app first (it holds the bundle and the backend)
osascript -e 'quit app "Goose"'; sleep 3
pkill -f "Goose.app/Contents" 2>/dev/null; pkill -f "bin/goose serve" 2>/dev/null

# CLI — atomic rename, no cp onto the live file
mv target/release/goose ~/.local/bin/goose.new && mv ~/.local/bin/goose ~/.local/bin/goose.prev && mv ~/.local/bin/goose.new ~/.local/bin/goose

# GUI — unzip to staging, swap the bundle by rename
unzip -q -o ui/desktop/out/Goose-darwin-arm64/Goose.zip -d /tmp/goose-install
mv ~/Applications/Goose.app ~/Applications/Goose.app.bak-$(date +%Y%m%d-%H%M%S)
mv /tmp/goose-install/Goose.app ~/Applications/Goose.app

# Start
open -a ~/Applications/Goose.app
```

`/Applications/Goose.app` is root-owned and stale — leave it alone (needs sudo).

## 6. Verify

```bash
goose --version                                              # expect the new version
/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' \
  ~/Applications/Goose.app/Contents/Info.plist
ps aux | grep "bin/goose serve" | grep -v grep               # backend running from the new bundle
curl -sk https://127.0.0.1:<port>/health                     # TLS! plain http returns HTTP/0.9 error
```

Logs live at `~/Library/Application Support/Goose/logs/main.log` — NOT `~/Library/Logs/goose/` (stale since 2025). Check the startup lines for `GitHubUpdater: API URL: https://api.github.com/repos/vincenzopalazzo/edvige/...` to confirm the fork feed is baked, and `Current version` vs `Latest version` (an older fork release like `1.48.1-edvige` must NOT trigger an update).

If no window appears but processes are alive: the renderer logged `React ready event received` and the backend answers `/health` — check the saved window bounds in `~/Library/Application Support/Goose/window-state.json` (off-screen restore from a disconnected display) or just relaunch via `open -a`.

## 7. Push

```bash
git push fork edvige-beta                # normal
git push fork edvige-beta --force-with-lease  # after a rebase
```
