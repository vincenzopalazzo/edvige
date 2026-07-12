use std::collections::HashMap;

use agent_client_protocol::{JsonRpcRequest, JsonRpcResponse};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::EmptyResponse;

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GatewayPairedUserDto {
    pub platform: String,
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub session_id: String,
    pub paired_at: i64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GatewayStatusDto {
    pub gateway_type: String,
    pub running: bool,
    pub configured: bool,
    pub paired_users: Vec<GatewayPairedUserDto>,
    #[serde(default)]
    pub info: HashMap<String, String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/gateways/status",
    response = GatewayStatusResponse
)]
pub struct GatewayStatusRequest {}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct GatewayStatusResponse {
    pub gateways: Vec<GatewayStatusDto>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/gateways/sonar/start",
    response = StartSonarGatewayResponse
)]
pub struct StartSonarGatewayRequest {
    pub controllers: Vec<String>,
    #[serde(default)]
    pub relays: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
pub struct StartSonarGatewayResponse {
    pub gateway: GatewayStatusDto,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/gateways/sonar/pair",
    response = PairSonarGatewayResponse
)]
#[serde(rename_all = "camelCase")]
pub struct PairSonarGatewayRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcResponse)]
#[serde(rename_all = "camelCase")]
pub struct PairSonarGatewayResponse {
    pub code: String,
    pub expires_at: i64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/gateways/sonar/stop",
    response = EmptyResponse
)]
pub struct StopSonarGatewayRequest {
    #[serde(default)]
    pub forget: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema, JsonRpcRequest)]
#[request(
    method = "_goose/unstable/gateways/sonar/unpair",
    response = EmptyResponse
)]
#[serde(rename_all = "camelCase")]
pub struct UnpairSonarGatewayRequest {
    pub group_id: String,
}
