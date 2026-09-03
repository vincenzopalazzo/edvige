# Brainstorm: Open the current project in your IDE (fork-local)

- **Date:** 2026-08-24
- **Origin:** [aaif-goose/goose#11504](https://github.com/aaif-goose/goose/issues/11504) — closed upstream as *not planned*; shipping on fork
- **Status:** decided, ready to implement

## Problem statement

Goose Desktop runs beside the user's editor; reviewing what goose touched currently means manually locating and opening the project folder. We add a button in the chat input area showing the default IDE that opens the current project directory in one click. A dropdown on the button lists every IDE detected on the machine, any of which can be set as default; a Settings section offers the same choice. Undetected IDEs are disabled/hidden, and with nothing detected the button stays out of the way. The choice persists under `GOOSE_DEFAULT_IDE`, mirroring how `GOOSE_PROMPT_EDITOR` works. Beyond directories, opening accepts a single file with optional `line[:col]` position so goose can jump straight at a diff — deep integration explored Zed-first.

## Decisions (user-confirmed)

| Question | Decision |
| --- | --- |
| Scope | Full spec: button dropdown + Settings section + deep detection |
| Detection | All installed IDEs, including JetBrains Toolbox and non-PATH installs |
| Target | Current project directory; also `file` and `file:line[:col]` positions |
| Upstream | Write clean code so it can be re-proposed upstream later |
| Rebase mechanics | Long-lived fork branch, merge-based syncs from `aaif-goose/main` |

## Approach chosen: capability-adapter module

A self-contained new module keeps ~all of the feature out of high-churn files, so merges from upstream stay trivial:

```
ui/desktop/src/ide/
├── types.ts        # IdeId, DetectedIde, OpenTarget {dir, file?, line?, col?}
├── registry.ts     # known IDE catalog + dedupe/ranking of detections
├── detect/
│   ├── pathLookup.ts    # layer 1: PATH lookup
│   ├── appBundles.ts    # layer 2: /Applications, Program Files, Start Menu
│   ├── toolbox.ts       # layer 3: JetBrains Toolbox layouts
│   └── desktopFiles.ts  # layer 2b: Linux .desktop Exec parsing
└── adapters/
    ├── zed.ts           # `zed file:line:col`
    ├── vscodeFamily.ts  # code/cursor/windsurf/codium `-g file:line:col`
    ├── jetbrains.ts     # `--line N file`
    └── sublime.ts       # `subl file:line:col`
```

Rejected alternatives: monolith inside `main.ts` (conflict-prone, hard to test), Rust/goosed route (heavy for a UI-only feature; CLI explicitly out of scope).

## Detection design (layered, all layers run, results deduped)

Ground truth from local probe (macOS): `Zed.app`, `Cursor.app`, `Visual Studio Code.app`, `Xcode.app` in `/Applications`; `code` on PATH at `/usr/local/bin/code`; `cursor` at `~/.local/bin/cursor`; no Toolbox apps installed — so detection cannot assume Toolbox exists, and bundle-only installs (Zed) are the common case here.

1. **PATH lookup** — `code`, `code-insiders`, `codium`, `cursor`, `windsurf`, `zed`, `subl`, JetBrains products.
2. **Standard install locations**
   - macOS: `/Applications`, `~/Applications` (`*.app` bundles)
   - Windows: `%LocalAppData%\Programs\*`, `Program Files`, Start Menu `.lnk` resolution
   - Linux: `/usr/share/applications`, `~/.local/share/applications` `*.desktop` files (`Exec=` parsing with `%f`/`%u` field-code stripping)
3. **JetBrains Toolbox**
   - macOS: `~/Library/Application Support/JetBrains/Toolbox/apps/*/**/*.app` (read `product-info.json`)
   - Linux: `~/.local/share/JetBrains/Toolbox/apps/*`
   - Windows: `%LocalAppData%\JetBrains\Toolbox\apps\*`
   - Legacy channel layouts + generated wrapper scripts (e.g. `~/bin/<product>`)
4. **Bundled CLIs inside app bundles** — prefer launching the embedded CLI (`Cursor.app/Contents/Resources/app/bin/cursor`, VS Code `bin/code`, Zed `cli`) over `open -a`, because CLIs take uniform positional args cross-platform.

An IDE is offered in the UI only if a usable launcher (CLI binary or bundle) resolves.

## Launch / deep-link semantics

| Family | Project dir | File at position |
| --- | --- | --- |
| Zed | `zed <dir>` | `zed <file>:<line>:<col>` (native) |
| VS Code family | `<cli> <dir>` | `<cli> -g <file>:<line>:<col>` |
| JetBrains | `<product> <dir>` | `<product> --line <n> <file>` (no column support) |
| Sublime | `subl <dir>` | `subl <file>:<line>:<col>` |
| Fallback | macOS `open -a <App> <path>` / Windows `start` / Linux `gio open` — directory only, no position |

## Integration points (minimal-diff plan)

| File | Conflict risk | Edit |
| --- | --- | --- |
| `ui/desktop/src/ide/**` | none | new module + unit tests |
| `ui/desktop/src/main.ts` | high churn, but append-friendly | two `ipcMain.handle`s (`ide-list-detected`, `ide-open`) next to `list-recent-dirs` |
| `ui/desktop/src/preload.ts` | medium | expose the two methods on `window.electron` |
| `settings/IdeSettingsSection.tsx` | none | new section |
| `settings/SettingsView.tsx` | low | register the section |
| `ChatInput.tsx` | **highest churn in repo** | import + one render line for `<IdeButton />`, nothing else |

Default selection reads/writes through the existing `get-setting`/`set-setting` IPC under `GOOSE_DEFAULT_IDE`.

## Non-goals

- Reading open tabs/files from the IDE back into goose context
- Detecting running instances or specific windows; window-reuse policy
- CLI (`goose-cli`) support or a Rust/goosed route
- IDE extensions/plugins

## Verification plan

1. `cd ui/desktop && pnpm run typecheck && pnpm test`
2. Unit tests per detector using tmp-dir fixtures mimicking Toolbox layouts (incl. `product-info.json`) and `.desktop` files
3. Manual matrix on this machine: Zed (bundle-only), Cursor (PATH), VS Code (both); verify hidden-button case when nothing resolves (e.g. scrubbed PATH)
4. Positional smoke test: open `file:line:col` against Zed and Cursor

## Ship order

1. `src/ide/` scaffold + detector unit tests
2. IPC handlers + preload bridge
3. `IdeSettingsSection` + `SettingsView` registration
4. `IdeButton` minimal edit in `ChatInput.tsx`
5. typecheck/lint/tests + manual QA pass
