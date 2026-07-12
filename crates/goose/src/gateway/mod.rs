pub mod handler;
pub mod manager;
pub mod pairing;
#[cfg(feature = "sonar-gateway")]
pub mod sonar;
pub mod telegram;
pub mod telegram_format;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

use handler::GatewayHandler;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformUser {
    pub platform: String,
    pub user_id: String,
    pub display_name: Option<String>,
}

impl PartialEq for PlatformUser {
    fn eq(&self, other: &Self) -> bool {
        self.platform == other.platform && self.user_id == other.user_id
    }
}

impl Eq for PlatformUser {}

impl std::hash::Hash for PlatformUser {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.platform.hash(state);
        self.user_id.hash(state);
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IncomingMessage {
    pub user: PlatformUser,
    pub sender_label: Option<String>,
    pub text: String,
    pub platform_message_id: Option<String>,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Attachment {
    pub filename: String,
    pub mime_type: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutgoingMessage {
    Text { body: String },
    Typing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PairingState {
    Unpaired,
    PendingCode {
        code: String,
        expires_at: i64,
    },
    Paired {
        #[serde(with = "persisted_session_id")]
        session_id: String,
        paired_at: i64,
        #[serde(default, with = "persisted_session_ids")]
        session_ids: Vec<String>,
    },
}

const PERSISTED_SESSION_ID_PREFIX: &str = "goose-session-id:";

fn decode_persisted_session_id<E: serde::de::Error>(value: serde_json::Value) -> Result<String, E> {
    match value {
        serde_json::Value::String(value) => Ok(value
            .strip_prefix(PERSISTED_SESSION_ID_PREFIX)
            .unwrap_or(&value)
            .to_string()),
        serde_json::Value::Number(value) => {
            let value = value.to_string();
            if value.len() > 8 && value.bytes().all(|byte| byte.is_ascii_digit()) {
                let mut digits = value.chars();
                let date: String = digits.by_ref().take(8).collect();
                let suffix: String = digits.collect();
                let valid_date = chrono::NaiveDate::parse_from_str(&date, "%Y%m%d").is_ok();
                let valid_suffix = suffix.parse::<u64>().is_ok_and(|sequence| sequence > 0);
                if valid_date && valid_suffix {
                    Ok(format!("{date}_{suffix}"))
                } else {
                    Err(E::custom("invalid legacy Goose session id"))
                }
            } else {
                Err(E::custom("invalid legacy Goose session id"))
            }
        }
        _ => Err(E::custom("Goose session id must be a string")),
    }
}

mod persisted_session_id {
    use serde::{Deserialize, Serialize};

    pub fn serialize<S: serde::Serializer>(value: &str, serializer: S) -> Result<S::Ok, S::Error> {
        format!("{}{}", super::PERSISTED_SESSION_ID_PREFIX, value).serialize(serializer)
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<String, D::Error> {
        super::decode_persisted_session_id(serde_json::Value::deserialize(deserializer)?)
    }
}

mod persisted_session_ids {
    use serde::{Deserialize, Serialize};

    pub fn serialize<S: serde::Serializer>(
        values: &[String],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        values
            .iter()
            .map(|value| format!("{}{}", super::PERSISTED_SESSION_ID_PREFIX, value))
            .collect::<Vec<_>>()
            .serialize(serializer)
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<String>, D::Error> {
        Vec::<serde_json::Value>::deserialize(deserializer)?
            .into_iter()
            .map(super::decode_persisted_session_id)
            .collect()
    }
}

mod persisted_optional_session_id {
    use serde::{Deserialize, Serialize};

    pub fn serialize<S: serde::Serializer>(
        value: &Option<String>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value
            .as_ref()
            .map(|value| format!("{}{}", super::PERSISTED_SESSION_ID_PREFIX, value))
            .serialize(serializer)
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<String>, D::Error> {
        Option::<serde_json::Value>::deserialize(deserializer)?
            .map(super::decode_persisted_session_id)
            .transpose()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub gateway_type: String,
    pub platform_config: serde_json::Value,
    pub max_sessions: usize,
}

#[async_trait]
#[allow(dead_code)]
pub trait Gateway: Send + Sync + 'static {
    fn gateway_type(&self) -> &str;

    async fn start(&self, handler: GatewayHandler, cancel: CancellationToken)
        -> anyhow::Result<()>;

    async fn send_message(
        &self,
        user: &PlatformUser,
        message: OutgoingMessage,
    ) -> anyhow::Result<()>;

    async fn validate_config(&self) -> anyhow::Result<()>;

    fn info(&self) -> HashMap<String, String> {
        HashMap::new()
    }
}

pub fn create_gateway(config: &mut GatewayConfig) -> anyhow::Result<std::sync::Arc<dyn Gateway>> {
    match config.gateway_type.as_str() {
        "telegram" => Ok(std::sync::Arc::new(telegram::TelegramGateway::new(config)?)),
        #[cfg(feature = "sonar-gateway")]
        "sonar" => Ok(std::sync::Arc::new(sonar::SonarGateway::new(config)?)),
        #[cfg(not(feature = "sonar-gateway"))]
        "sonar" => anyhow::bail!("Sonar gateway support is not available in this build"),
        other => anyhow::bail!("Unknown gateway type: {}", other),
    }
}
