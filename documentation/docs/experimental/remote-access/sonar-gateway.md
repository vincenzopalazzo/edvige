---
title: Sonar Gateway
sidebar_position: 3
sidebar_label: Sonar Gateway
description: Control a goose session from a shared, end-to-end encrypted Sonar group.
---

The Sonar Gateway binds one Sonar/Marmot MLS group to one goose session. You can use the Sonar app today and build a compatible goose mobile fork on the same Sonar Core transport later.

:::warning Experimental feature
The gateway and bridge protocol are experimental. Goose Desktop coordinates local and Sonar prompts through one session manager, but separately started Goose CLI processes do not coordinate with the Desktop app.
:::

## Security model

Sonar provides authenticated, end-to-end encrypted group transport over Nostr relays. goose adds two local authorization checks:

- The group must be paired to a goose session with a short-lived code.
- The message sender's authenticated npub must be in the gateway's allowed-user list.
- The bridge accepts new group invitations only when an allowed user sent the welcome.

Joining the group does not grant permission to run goose. Observers can read group traffic but the bridge checkpoints and ignores their commands before they reach the execution queue. Sonar currently allows every MLS group member to administer membership, so share a control group only with people who may read its future messages.

The allowed-user list is local configuration. Every listed npub has equal control. Group messages cannot add users or change goose's permission policy.

## Goose Desktop setup

Production Goose Desktop bundles the feature-enabled Goose backend and `goose-sonar-bridge`. Open **Settings → External Backend → Sonar remote control** and:

1. Enter each allowed-user npub shown by the Sonar app.
2. Keep the editable Sonar relay preset (Hedwig, Damus, nos.lol, Primal, and KaleidoSwap), or enter the relay set used by your group.
3. Start remote control and copy the Goose bridge npub.
4. From an allowed Sonar identity, invite the bridge npub to a group.
5. In Goose, choose an existing session or the dedicated remote-session option and generate the one-time pairing code.
6. Send the pairing code in the Sonar group from an allowed user.

Fresh configurations use all five relay defaults. Goose leaves any relay text already saved in Desktop unchanged, including an older single-relay configuration, so review it when upgrading.

The group remains authorized after that first code. Any allowed user in the group can manage its active Goose session with:

- `/new [name]` — create and activate a new group-owned session.
- `/sessions` — list sessions authorized for this group.
- `/use SESSION_ID` — activate one of those sessions.
- `/session` — show the active session.

The initial paired session and sessions created with `/new` are the only sessions exposed to that group. Unknown slash commands remain normal Goose prompts.

Keep that Goose Desktop window open while using the remote session. The gateway configuration, bridge identity, authorization, active session, and group-owned session list persist across app launches. **Stop** keeps the saved configuration and authorizations; **Forget and revoke all** removes the saved gateway configuration and all Goose group authorizations. Revoke a single group from its entry in the Paired groups list. Revocation blocks subsequent commands but does not cancel an already-running prompt.

## Manual CLI build

Sonar Core uses a SQLite dependency that cannot currently be linked into the goose binary. Build the isolated bridge and the feature-enabled CLI separately:

```bash
cargo build --release --manifest-path integrations/sonar-bridge/Cargo.toml
cargo build --release -p goose-cli --features sonar-gateway
```

Install `goose-sonar-bridge` on `PATH`, place it beside the `goose` executable, or pass its path when starting the gateway.

## Set up a group

1. Get each allowed-user npub from the Sonar app identities that may send commands.
2. Initialize the bridge state and print the bridge npub with `goose-sonar-bridge --home /path/to/state identity`.
3. From an allowed identity, add the bridge npub to a Sonar group. The bridge accepts the invite and announces that the group is ready to pair.
4. Generate a pairing code for the goose session.
5. Send that code to the Sonar group from an allowed user. No additional code is required for `/new` or `/use`.

```bash
goose gateway start sonar \
  --bridge-path /absolute/path/to/goose-sonar-bridge \
  --sonar-home /path/to/state \
  --controller npub1yourcontroller \
  --relay wss://nostr.relay.hedwig.sh/

goose gateway pair sonar --session-id SESSION_ID
```

`--controller` and `--relay` are repeatable. Every controller has equal authority, and at least one is required. If no relays are supplied, the bridge uses Hedwig, Damus, nos.lol, Primal, and KaleidoSwap.

Omit `--session-id` to create a dedicated gateway session during pairing. With `--session-id`, the existing session keeps its model, provider, extensions, working directory, and mode. A session can be paired to only one Sonar group at a time.

## Sharing and revoking access

Invite another person to the Sonar group to share the transcript. They remain an observer until their npub is added to the allowed-user list (the repeatable `--controller` argument in the CLI) and the gateway is restarted. Remove their npub and restart to revoke command authority; remove them from the Sonar group as well to revoke access to future group traffic. Revoking Goose authorization or forgetting the gateway does not remove the bridge or other members from the MLS group.

## State, backup, and replay behavior

The bridge stores its Nostr secret, database key, encrypted MLS database, and command ledger under the goose data directory's `sonar-gateway` folder by default. Use `--sonar-home` to choose another location.

Back up the entire state directory while the gateway is stopped. Treat `config.json` as a secret. Relays alone may not be sufficient to recover the MLS session if the bridge identity or database key is lost.

Commands are persisted before execution. A command interrupted by a restart is never replayed automatically, because its tool side effects may already have occurred. The bridge marks it complete and asks the controller to verify its effects before deciding whether to resubmit. Existing group history is checkpointed at first discovery and is not executed as new input.

## Limitations

- Transport uses Nostr relays and Marmot MLS, not BLE mesh.
- Commands and responses are plain group text in this first protocol version.
- Rich ACP events, steering, and cancellation are reserved for a later typed mobile control envelope.
- The bridge must remain running, and the host must stay awake and connected to configured relays.
- The completed-command ledger grows with received history and should be retained with the MLS state.

## Run the smoke test

After configuring a goose provider, run the repository's interactive release smoke test:

```bash
./integrations/sonar-bridge/smoke-test.sh \
  --controller npub1yourcontroller \
  --relay wss://nostr.relay.hedwig.sh/
```

The script builds the release binaries and prints the bridge npub. Invite that identity from the configured controller, press Enter, then follow the pairing, ordering, and unauthorized-member checks printed in the terminal. Use `--session-id SESSION_ID` to test an existing session. The gateway uses `--no-persist`, so the smoke run is not saved for automatic restart.
