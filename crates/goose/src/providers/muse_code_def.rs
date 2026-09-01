use anyhow::Result;
use futures::future::BoxFuture;
use goose_providers::anthropic::{
    AnthropicProvider, AnthropicProviderBuilder, ANTHROPIC_API_VERSION,
};
use goose_providers::api_client::{ApiClient, AuthMethod, TlsConfig};
use goose_providers::base::{
    model_info_for_provider_model, ConfigKey, ModelInfo, ProviderDescriptor, ProviderMetadata,
};

use crate::config::Config;
use crate::providers::base::ProviderDef;

pub const MUSE_CODE_PROVIDER_NAME: &str = "muse_code";
pub const MUSE_CODE_API_HOST: &str = "https://api.meta.ai";
const MUSE_CODE_DOC_URL: &str = "https://ai.developer.meta.com/docs/muse-code/subscriptions";

const MUSE_CODE_KNOWN_MODELS: &[&str] = &[
    "muse-spark-1.2",
    "muse-spark-1.1",
    "muse-spark-1.2-contributor",
];

pub struct MuseCodeProviderDef;

fn known_models() -> Vec<ModelInfo> {
    MUSE_CODE_KNOWN_MODELS
        .iter()
        .map(|&name| model_info_for_provider_model(MUSE_CODE_PROVIDER_NAME, name))
        .collect()
}

impl ProviderDescriptor for MuseCodeProviderDef {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::with_models(
            MUSE_CODE_PROVIDER_NAME,
            "Meta Muse Code",
            "Muse Spark models from a Meta Muse Code subscription",
            "muse-spark-1.2",
            known_models(),
            MUSE_CODE_DOC_URL,
            vec![
                ConfigKey::new("META_MODEL_API_KEY", true, true, None, true),
                ConfigKey::new("MUSE_CODE_HOST", true, false, Some(MUSE_CODE_API_HOST), false),
            ],
        )
        .with_setup_steps(vec![
            "Create an API key at https://ai.developer.meta.com (requires a Muse Code subscription)",
            "Copy the key and paste it above",
        ])
        .with_setup(
            crate::providers::catalog::ProviderSetupMetadata::api_key(
                crate::providers::catalog::ProviderSetupGroup::Default,
            )
            .with_docs_url(MUSE_CODE_DOC_URL),
        )
    }
}

impl ProviderDef for MuseCodeProviderDef {
    type Provider = AnthropicProvider;

    fn from_env(
        _extensions: Vec<crate::config::ExtensionConfig>,
        tls_config: Option<TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(from_env(tls_config))
    }
}

async fn from_env(tls_config: Option<TlsConfig>) -> Result<AnthropicProvider> {
    let config = Config::global();
    let api_key: String = config.get_secret("META_MODEL_API_KEY")?;
    let host: String = config
        .get_param("MUSE_CODE_HOST")
        .unwrap_or_else(|_| MUSE_CODE_API_HOST.to_string());

    let api_client = ApiClient::with_timeout_and_tls(
        host,
        AuthMethod::BearerToken(api_key),
        std::time::Duration::from_secs(crate::providers::base::DEFAULT_PROVIDER_TIMEOUT_SECS),
        tls_config,
    )?
    .with_request_builder(crate::session_context::session_id_request_builder())
    .with_header("anthropic-version", ANTHROPIC_API_VERSION)?;

    Ok(AnthropicProviderBuilder::new(api_client)
        .name(MUSE_CODE_PROVIDER_NAME)
        .custom_models(Some(known_models()))
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use goose_providers::base::Provider as _;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn metadata_declares_subscription_models_and_keys() {
        let metadata = MuseCodeProviderDef::metadata();
        assert_eq!(metadata.name, "muse_code");
        assert_eq!(metadata.default_model, "muse-spark-1.2");
        assert_eq!(
            metadata
                .known_models
                .iter()
                .map(|model| model.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "muse-spark-1.2",
                "muse-spark-1.1",
                "muse-spark-1.2-contributor"
            ]
        );
        assert_eq!(
            metadata
                .known_models
                .iter()
                .map(|model| model.context_limit)
                .collect::<Vec<_>>(),
            vec![Some(1_048_576), Some(1_000_000), Some(1_048_576)]
        );
        assert!(
            metadata.known_models.iter().all(|model| model.reasoning),
            "muse-spark models should resolve as reasoning models via the canonical registry"
        );
        assert!(metadata
            .config_keys
            .iter()
            .any(|key| key.name == "META_MODEL_API_KEY" && key.secret && key.primary));
    }

    #[tokio::test]
    async fn from_env_builds_provider_against_configured_host_with_bearer_auth() {
        let server = MockServer::start().await;
        Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data": [{"id": "muse-spark-1.2"}]})),
            )
            .mount(&server)
            .await;

        let _guard = env_lock::lock_env([
            ("GOOSE_PATH_ROOT", None::<&str>),
            ("META_MODEL_API_KEY", None::<&str>),
            ("MUSE_CODE_HOST", None::<&str>),
        ]);
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        std::env::set_var("GOOSE_PATH_ROOT", temp_dir.path());
        std::env::set_var("META_MODEL_API_KEY", "test-key");
        std::env::set_var("MUSE_CODE_HOST", server.uri());

        let provider = MuseCodeProviderDef::from_env(Vec::new(), None)
            .await
            .expect("provider should build");

        let models = provider
            .fetch_supported_models()
            .await
            .expect("models should be fetched from the configured host");
        assert_eq!(models, vec!["muse-spark-1.2".to_string()]);

        let requests = server.received_requests().await.expect("requests recorded");
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]
                .headers
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer test-key")
        );
    }
}
