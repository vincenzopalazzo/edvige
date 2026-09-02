# AGENTS Instructions

goose is an AI agent framework in Rust with CLI and Electron desktop interfaces.

## Contribution Workflow

The issue is the source of truth for work intended for an upstream pull request. Track issue status on the [Goose Issues board](https://github.com/orgs/aaif-goose/projects/1).

- Before implementing an issue for a pull request, confirm that it is on the board with Status **Ready**.
- Do not implement issues in **Inbox**, **Needs info**, or **Accepted / design**. Help resolve the issue discussion instead.
- Read the agreed design, constraints, non-goals, and verification plan before changing code.
- Keep the implementation within the issue's agreed scope.
- If implementation reveals a material design change, return to the issue before continuing.
- Every external pull request must link the Ready issue it implements and explain how the verification plan was performed.
- Structure new issues on the matching template in `.github/ISSUE_TEMPLATE/` and set the issue type (e.g. Bug, Feature). `gh issue create` does not apply templates automatically.

Maintainer-directed work, urgent security fixes, release automation, and local or exploratory changes do not require a Ready issue.

## GitHub Communication

Write issue and pull request comments for humans, not as exhaustive work logs.

- Lead with the conclusion or action needed.
- Keep comments concise; do not repeat context already present in the thread.
- Use short paragraphs or bullets, and include implementation details only when they affect a decision or review.
- Prefer one clear summary over multiple incremental comments.

## Agent Loop Migration

We are replacing the legacy agent loop in `crates/goose/src/agents/agent.rs` with the state machine in `crates/goose/src/agents/state_machine/`. The state-machine path is enabled with `GOOSE_STATE_MACHINE=1`.

Until the migration is complete, changes to agent-loop behavior must be implemented and tested in both paths. When reviewing code, check whether a change to either path also applies to the other and flag missing parity.

## Setup
```bash
source bin/activate-hermit
cargo build
```

## Commands

### Build
```bash
cargo build                   # debug
cargo build --release         # release  
just release-binary           # release binary
```

### Test
```bash
cargo test                   # all tests
cargo test -p goose          # specific crate
cargo test --package goose --test mcp_integration_test
just record-mcp-tests        # record MCP
```

### Lint/Format
```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
```

### UI
```bash
just run-ui                  # start desktop
cd ui/desktop && pnpm run typecheck
cd ui/desktop && pnpm test   # test UI
```

## Structure
```
crates/       # Rust workspace members — see root Cargo.toml (`members = ["crates/*"]`)
ui/desktop/   # Electron app
ui/text/      # deprecated ACP TUI (see ui/text/README.md)
```

## Development Loop
```bash
# 1. source bin/activate-hermit
# 2. Make changes
# 3. cargo fmt
```

### Run these only if the user has asked you to build/test your changes:
```
# 1. cargo build
# 2. cargo test -p <crate>
# 3. cargo clippy --all-targets -- -D warnings
```

## Rules

- Test: Prefer tests/ folder, e.g. crates/goose/tests/
- Test: When adding features, update goose-self-test.yaml, rebuild, then run `goose run --recipe goose-self-test.yaml` to validate
- Error: Use anyhow::Result
- Provider: Implement Provider trait see providers/base.rs
- MCP: Extensions in crates/goose-mcp/
- UI Desktop: Use ACP SDK types or local `src/types/*` types. Do not import generated OpenAPI types/client code from `ui/desktop/src/api`

## Code Quality

- Comments: Write self-documenting code - prefer clear names over comments
- Comments: Never add comments that restate what code does
- Comments: Only comment for complex algorithms, non-obvious business logic, or "why" not "what"
- Simplicity: Don't make things optional that don't need to be - the compiler will enforce
- Simplicity: Booleans should default to false, not be optional
- Errors: Don't add error context that doesn't add useful information (e.g., `.context("Failed to X")` when error already says it failed)
- Simplicity: Avoid overly defensive code - trust Rust's type system
- Logging: Clean up existing logs, don't add more unless for errors or security events

## Never

- Never: Recreate `ui/desktop/src/api` or add `@hey-api/openapi-ts` to `ui/desktop`
- Cargo.toml: For human-authored dependency changes, use `cargo add` instead of manually editing dependency entries unless there is a specific reason not to.
- Cargo.toml: Automated dependency bump PRs are exempt; when manual edits are necessary, keep `Cargo.lock` consistent.
- Never: Skip cargo fmt
- Never: Merge without running clippy
- Never: Comment self-evident operations (`// Initialize`, `// Return result`), getters/setters, constructors, or standard Rust idioms
- Never: Overwrite a live binary in place (e.g. `cp`/`fs.copyFileSync` onto an existing executable) - unlink or atomic-rename the destination first, otherwise macOS SIGKILLs running processes with "Code Signature Invalid"

## Entry Points
- CLI: crates/goose-cli/src/main.rs
- UI: ui/desktop/src/main.ts
- Agent: crates/goose/src/agents/agent.rs

## Fork Maintenance (vincenzopalazzo/goose — rebase-only desktop fork)

This fork stays **rebase-only** on top of `aaif-goose/goose` to keep core features in sync while shipping desktop-only extras that upstream may not take.

**Goal:** Same Goose core (`crates/*`), same CLI/server, but a desktop app with additional features (e.g. in-place auto-update, fork-aware feed). Never diverge core unless required; keep changes isolated so `git rebase` stays trivial.

**Setup**
```bash
git remote add upstream git@github.com:aaif-goose/goose.git  # or aaif-goose
git remote add fork git@github.com:vincenzopalazzo/goose.git
git fetch upstream
git fetch fork
# Primary worktrees track fork branches, but rebase onto upstream/main
```

**Weekly rebase (keeps fork fresh)**
```bash
git fetch upstream main
git checkout feat/your-feature   # or main of fork if you keep a main
git rebase upstream/main
# resolve conflicts (prefer upstream for crates/*, keep your ui/desktop deltas)
git push -f fork feat/your-feature
```
CI has `.github/workflows/weekly-rebase.yml` to open/update a rebase PR automatically. Never `git merge upstream/main` — that creates a merge commit that makes future rebases painful; always `rebase`.

**Where to put fork features**
- Prefer `ui/desktop` only: `ui/desktop/src/utils/githubUpdater.ts`, `ui/desktop/src/utils/autoUpdater.ts`, `ui/desktop/src/main.ts`, `ui/desktop/src/utils/settings.ts`.
- Avoid touching `crates/goose`, `crates/goose-cli`, `crates/goose-server` unless the feature truly needs backend support. If you must, keep it behind a flag/build tag so upstream can ignore it.
- Keep desktop extras small and isolated (e.g. `getUpdateCacheDir`/`pruneCache`/`isTargetWritable` are self-contained helpers). This is why the in-place updater PR is ~120 lines in 2 files.

**Fork-aware auto-update (no rebuild needed after first fork build)**
- Build-time (first fork build): `GITHUB_OWNER=vincenzopalazzo GITHUB_REPO=goose GOOSE_BUNDLE_NAME=Goose pnpm --prefix ui/desktop make` — bakes feed to your fork.
- Runtime (after that, no rebuild): Settings → `customUpdateOwner`/`customUpdateRepo`/`customUpdateBundleName` (stored in `Settings` via `get-setting`/`set-setting`) are applied at startup in `appMain()` and on `set-setting` via `setCustomUpdateRepository()` + `githubUpdater.setCustomRepository()`. Users or you can switch feed without reinstall:
  ```ts
  await window.electron.setSetting('customUpdateOwner', 'vincenzopalazzo')
  await window.electron.setSetting('customUpdateRepo', 'goose')
  ```
  Or set via env before first fork build and let runtime override handle drift.

**Publishing fork releases**
- Assets must match `githubUpdater` expectations: `Goose.zip` (mac arm64), `Goose_intel_mac.zip` (mac x64), `Goose-win32-x64.zip`.
- Keep your own code-signing certs (macOS notarization/Windows). `ditto` preserves xattrs, but a new signature is still required.

**Do / Don't**
- Do: rebase often (weekly), keep desktop changes behind minimal flags, push feature branches to `fork`, open PRs against `vincenzopalazzo/goose` for fork-only features and against `aaif-goose/goose` when you want upstream to take it.
- Don't: merge upstream with a merge commit, edit `ui/desktop/src/api` (generated), or change `Cargo.lock` manually; don't add core divergences that would make `cargo fmt`/`clippy` noisy on rebase.

