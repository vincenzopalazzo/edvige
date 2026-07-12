use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::agents::{AgentEvent, ExtensionConfig, SessionConfig};
use crate::config::extensions::get_enabled_extensions;
use crate::config::paths::Paths;
use crate::config::Config;
use crate::conversation::message::{Message, MessageContent};
use crate::execution::manager::AgentManager;
use crate::session::SessionType;
use crate::session::{EnabledExtensionsState, ExtensionState, Session};

use super::pairing::{PairingStore, PendingPairing};
use super::{Gateway, GatewayConfig, IncomingMessage, OutgoingMessage, PairingState, PlatformUser};

/// Conservative default cap on tool-calling loops for gateway sessions.
///
/// Chat platforms like Telegram favor short, snappy replies, so the gateway
/// keeps a stricter default than the global `GOOSE_MAX_TURNS` ceiling.  Users
/// can override this through `GOOSE_GATEWAY_MAX_TURNS` (gateway-specific) or
/// `GOOSE_MAX_TURNS` (applies globally).
const DEFAULT_GATEWAY_MAX_TURNS: u32 = 5;
const MAX_REMOTE_SESSION_NAME_CHARS: usize = 80;
const MAX_REMOTE_SESSION_LIST_ENTRIES: usize = 20;

#[derive(Debug, PartialEq, Eq)]
enum SonarSessionCommand {
    New(Option<String>),
    Sessions,
    Use(String),
    Session,
}

fn parse_sonar_session_command(text: &str) -> Option<Result<SonarSessionCommand, String>> {
    let text = text.trim();
    let mut parts = text.splitn(2, char::is_whitespace);
    let command = parts.next()?;
    let argument = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match command {
        "/new" => Some(Ok(SonarSessionCommand::New(argument.map(str::to_string)))),
        "/sessions" if argument.is_none() => Some(Ok(SonarSessionCommand::Sessions)),
        "/sessions" => Some(Err("Usage: /sessions".into())),
        "/use" => Some(match argument {
            Some(session_id) if !session_id.chars().any(char::is_whitespace) => {
                Ok(SonarSessionCommand::Use(session_id.to_string()))
            }
            _ => Err("Usage: /use <session-id>".into()),
        }),
        "/session" if argument.is_none() => Some(Ok(SonarSessionCommand::Session)),
        "/session" => Some(Err("Usage: /session".into())),
        _ => None,
    }
}

fn validate_remote_session_name(name: Option<String>) -> anyhow::Result<Option<String>> {
    let Some(name) = name else {
        return Ok(None);
    };
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    if name.chars().any(char::is_control) {
        anyhow::bail!("session name cannot contain control characters");
    }
    if name.chars().count() > MAX_REMOTE_SESSION_NAME_CHARS {
        anyhow::bail!("session name cannot exceed {MAX_REMOTE_SESSION_NAME_CHARS} characters");
    }
    Ok(Some(name.to_string()))
}

fn display_session_name(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(MAX_REMOTE_SESSION_NAME_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Resolve the max turns to use for a gateway session.
///
/// Precedence: `GOOSE_GATEWAY_MAX_TURNS` -> `GOOSE_MAX_TURNS` ->
/// `DEFAULT_GATEWAY_MAX_TURNS`.  Extracted as a pure function so the
/// precedence rules can be unit-tested without touching the global config.
fn resolve_gateway_max_turns(gateway_override: Option<u32>, global_max_turns: Option<u32>) -> u32 {
    gateway_override
        .or(global_max_turns)
        .unwrap_or(DEFAULT_GATEWAY_MAX_TURNS)
}

#[derive(Clone)]
pub struct GatewayHandler {
    agent_manager: Arc<AgentManager>,
    pairing_store: Arc<PairingStore>,
    gateway: Arc<dyn Gateway>,
    config: GatewayConfig,
}

struct SessionBusyGuard {
    agent_manager: Arc<AgentManager>,
    session_id: String,
    cancel: CancellationToken,
    armed: bool,
}

impl SessionBusyGuard {
    fn new(agent_manager: Arc<AgentManager>, session_id: &str, cancel: CancellationToken) -> Self {
        Self {
            agent_manager,
            session_id: session_id.to_string(),
            cancel,
            armed: true,
        }
    }

    async fn release(mut self) {
        self.agent_manager
            .unregister_cancel_token(&self.session_id)
            .await;
        self.armed = false;
    }
}

impl Drop for SessionBusyGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.cancel.cancel();
        let agent_manager = self.agent_manager.clone();
        let session_id = self.session_id.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                agent_manager.unregister_cancel_token(&session_id).await;
            });
        }
    }
}

impl GatewayHandler {
    pub fn new(
        agent_manager: Arc<AgentManager>,
        pairing_store: Arc<PairingStore>,
        gateway: Arc<dyn Gateway>,
        config: GatewayConfig,
    ) -> Self {
        Self {
            agent_manager,
            pairing_store,
            gateway,
            config,
        }
    }

    pub async fn handle_message(&self, message: IncomingMessage) -> anyhow::Result<()> {
        let pairing = self.pairing_store.get(&message.user).await?;

        match pairing {
            PairingState::Unpaired => {
                if let Some(pending) = self.try_consume_code(message.text.trim()).await? {
                    if pending.gateway_type == self.config.gateway_type {
                        self.complete_pairing(&message.user, pending.session_id.as_deref())
                            .await?;
                    } else {
                        self.gateway
                            .send_message(
                                &message.user,
                                OutgoingMessage::Text {
                                    body: "⚠️ That code is for a different gateway.".into(),
                                },
                            )
                            .await?;
                    }
                } else {
                    self.gateway
                        .send_message(
                            &message.user,
                            OutgoingMessage::Text {
                                body: "Welcome! Enter your pairing code to connect to goose."
                                    .into(),
                            },
                        )
                        .await?;
                }
            }
            PairingState::PendingCode { code, expires_at } => {
                let now = chrono::Utc::now().timestamp();
                if now > expires_at {
                    self.pairing_store
                        .set(&message.user, PairingState::Unpaired)
                        .await?;
                    self.gateway
                        .send_message(
                            &message.user,
                            OutgoingMessage::Text {
                                body: "Your pairing code expired. Please request a new one.".into(),
                            },
                        )
                        .await?;
                } else if message.text.trim().eq_ignore_ascii_case(&code) {
                    self.complete_pairing(&message.user, None).await?;
                } else {
                    self.gateway
                        .send_message(
                            &message.user,
                            OutgoingMessage::Text {
                                body: "Invalid code. Please try again.".into(),
                            },
                        )
                        .await?;
                }
            }
            PairingState::Paired { session_id, .. } => {
                if self.config.gateway_type == "sonar" {
                    if let Some(command) = parse_sonar_session_command(&message.text) {
                        match command {
                            Ok(command) => {
                                self.handle_sonar_session_command(&message, &session_id, command)
                                    .await?;
                            }
                            Err(usage) => self.send_text(&message.user, usage).await?,
                        }
                        return Ok(());
                    }
                    if let Err(reason) = self.live_session(&session_id).await {
                        self.send_text(
                            &message.user,
                            format!(
                                "Active session '{session_id}' is unavailable ({reason}). Use /use or /new to recover."
                            ),
                        )
                        .await?;
                        return Ok(());
                    }
                }
                self.relay_to_session(&message, &session_id).await?;
            }
        }

        Ok(())
    }

    async fn try_consume_code(&self, text: &str) -> anyhow::Result<Option<PendingPairing>> {
        let normalized = text.to_uppercase().replace(['-', ' '], "");
        if normalized.len() == 6
            && normalized
                .chars()
                .all(|c| "ABCDEFGHJKLMNPQRSTUVWXYZ23456789".contains(c))
        {
            return self.pairing_store.consume_pending_code(&normalized).await;
        }
        Ok(None)
    }

    async fn handle_sonar_session_command(
        &self,
        message: &IncomingMessage,
        active_session_id: &str,
        command: SonarSessionCommand,
    ) -> anyhow::Result<()> {
        match command {
            SonarSessionCommand::New(name) => {
                let name = match validate_remote_session_name(name) {
                    Ok(name) => name,
                    Err(error) => {
                        self.send_text(&message.user, format!("Cannot create session: {error}"))
                            .await?;
                        return Ok(());
                    }
                };
                let session = self
                    .create_gateway_session(&message.user, name.as_deref())
                    .await?;
                if let Err(error) = self
                    .pairing_store
                    .authorize_and_activate_session(&message.user, &session.id)
                    .await
                {
                    if let Err(cleanup_error) = self
                        .agent_manager
                        .session_manager()
                        .delete_session(&session.id)
                        .await
                    {
                        tracing::error!(
                            session_id = %session.id,
                            %cleanup_error,
                            "failed to delete a Sonar session after authorization failed"
                        );
                    }
                    tracing::error!(session_id = %session.id, %error, "failed to authorize a new Sonar session");
                    self.send_text(
                        &message.user,
                        "Could not activate the new Goose session. Your previous session is still active; try /new again.".into(),
                    )
                    .await?;
                    return Ok(());
                }
                self.send_text(
                    &message.user,
                    format!(
                        "Created and activated Goose session '{}' ({}).",
                        display_session_name(&session.name),
                        session.id
                    ),
                )
                .await?;
            }
            SonarSessionCommand::Sessions => {
                let mut session_ids = self
                    .pairing_store
                    .authorized_sessions(&message.user)
                    .await?;
                if let Some(position) = session_ids
                    .iter()
                    .position(|session_id| session_id == active_session_id)
                {
                    session_ids.swap(0, position);
                }
                let total = session_ids.len();
                let mut lines = Vec::new();
                for session_id in session_ids {
                    if lines.len() == MAX_REMOTE_SESSION_LIST_ENTRIES {
                        break;
                    }
                    match self
                        .agent_manager
                        .session_manager()
                        .get_session(&session_id, false)
                        .await
                    {
                        Ok(session) if session.archived_at.is_none() => {
                            let marker = if session.id == active_session_id {
                                "*"
                            } else {
                                "-"
                            };
                            lines.push(format!(
                                "{marker} {} — {}",
                                display_session_name(&session.name),
                                session.id
                            ));
                        }
                        Ok(session) if session.id == active_session_id => {
                            lines.push(format!("* [archived] — {}", session.id));
                        }
                        Ok(_) => {}
                        Err(_) if session_id == active_session_id => {
                            lines.push(format!("* [missing] — {session_id}"));
                        }
                        Err(_) => {}
                    }
                }
                let mut body = if lines.is_empty() {
                    "No available Goose sessions. Use /new to create one.".to_string()
                } else {
                    format!(
                        "Authorized Goose sessions (* active):\n{}\nUse /use <session-id> to switch.",
                        lines.join("\n")
                    )
                };
                if total > lines.len() {
                    body.push_str(&format!(
                        "\nShowing {} available or active entries from {total} authorized sessions.",
                        lines.len()
                    ));
                }
                self.send_text(&message.user, body).await?;
            }
            SonarSessionCommand::Use(session_id) => {
                let authorized = self
                    .pairing_store
                    .authorized_sessions(&message.user)
                    .await?;
                if !authorized
                    .iter()
                    .any(|authorized| authorized == &session_id)
                {
                    self.send_text(
                        &message.user,
                        format!(
                            "Session '{session_id}' is not authorized for this Sonar group. Use /sessions to list available sessions."
                        ),
                    )
                    .await?;
                    return Ok(());
                }
                let session = match self.live_session(&session_id).await {
                    Ok(session) => session,
                    Err(reason) => {
                        self.send_text(
                            &message.user,
                            format!(
                                "Session '{session_id}' cannot be activated ({reason}). Use /new or /sessions to recover."
                            ),
                        )
                        .await?;
                        return Ok(());
                    }
                };
                self.pairing_store
                    .activate_session(&message.user, &session.id)
                    .await?;
                let session = match self.live_session(&session.id).await {
                    Ok(session) => session,
                    Err(reason) => {
                        if session_id != active_session_id {
                            self.pairing_store
                                .activate_session(&message.user, active_session_id)
                                .await?;
                        }
                        self.send_text(
                            &message.user,
                            format!(
                                "Session '{session_id}' became unavailable ({reason}). Use /use or /new to recover."
                            ),
                        )
                        .await?;
                        return Ok(());
                    }
                };
                self.send_text(
                    &message.user,
                    format!(
                        "Activated Goose session '{}' ({}).",
                        display_session_name(&session.name),
                        session.id
                    ),
                )
                .await?;
            }
            SonarSessionCommand::Session => {
                let body = match self.live_session(active_session_id).await {
                    Ok(session) => format!(
                        "Active Goose session: '{}' ({}).",
                        display_session_name(&session.name),
                        session.id
                    ),
                    Err(reason) => format!(
                        "Active session '{active_session_id}' is unavailable ({reason}). Use /new or /sessions to recover."
                    ),
                };
                self.send_text(&message.user, body).await?;
            }
        }
        Ok(())
    }

    async fn live_session(&self, session_id: &str) -> Result<Session, String> {
        let session = self
            .agent_manager
            .session_manager()
            .get_session(session_id, false)
            .await
            .map_err(|_| "not found".to_string())?;
        if session.archived_at.is_some() {
            return Err("archived".into());
        }
        Ok(session)
    }

    async fn send_text(&self, user: &PlatformUser, body: String) -> anyhow::Result<()> {
        self.gateway
            .send_message(user, OutgoingMessage::Text { body })
            .await
    }

    async fn complete_pairing(
        &self,
        user: &PlatformUser,
        target_session_id: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Some(session_id) = target_session_id {
            let session = self
                .agent_manager
                .session_manager()
                .get_session(session_id, false)
                .await?;
            let paired_at = chrono::Utc::now().timestamp();
            self.pairing_store
                .pair_with_session(
                    user,
                    &session.id,
                    paired_at,
                    self.config.gateway_type == "sonar",
                )
                .await?;
            let body = if self.config.gateway_type == "sonar" {
                format!(
                    "Authorized this group with Goose session '{}'. Use /new, /sessions, /use, or /session without another pairing code.",
                    session.name
                )
            } else {
                "Paired! You can now chat with goose.".into()
            };
            self.send_text(user, body).await?;
            return Ok(());
        }

        let session = self.create_gateway_session(user, None).await?;

        let now = chrono::Utc::now().timestamp();
        if let Err(error) = self
            .pairing_store
            .pair_with_session(user, &session.id, now, self.config.gateway_type == "sonar")
            .await
        {
            if let Err(cleanup_error) = self
                .agent_manager
                .session_manager()
                .delete_session(&session.id)
                .await
            {
                tracing::error!(
                    session_id = %session.id,
                    %cleanup_error,
                    "failed to delete a gateway session after pairing failed"
                );
            }
            return Err(error);
        }

        let body = if self.config.gateway_type == "sonar" {
            "Authorized this group. You can chat with Goose or use /new, /sessions, /use, and /session without another pairing code.".into()
        } else {
            "Paired! You can now chat with goose.".into()
        };
        self.send_text(user, body).await?;

        Ok(())
    }

    async fn create_gateway_session(
        &self,
        user: &PlatformUser,
        requested_name: Option<&str>,
    ) -> anyhow::Result<Session> {
        let working_dir = gateway_working_dir(&user.platform, &user.user_id);
        std::fs::create_dir_all(&working_dir)?;

        let session_name = requested_name.map(str::to_string).unwrap_or_else(|| {
            format!(
                "{}/{}",
                user.platform,
                user.display_name.as_deref().unwrap_or(&user.user_id)
            )
        });
        let config = Config::global();
        let manager = self.agent_manager.session_manager();
        let session = manager
            .create_session(
                working_dir,
                session_name,
                SessionType::Gateway,
                config.get_goose_mode().unwrap_or_default(),
            )
            .await?;

        let result = async {
            let mut update = manager.update(&session.id);
            if let Some(name) = requested_name {
                update = update.user_provided_name(name);
            }
            let provider = config.get_goose_provider().ok();
            if let Some(ref provider) = provider {
                update = update.provider_name(provider);
            }
            if let (Some(ref provider), Ok(model_name)) = (&provider, config.get_goose_model()) {
                if let Ok(model_config) =
                    crate::model_config::model_config_from_user_config(provider, &model_name)
                {
                    update = update.model_config(model_config);
                }
            }

            let mut extensions = get_enabled_extensions();
            extensions.extend(crate::plugins::mcp_servers::enabled_plugin_mcp_servers(
                Some(&session.working_dir),
            ));
            let extensions_state = EnabledExtensionsState::new(extensions);
            let mut extension_data = session.extension_data.clone();
            if let Err(error) = extensions_state.to_extension_data(&mut extension_data) {
                tracing::warn!(%error, "failed to initialize gateway session extensions");
            } else {
                update = update.extension_data(extension_data);
            }
            update.apply().await?;
            manager.get_session(&session.id, false).await
        }
        .await;

        match result {
            Ok(session) => Ok(session),
            Err(error) => {
                if let Err(cleanup_error) = manager.delete_session(&session.id).await {
                    tracing::error!(
                        session_id = %session.id,
                        %cleanup_error,
                        "failed to delete a partially initialized gateway session"
                    );
                }
                Err(error)
            }
        }
    }

    /// Sync the session's provider, model, and extensions with the current
    /// global config so gateway sessions always reflect what the user has
    /// configured in the desktop app.  Returns `true` if extensions changed
    /// (which means the caller must recreate the agent so stale extension
    /// processes are torn down).
    async fn sync_session_config(&self, session: &Session) -> anyhow::Result<bool> {
        let config = Config::global();
        let manager = self.agent_manager.session_manager();

        // --- current global config ---
        let current_provider = config.get_goose_provider().ok();
        let current_model_name = config.get_goose_model().ok();
        let mut current_extensions = get_enabled_extensions();
        current_extensions.extend(crate::plugins::mcp_servers::enabled_plugin_mcp_servers(
            Some(&session.working_dir),
        ));
        let current_mode = config.get_goose_mode().unwrap_or_default();

        // --- what the session has ---
        let session_extensions: Vec<ExtensionConfig> =
            EnabledExtensionsState::from_extension_data(&session.extension_data)
                .map(|s| s.extensions)
                .unwrap_or_default();

        let provider_changed = current_provider.as_deref() != session.provider_name.as_deref();
        let model_changed = current_model_name.as_deref()
            != session.model_config.as_ref().map(|m| m.model_name.as_str());
        let extensions_changed = current_extensions != session_extensions;
        let mode_changed = current_mode != session.goose_mode;

        if !provider_changed && !model_changed && !extensions_changed && !mode_changed {
            return Ok(false);
        }

        tracing::info!(
            session_id = %session.id,
            provider_changed,
            model_changed,
            extensions_changed,
            mode_changed,
            "syncing gateway session with current config"
        );

        let mut update = manager.update(&session.id);

        if let Some(ref provider) = current_provider {
            update = update.provider_name(provider);
        }
        if let (Some(ref provider), Some(ref model_name)) = (&current_provider, &current_model_name)
        {
            if let Ok(model_config) =
                crate::model_config::model_config_from_user_config(provider, model_name)
            {
                update = update.model_config(model_config);
            }
        }

        if extensions_changed {
            let extensions_state = EnabledExtensionsState::new(current_extensions);
            let mut extension_data = session.extension_data.clone();
            if let Err(e) = extensions_state.to_extension_data(&mut extension_data) {
                tracing::warn!(error = %e, "failed to update gateway session extensions");
            } else {
                update = update.extension_data(extension_data);
            }
        }

        if mode_changed {
            update = update.goose_mode(current_mode);
        }

        update.apply().await?;
        Ok(extensions_changed)
    }

    async fn relay_to_session(
        &self,
        message: &IncomingMessage,
        session_id: &str,
    ) -> anyhow::Result<()> {
        let cancel = CancellationToken::new();
        if let Err(error) = self
            .agent_manager
            .try_register_cancel_token(session_id, cancel.clone())
            .await
        {
            self.gateway
                .send_message(
                    &message.user,
                    OutgoingMessage::Text {
                        body: format!("Session is busy: {error}"),
                    },
                )
                .await?;
            return Ok(());
        }

        let busy_guard =
            SessionBusyGuard::new(self.agent_manager.clone(), session_id, cancel.clone());
        let result = self.run_session(message, session_id, cancel).await;
        busy_guard.release().await;
        result
    }

    async fn run_session(
        &self,
        message: &IncomingMessage,
        session_id: &str,
        cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        self.gateway
            .send_message(&message.user, OutgoingMessage::Typing)
            .await?;

        let session = self
            .agent_manager
            .session_manager()
            .get_session(session_id, false)
            .await?;

        let extensions_changed = if session.session_type == SessionType::Gateway {
            self.sync_session_config(&session).await?
        } else {
            false
        };
        if extensions_changed {
            self.agent_manager.remove_loaded_agent(session_id).await?;
        }

        let agent = self
            .agent_manager
            .get_or_create_agent(session_id.to_string())
            .await?;

        // Re-read the session after sync so restore picks up the new values.
        let session = self
            .agent_manager
            .session_manager()
            .get_session(session_id, false)
            .await?;

        // Ensure provider is configured (handles first use and LRU eviction).
        if let Err(e) = agent.restore_provider_from_session(&session).await {
            self.gateway
                .send_message(
                    &message.user,
                    OutgoingMessage::Text {
                        body: format!("⚠️ Failed to configure provider: {e}"),
                    },
                )
                .await?;
            return Ok(());
        }

        // Load extensions (skips any already loaded on the agent).
        agent.load_extensions_from_session(&session).await;

        let prompt = message
            .sender_label
            .as_deref()
            .map(|sender| format!("[{sender}] {}", message.text))
            .unwrap_or_else(|| message.text.clone());
        let user_message = Message::user().with_text(&prompt);

        // Cap tool-calling loops so the agent doesn't run away doing
        // dozens of tool calls before responding.  After this many
        // LLM→tool round-trips the agent will stop and reply with
        // whatever it has.
        //
        // Honors `GOOSE_GATEWAY_MAX_TURNS` (gateway-specific override) and
        // `GOOSE_MAX_TURNS` (global), falling back to a conservative default
        // so the limit is configurable without editing the source.
        let config = Config::global();
        let max_turns = resolve_gateway_max_turns(
            config.get_param::<u32>("GOOSE_GATEWAY_MAX_TURNS").ok(),
            config.get_param::<u32>("GOOSE_MAX_TURNS").ok(),
        );

        let session_config = SessionConfig {
            id: session_id.to_string(),
            schedule_id: None,
            max_turns: Some(max_turns),
            retry_config: None,
        };

        let mut stream = match agent
            .reply(user_message, session_config, Some(cancel))
            .await
        {
            Ok(s) => s,
            Err(e) => {
                self.gateway
                    .send_message(
                        &message.user,
                        OutgoingMessage::Text {
                            body: format!("⚠️ Failed to start agent: {e}"),
                        },
                    )
                    .await?;
                return Ok(());
            }
        };

        // Telegram stops showing "typing…" after ~5 seconds.  Re-send the
        // indicator every 4 s so the user always sees activity while the
        // agent is working (tool calls, LLM round-trips, etc.).
        let typing_cancel = CancellationToken::new();
        let typing_gateway = self.gateway.clone();
        let typing_user = message.user.clone();
        let typing_handle = tokio::spawn({
            let cancel = typing_cancel.clone();
            async move {
                let mut interval = tokio::time::interval(Duration::from_secs(4));
                interval.tick().await; // first tick is immediate, skip it
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        _ = interval.tick() => {
                            if let Err(e) = typing_gateway
                                .send_message(&typing_user, OutgoingMessage::Typing)
                                .await
                            {
                                tracing::debug!(error = %e, "failed to re-send typing indicator");
                                break;
                            }
                        }
                    }
                }
            }
        });

        // Buffer text within a single assistant message so we send one
        // Telegram message per LLM turn rather than per-chunk.  When a
        // ToolRequest appears in the same message we flush the buffer
        // first — the user sees "Let me check…" immediately, then the
        // typing indicator while the tool runs, then the next response.
        let mut pending_text = String::new();
        let mut sent_any = false;
        let mut event_count: u64 = 0;

        while let Some(event) = stream.next().await {
            event_count += 1;
            match event {
                Ok(AgentEvent::Message(ref msg)) => {
                    tracing::debug!(
                        session_id,
                        role = ?msg.role,
                        content_items = msg.content.len(),
                        "gateway stream: message event #{event_count}"
                    );
                    if msg.role == rmcp::model::Role::Assistant {
                        for content in &msg.content {
                            match content {
                                MessageContent::Text(t) => {
                                    if !t.text.is_empty() {
                                        pending_text.push_str(&t.text);
                                    }
                                }
                                MessageContent::ToolRequest(req) => {
                                    // Flush any accumulated text before
                                    // the tool runs — the user sees the
                                    // assistant's intent immediately.
                                    if !pending_text.is_empty() {
                                        let _ = self
                                            .gateway
                                            .send_message(
                                                &message.user,
                                                OutgoingMessage::Text {
                                                    body: std::mem::take(&mut pending_text),
                                                },
                                            )
                                            .await;
                                        sent_any = true;
                                    }
                                    if let Ok(call) = &req.tool_call {
                                        tracing::debug!(
                                            session_id,
                                            tool = %call.name,
                                            "gateway stream: tool request"
                                        );
                                        let _ = self
                                            .gateway
                                            .send_message(&message.user, OutgoingMessage::Typing)
                                            .await;
                                    }
                                }
                                MessageContent::ToolResponse(resp) => {
                                    tracing::debug!(
                                        session_id,
                                        id = %resp.id,
                                        success = resp.tool_result.is_ok(),
                                        "gateway stream: tool response"
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Ok(AgentEvent::Usage(_)) => {}
                Ok(AgentEvent::MessageUsage { .. }) => {}
                Ok(AgentEvent::McpNotification(_)) => {
                    tracing::debug!(
                        session_id,
                        "gateway stream: mcp notification #{event_count}"
                    );
                }
                Ok(AgentEvent::HistoryReplaced(_)) => {
                    tracing::debug!(
                        session_id,
                        "gateway stream: history replaced #{event_count}"
                    );
                }
                Err(e) => {
                    tracing::error!(session_id, error = %e, "gateway stream: error at event #{event_count}");
                    // Stop typing indicator before sending error.
                    typing_cancel.cancel();
                    let _ = typing_handle.await;
                    self.gateway
                        .send_message(
                            &message.user,
                            OutgoingMessage::Text {
                                body: format!("⚠️ Agent error: {e}"),
                            },
                        )
                        .await?;
                    return Ok(());
                }
            }
        }

        // Stream finished — stop the typing indicator.
        typing_cancel.cancel();
        let _ = typing_handle.await;

        tracing::debug!(
            session_id,
            event_count,
            pending_text_len = pending_text.len(),
            sent_any,
            "gateway stream: finished"
        );

        // Send any remaining buffered text (this is typically the final
        // assistant response after the last tool round-trip).
        if !pending_text.is_empty() {
            self.gateway
                .send_message(&message.user, OutgoingMessage::Text { body: pending_text })
                .await?;
        } else if !sent_any {
            // Nothing was ever sent — let the user know.
            self.gateway
                .send_message(
                    &message.user,
                    OutgoingMessage::Text {
                        body: "(No response)".to_string(),
                    },
                )
                .await?;
        }

        Ok(())
    }
}

fn gateway_working_dir(platform: &str, user_id: &str) -> PathBuf {
    Paths::config_dir()
        .join("gateway")
        .join(platform)
        .join(user_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_no_overrides() {
        assert_eq!(
            resolve_gateway_max_turns(None, None),
            DEFAULT_GATEWAY_MAX_TURNS
        );
    }

    #[test]
    fn uses_global_max_turns_when_gateway_unset() {
        assert_eq!(resolve_gateway_max_turns(None, Some(42)), 42);
    }

    #[test]
    fn gateway_override_wins_over_global() {
        assert_eq!(resolve_gateway_max_turns(Some(10), Some(42)), 10);
    }

    #[test]
    fn gateway_override_used_when_global_unset() {
        assert_eq!(resolve_gateway_max_turns(Some(25), None), 25);
    }

    #[test]
    fn parses_sonar_session_commands_without_changing_opaque_ids() {
        assert_eq!(
            parse_sonar_session_command(" /new Release work "),
            Some(Ok(SonarSessionCommand::New(Some("Release work".into()))))
        );
        assert_eq!(
            parse_sonar_session_command("/new"),
            Some(Ok(SonarSessionCommand::New(None)))
        );
        assert_eq!(
            parse_sonar_session_command("/sessions"),
            Some(Ok(SonarSessionCommand::Sessions))
        );
        assert_eq!(
            parse_sonar_session_command("/use 20260712_50"),
            Some(Ok(SonarSessionCommand::Use("20260712_50".into())))
        );
        assert_eq!(
            parse_sonar_session_command("/session"),
            Some(Ok(SonarSessionCommand::Session))
        );
    }

    #[test]
    fn rejects_malformed_known_commands_and_forwards_unknown_commands() {
        assert!(matches!(
            parse_sonar_session_command("/use"),
            Some(Err(message)) if message == "Usage: /use <session-id>"
        ));
        assert!(matches!(
            parse_sonar_session_command("/use first second"),
            Some(Err(message)) if message == "Usage: /use <session-id>"
        ));
        assert!(matches!(
            parse_sonar_session_command("/sessions now"),
            Some(Err(message)) if message == "Usage: /sessions"
        ));
        assert_eq!(parse_sonar_session_command("/useful prompt"), None);
        assert_eq!(parse_sonar_session_command("/other prompt"), None);
        assert_eq!(parse_sonar_session_command("ordinary prompt"), None);
    }

    #[test]
    fn validates_remote_session_names() {
        assert_eq!(
            validate_remote_session_name(Some("  Release work  ".into())).unwrap(),
            Some("Release work".into())
        );
        assert_eq!(
            validate_remote_session_name(Some("  ".into())).unwrap(),
            None
        );
        assert!(validate_remote_session_name(Some("bad\nname".into())).is_err());
        assert!(validate_remote_session_name(Some("x".repeat(81))).is_err());
        assert_eq!(
            validate_remote_session_name(Some("こんにちは".into())).unwrap(),
            Some("こんにちは".into())
        );
    }
}
