use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::gateway::manager::{GatewayManager, GatewayStatus, PairedUserInfo};
use crate::gateway::{create_gateway, GatewayConfig};

const SONAR_GATEWAY_TYPE: &str = "sonar";
const SONAR_READY_TIMEOUT: Duration = Duration::from_secs(50);

impl GooseAcpAgent {
    fn desktop_gateway_manager(
        &self,
    ) -> Result<&Arc<GatewayManager>, agent_client_protocol::Error> {
        self.gateway_manager.as_ref().ok_or_else(|| {
            agent_client_protocol::Error::invalid_params()
                .data("Gateway control is only available from Goose Desktop")
        })
    }

    pub(super) async fn on_gateway_status(
        &self,
    ) -> Result<GatewayStatusResponse, agent_client_protocol::Error> {
        let gateways = self
            .desktop_gateway_manager()?
            .status()
            .await
            .into_iter()
            .map(GatewayStatusDto::from)
            .collect();
        Ok(GatewayStatusResponse { gateways })
    }

    pub(super) async fn on_start_sonar_gateway(
        &self,
        req: StartSonarGatewayRequest,
    ) -> Result<StartSonarGatewayResponse, agent_client_protocol::Error> {
        if req.controllers.is_empty() {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("At least one Sonar controller npub is required"));
        }

        let manager = self.desktop_gateway_manager()?;
        if manager.is_running(SONAR_GATEWAY_TYPE).await {
            manager
                .stop_gateway(SONAR_GATEWAY_TYPE)
                .await
                .internal_err()?;
        }

        let mut config = GatewayConfig {
            gateway_type: SONAR_GATEWAY_TYPE.to_string(),
            platform_config: serde_json::json!({
                "controllers": req.controllers,
                "relays": req.relays,
            }),
            max_sessions: 0,
        };
        let gateway = create_gateway(&mut config).map_err(|error| {
            agent_client_protocol::Error::invalid_params().data(error.to_string())
        })?;
        manager
            .start_gateway_ephemeral(config.clone(), gateway)
            .await
            .internal_err()?;

        let deadline = Instant::now() + SONAR_READY_TIMEOUT;
        let ready = async {
            loop {
                let status = manager
                    .status()
                    .await
                    .into_iter()
                    .find(|status| status.gateway_type == SONAR_GATEWAY_TYPE)
                    .ok_or_else(|| {
                        agent_client_protocol::Error::internal_error()
                            .data("Sonar gateway disappeared during startup")
                    })?;
                if !status.running {
                    return Err(agent_client_protocol::Error::internal_error()
                        .data("Sonar gateway stopped before becoming ready"));
                }
                if status.info.contains_key("npub") {
                    return Ok(status);
                }
                if Instant::now() >= deadline {
                    return Err(agent_client_protocol::Error::internal_error()
                        .data("Sonar gateway did not become ready before the startup timeout"));
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        .await;

        match ready {
            Ok(status) => {
                if let Err(error) = GatewayManager::persist_gateway_config(&config) {
                    let _ = manager.stop_gateway(SONAR_GATEWAY_TYPE).await;
                    return Err(error).internal_err();
                }
                Ok(StartSonarGatewayResponse {
                    gateway: status.into(),
                })
            }
            Err(error) => {
                let _ = manager.stop_gateway(SONAR_GATEWAY_TYPE).await;
                Err(error)
            }
        }
    }

    pub(super) async fn on_pair_sonar_gateway(
        &self,
        req: PairSonarGatewayRequest,
    ) -> Result<PairSonarGatewayResponse, agent_client_protocol::Error> {
        let manager = self.desktop_gateway_manager()?;
        let running = manager
            .status()
            .await
            .into_iter()
            .any(|status| status.gateway_type == SONAR_GATEWAY_TYPE && status.running);
        if !running {
            return Err(
                agent_client_protocol::Error::invalid_params().data("Sonar gateway is not running")
            );
        }
        let (code, expires_at) = manager
            .generate_pairing_code(SONAR_GATEWAY_TYPE, req.session_id.as_deref())
            .await
            .internal_err()?;
        Ok(PairSonarGatewayResponse { code, expires_at })
    }

    pub(super) async fn on_stop_sonar_gateway(
        &self,
        req: StopSonarGatewayRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        let manager = self.desktop_gateway_manager()?;
        if req.forget {
            manager
                .remove_gateway(SONAR_GATEWAY_TYPE)
                .await
                .internal_err()?;
        } else if manager.is_running(SONAR_GATEWAY_TYPE).await {
            manager
                .stop_gateway(SONAR_GATEWAY_TYPE)
                .await
                .internal_err()?;
        }
        Ok(EmptyResponse {})
    }

    pub(super) async fn on_unpair_sonar_gateway(
        &self,
        req: UnpairSonarGatewayRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        if req.group_id.trim().is_empty() {
            return Err(
                agent_client_protocol::Error::invalid_params().data("A Sonar group ID is required")
            );
        }
        let removed = self
            .desktop_gateway_manager()?
            .unpair_user(SONAR_GATEWAY_TYPE, &req.group_id)
            .await
            .internal_err()?;
        if !removed {
            return Err(agent_client_protocol::Error::invalid_params()
                .data("The Sonar group is not paired"));
        }
        Ok(EmptyResponse {})
    }
}

impl From<PairedUserInfo> for GatewayPairedUserDto {
    fn from(user: PairedUserInfo) -> Self {
        Self {
            platform: user.platform,
            user_id: user.user_id,
            display_name: user.display_name,
            session_id: user.session_id,
            paired_at: user.paired_at,
        }
    }
}

impl From<GatewayStatus> for GatewayStatusDto {
    fn from(status: GatewayStatus) -> Self {
        Self {
            gateway_type: status.gateway_type,
            running: status.running,
            configured: status.configured,
            paired_users: status.paired_users.into_iter().map(Into::into).collect(),
            info: status.info,
        }
    }
}
