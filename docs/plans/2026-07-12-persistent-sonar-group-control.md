# Plan: Persistent Sonar Group Control

**Goal:** Authorize each Sonar group once, then let any allowlisted `npub` create and switch that group's Goose sessions without additional pairing codes.

**Delivery constraint:** Keep the work private on `codex/sonar-remote-control`, push the branch, and do not open a pull request.

## Affected files

- `crates/goose/src/gateway/mod.rs` — extend paired state with the sessions authorized to a group while retaining the current active session and initial authorization timestamp.
- `crates/goose/src/gateway/pairing.rs` — migrate legacy pairings, persist opaque session IDs safely, and add atomic authorize/switch/list operations that preserve Sonar session exclusivity.
- `crates/goose/src/gateway/handler.rs` — intercept Sonar-only `/new`, `/sessions`, `/use`, and `/session` commands before prompt execution and reuse the existing gateway-session setup path.
- `crates/goose/src/gateway/sonar/mod.rs` — preserve allowlist defense in depth and canonicalize/deduplicate relay configuration.
- `integrations/sonar-bridge/src/main.rs` — use the five common Sonar relays by default and deduplicate validated overrides.
- `ui/desktop/src/components/settings/app/SonarRemoteControlSection.tsx` — present controller npubs as an equal-permission allowed-user list, use the five-relay editable preset, and explain one-time authorization and session commands.
- `ui/desktop/src/i18n/messages/*.json` — add/update the Desktop copy in every locale catalog.
- `documentation/docs/experimental/remote-access/sonar-gateway.md` and `integrations/sonar-bridge/README.md` — document the allowlist, relay preset, commands, privacy boundary, revocation, and smoke test.
- Focused Rust, bridge, and Desktop tests — cover parsing, authorization, persistence, migration, relay defaults, and UI behavior.

## Approach

The existing `PlatformUser` key already maps one Sonar MLS group to one `PairingState`, and the bridge plus gateway both enforce the configured `npub` list. Keep that authorization boundary and only intercept the four exact commands after a group is paired. Pairing state will retain one active session plus the initial session and any sessions created through `/new`; `/use` can select only from that group-owned set, preventing a remote group from discovering or taking over unrelated Desktop sessions.

Session switching will be an atomic `PairingStore` mutation under the existing write lock. It will preserve the original `paired_at`, reject sessions owned by another Sonar group, and persist before acknowledging. `/new [name]` will reuse the current provider/model/extensions initialization, validate the optional name, create a group-scoped Gateway session, authorize it, and make it active. `/sessions` will return a bounded list with the active item marked; `/session` will report the active session; unknown slash-prefixed text will continue to the Goose agent unchanged.

Fresh bridge and Desktop configurations will default to Hedwig, Damus, nos.lol, Primal, and KaleidoSwap. Existing saved relay lists remain unchanged until the user edits them.

## Edge cases

- Non-allowlisted senders must remain unable to consume pairing codes or invoke management commands.
- Pairing codes remain single-use bearer values accepted only from an allowlisted sender; first valid use wins.
- Session IDs are opaque and must preserve underscores across YAML/config round trips and restarts.
- Legacy paired records without an authorized-session list migrate by treating their active session as authorized.
- `/new` failures must not change the active session; persistence failures must not leave an acknowledged but unusable mapping.
- `/use` validates ownership and existence before mutation and does not cancel an already-running prompt; it affects subsequent messages.
- A deleted active session does not revoke the group. `/session` reports it missing, while `/new` or `/use` recovers.
- Session names reject control characters and are length bounded; session listings are bounded below the Sonar protocol limit and never reveal working directories.
- Revocation blocks subsequent Goose commands but does not remove members or the bridge from the MLS group; Sonar controls group confidentiality.
- Relay URLs are validated and deduplicated without silently replacing customized persisted lists.

## Test plan

- Unit-test exact command parsing, whitespace, malformed arguments, unknown commands, Unicode names, control characters, and underscore-containing session IDs.
- Unit-test legacy pairing migration, safe persistence round trips, original `paired_at` preservation, group-owned session history, revoke behavior, and cross-group exclusivity.
- Test `/new`, `/sessions`, `/use`, and `/session` behavior around missing sessions, busy sessions, and bounded output where practical.
- Test all five bridge defaults, relay validation/deduplication, custom overrides, and unauthorized sender filtering.
- Update Desktop tests for the allowed-user terminology and relay preset; validate every i18n catalog.
- Run `cargo fmt`, Goose gateway tests with `sonar-gateway`, bridge tests, clippy with warnings denied, Desktop tests/typecheck/lint, schema freshness checks, and `git diff --check`.
- Run a private Grok 4.5 review through Grok CLI, apply every valid finding, rerun verification, and repeat until the reviewer returns `ACK` / production `GO`.
- Build and smoke-test the production Desktop bundle if the implementation changes package inputs.

## Conventions

- Use `anyhow::Result` and actionable errors; avoid `unwrap` in production paths.
- Keep Telegram behavior unchanged and command handling Sonar-specific.
- Preserve both bridge-side and gateway-side allowlist checks.
- Use ACP SDK or local Desktop types; do not recreate or import generated OpenAPI Desktop APIs.
- Run `cargo fmt` and clippy; keep generated artifacts consistent if any ACP schema changes become necessary.
- Add user-facing copy to every locale catalog and keep placeholders identical.

## Open risks

- The installed Sonar client can process relay events while leaving a stale conversation view; this work does not modify Sonar client rendering.
- Updating an allowlist or relay set restarts the gateway and retains the existing fail-closed interrupted-command behavior.
- Existing malformed pairing configuration must fail visibly rather than silently overwriting authorization state.

**Estimated size:** L (>200 LOC)
