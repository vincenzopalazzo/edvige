use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use utoipa::ToSchema;

use crate::config::Config;
use crate::execution::manager::AgentManager;

use super::handler::GatewayHandler;
use super::pairing::PairingStore;
use super::{Gateway, GatewayConfig, PairingState, PlatformUser};

const GATEWAY_CONFIGS_KEY: &str = "gateway_configs";
const GATEWAY_STOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

fn secret_key_for(gateway_type: &str) -> String {
    format!("gateway_platform_config_{}", gateway_type)
}

/// Serialized form stored in goose config (no secrets).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedGatewayEntry {
    gateway_type: String,
    max_sessions: usize,
}

#[allow(dead_code)]
pub struct GatewayInstance {
    pub config: GatewayConfig,
    pub gateway: Arc<dyn Gateway>,
    pub cancel: CancellationToken,
    pub handle: tokio::task::JoinHandle<()>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PairedUserInfo {
    pub platform: String,
    pub user_id: String,
    pub display_name: Option<String>,
    pub session_id: String,
    pub paired_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GatewayStatus {
    pub gateway_type: String,
    pub running: bool,
    pub configured: bool,
    pub paired_users: Vec<PairedUserInfo>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub info: HashMap<String, String>,
}

pub struct GatewayManager {
    gateways: RwLock<HashMap<String, GatewayInstance>>,
    pairing_store: Arc<PairingStore>,
    agent_manager: Arc<AgentManager>,
}

impl Drop for GatewayManager {
    fn drop(&mut self) {
        for (_, instance) in self.gateways.get_mut().drain() {
            instance.cancel.cancel();
            instance.handle.abort();
        }
    }
}

impl GatewayManager {
    pub fn new(agent_manager: Arc<AgentManager>) -> anyhow::Result<Self> {
        let pairing_store = Arc::new(PairingStore::new()?);

        Ok(Self {
            gateways: RwLock::new(HashMap::new()),
            pairing_store,
            agent_manager,
        })
    }

    #[allow(dead_code)]
    pub fn pairing_store(&self) -> &Arc<PairingStore> {
        &self.pairing_store
    }

    /// Load saved gateway configs and start them. Called once at server startup.
    pub async fn check_auto_start(&self) {
        let configs = match Self::load_saved_configs() {
            Ok(configs) => configs,
            Err(e) => {
                tracing::warn!(error = %e, "failed to load saved gateway configs");
                return;
            }
        };

        for mut config in configs {
            let gateway = match super::create_gateway(&mut config) {
                Ok(gw) => gw,
                Err(e) => {
                    tracing::error!(
                        gateway = %config.gateway_type,
                        error = %e,
                        "failed to create saved gateway"
                    );
                    continue;
                }
            };

            if let Err(e) = self.start_gateway_internal(config.clone(), gateway).await {
                tracing::error!(
                    gateway = %config.gateway_type,
                    error = %e,
                    "failed to auto-start gateway"
                );
            } else {
                tracing::info!(gateway = %config.gateway_type, "gateway auto-started");
            }
        }
    }

    /// Start a gateway and persist its config for auto-start on next launch.
    pub async fn start_gateway(
        &self,
        config: GatewayConfig,
        gateway: Arc<dyn Gateway>,
    ) -> anyhow::Result<()> {
        self.start_gateway_internal(config.clone(), gateway).await?;
        Self::save_config(&config)?;
        Ok(())
    }

    /// Start a gateway for the current process without saving auto-start configuration.
    pub async fn start_gateway_ephemeral(
        &self,
        config: GatewayConfig,
        gateway: Arc<dyn Gateway>,
    ) -> anyhow::Result<()> {
        self.start_gateway_internal(config, gateway).await
    }

    pub fn persist_gateway_config(config: &GatewayConfig) -> anyhow::Result<()> {
        Self::save_config(config)
    }

    /// Stop a gateway while preserving pairings so it can be restarted.
    pub async fn stop_gateway(&self, gateway_type: &str) -> anyhow::Result<()> {
        let instance = self
            .gateways
            .write()
            .await
            .remove(gateway_type)
            .ok_or_else(|| anyhow::anyhow!("Gateway '{}' is not running", gateway_type))?;

        Self::stop_instance(instance).await;

        tracing::info!(gateway = %gateway_type, "gateway stopped");
        Ok(())
    }

    /// Stop a gateway (if running), clear pairings, and remove its saved config entirely.
    pub async fn remove_gateway(&self, gateway_type: &str) -> anyhow::Result<()> {
        // Stop if running (ignore error if not running).
        if let Some(instance) = self.gateways.write().await.remove(gateway_type) {
            Self::stop_instance(instance).await;
        }

        if let Err(e) = self
            .pairing_store
            .remove_all_for_platform(gateway_type)
            .await
        {
            tracing::warn!(gateway = %gateway_type, error = %e, "failed to clear pairings on remove");
        }

        Self::remove_saved_config(gateway_type);
        tracing::info!(gateway = %gateway_type, "gateway removed");
        Ok(())
    }

    async fn stop_instance(instance: GatewayInstance) {
        instance.cancel.cancel();
        let mut handle = instance.handle;
        if tokio::time::timeout(GATEWAY_STOP_TIMEOUT, &mut handle)
            .await
            .is_err()
        {
            handle.abort();
            let _ = handle.await;
        }
    }

    /// Restart a stopped gateway using its saved config.
    pub async fn restart_gateway(&self, gateway_type: &str) -> anyhow::Result<()> {
        self.reap_finished_gateway(gateway_type).await;
        if self.gateways.read().await.contains_key(gateway_type) {
            anyhow::bail!("Gateway '{}' is already running", gateway_type);
        }

        let configs = Self::load_saved_configs()?;
        let mut config = configs
            .into_iter()
            .find(|c| c.gateway_type == gateway_type)
            .ok_or_else(|| anyhow::anyhow!("No saved config for gateway '{}'", gateway_type))?;

        let gateway = super::create_gateway(&mut config)?;
        self.start_gateway_internal(config, gateway).await?;
        tracing::info!(gateway = %gateway_type, "gateway restarted");
        Ok(())
    }

    async fn start_gateway_internal(
        &self,
        config: GatewayConfig,
        gateway: Arc<dyn Gateway>,
    ) -> anyhow::Result<()> {
        let gw_type = config.gateway_type.clone();
        self.reap_finished_gateway(&gw_type).await;

        gateway.validate_config().await?;

        let cancel = CancellationToken::new();
        let handler = GatewayHandler::new(
            self.agent_manager.clone(),
            self.pairing_store.clone(),
            gateway.clone(),
            config.clone(),
        );

        let gateway_clone = gateway.clone();
        let cancel_clone = cancel.clone();
        let gateway_type_for_task = gw_type.clone();

        let mut gateways = self.gateways.write().await;
        if gateways.contains_key(&gw_type) {
            anyhow::bail!("Gateway '{}' is already running", gw_type);
        }

        let handle = tokio::spawn(async move {
            if let Err(e) = gateway_clone.start(handler, cancel_clone).await {
                tracing::error!(gateway = %gateway_type_for_task, error = %e, "gateway stopped with error");
            }
        });

        let instance = GatewayInstance {
            config,
            gateway,
            cancel,
            handle,
        };

        gateways.insert(gw_type, instance);
        Ok(())
    }

    async fn reap_finished_gateway(&self, gateway_type: &str) {
        let instance = {
            let mut gateways = self.gateways.write().await;
            let finished = gateways
                .get(gateway_type)
                .is_some_and(|instance| instance.handle.is_finished());
            finished.then(|| gateways.remove(gateway_type)).flatten()
        };
        if let Some(instance) = instance {
            Self::stop_instance(instance).await;
        }
    }

    async fn reap_finished_gateways(&self) {
        let instances = {
            let mut gateways = self.gateways.write().await;
            let finished: Vec<String> = gateways
                .iter()
                .filter(|(_, instance)| instance.handle.is_finished())
                .map(|(gateway_type, _)| gateway_type.clone())
                .collect();
            finished
                .into_iter()
                .filter_map(|gateway_type| gateways.remove(&gateway_type))
                .collect::<Vec<_>>()
        };
        for instance in instances {
            Self::stop_instance(instance).await;
        }
    }

    #[allow(dead_code)]
    pub async fn stop_all(&self) {
        let instances: Vec<(String, GatewayInstance)> =
            self.gateways.write().await.drain().collect();
        for (gateway_type, instance) in instances {
            Self::stop_instance(instance).await;
            tracing::info!(gateway = %gateway_type, "gateway stopped");
        }
    }

    #[allow(dead_code)]
    pub async fn is_running(&self, gateway_type: &str) -> bool {
        self.gateways
            .read()
            .await
            .get(gateway_type)
            .is_some_and(|instance| !instance.handle.is_finished())
    }

    #[allow(dead_code)]
    pub async fn list_running(&self) -> Vec<String> {
        self.gateways
            .read()
            .await
            .iter()
            .filter(|(_, instance)| !instance.handle.is_finished())
            .map(|(gateway_type, _)| gateway_type.clone())
            .collect()
    }

    pub async fn status(&self) -> Vec<GatewayStatus> {
        self.reap_finished_gateways().await;
        let running = self.gateways.read().await;
        let mut statuses = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for (gw_type, instance) in running.iter() {
            seen.insert(gw_type.clone());
            let paired_users = self
                .pairing_store
                .list_paired_users(gw_type)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|(user, session_id, paired_at)| PairedUserInfo {
                    platform: user.platform,
                    user_id: user.user_id,
                    display_name: user.display_name,
                    session_id,
                    paired_at,
                })
                .collect();

            statuses.push(GatewayStatus {
                gateway_type: gw_type.clone(),
                running: !instance.handle.is_finished(),
                configured: true,
                paired_users,
                info: instance.gateway.info(),
            });
        }

        // Include configured-but-stopped gateways.
        if let Ok(saved) = Self::load_saved_configs() {
            for config in saved {
                if seen.contains(&config.gateway_type) {
                    continue;
                }
                let gateway_type = config.gateway_type;
                let paired_users = self
                    .pairing_store
                    .list_paired_users(&gateway_type)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(user, session_id, paired_at)| PairedUserInfo {
                        platform: user.platform,
                        user_id: user.user_id,
                        display_name: user.display_name,
                        session_id,
                        paired_at,
                    })
                    .collect();
                statuses.push(GatewayStatus {
                    gateway_type,
                    running: false,
                    configured: true,
                    paired_users,
                    info: HashMap::new(),
                });
            }
        }

        statuses.sort_by(|a, b| a.gateway_type.cmp(&b.gateway_type));
        statuses
    }

    pub async fn unpair_user(&self, platform: &str, user_id: &str) -> anyhow::Result<bool> {
        let user = PlatformUser {
            platform: platform.to_string(),
            user_id: user_id.to_string(),
            display_name: None,
        };
        let state = self.pairing_store.get(&user).await?;
        if matches!(state, PairingState::Paired { .. }) {
            self.pairing_store.remove(&user).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn generate_pairing_code(
        &self,
        gateway_type: &str,
        session_id: Option<&str>,
    ) -> anyhow::Result<(String, i64)> {
        if let Some(session_id) = session_id {
            self.agent_manager
                .session_manager()
                .get_session(session_id, false)
                .await?;
        }
        let code = PairingStore::generate_code();
        let expires_at = chrono::Utc::now().timestamp() + 300;
        self.pairing_store
            .store_pending_code(&code, gateway_type, expires_at, session_id)
            .await?;
        Ok((code, expires_at))
    }

    // --- Config persistence ---

    fn load_saved_configs() -> anyhow::Result<Vec<GatewayConfig>> {
        let config = Config::global();

        let entries: Vec<SavedGatewayEntry> = match config.get_param(GATEWAY_CONFIGS_KEY) {
            Ok(entries) => entries,
            Err(_) => return Ok(Vec::new()),
        };

        let mut configs = Vec::new();
        for entry in entries {
            let platform_config: serde_json::Value =
                match config.get_secret(&secret_key_for(&entry.gateway_type)) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(
                            gateway = %entry.gateway_type,
                            error = %e,
                            "skipping gateway with missing platform config secret"
                        );
                        continue;
                    }
                };

            configs.push(GatewayConfig {
                gateway_type: entry.gateway_type,
                platform_config,
                max_sessions: entry.max_sessions,
            });
        }

        Ok(configs)
    }

    fn save_config(gw_config: &GatewayConfig) -> anyhow::Result<()> {
        let config = Config::global();

        // Save platform_config (contains secrets like bot tokens) to the secret store.
        config
            .set_secret(
                &secret_key_for(&gw_config.gateway_type),
                &gw_config.platform_config,
            )
            .map_err(|e| anyhow::anyhow!("failed to save gateway secret: {}", e))?;

        // Load existing entries, add/replace this one, save back.
        let mut entries: Vec<SavedGatewayEntry> =
            config.get_param(GATEWAY_CONFIGS_KEY).unwrap_or_default();
        entries.retain(|e| e.gateway_type != gw_config.gateway_type);
        entries.push(SavedGatewayEntry {
            gateway_type: gw_config.gateway_type.clone(),
            max_sessions: gw_config.max_sessions,
        });

        config
            .set_param(GATEWAY_CONFIGS_KEY, &entries)
            .map_err(|e| anyhow::anyhow!("failed to save gateway config: {}", e))?;

        Ok(())
    }

    fn remove_saved_config(gateway_type: &str) {
        let config = Config::global();

        // Remove the secret.
        if let Err(e) = config.delete_secret(&secret_key_for(gateway_type)) {
            tracing::warn!(error = %e, "failed to remove gateway secret");
        }

        // Remove from the config entries list.
        let mut entries: Vec<SavedGatewayEntry> =
            config.get_param(GATEWAY_CONFIGS_KEY).unwrap_or_default();
        entries.retain(|e| e.gateway_type != gateway_type);
        if let Err(e) = config.set_param(GATEWAY_CONFIGS_KEY, &entries) {
            tracing::warn!(error = %e, "failed to update gateway config list");
        }
    }
}
