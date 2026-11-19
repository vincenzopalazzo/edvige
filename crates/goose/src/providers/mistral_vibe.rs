use super::api_client::{ApiClient, AuthMethod, AuthProvider};
use super::base::{ConfigKey, MessageStream, Provider, ProviderDef, ProviderMetadata};
use super::catalog::{
    ProviderSetupCategory, ProviderSetupGroup, ProviderSetupMetadata, ProviderSetupMethod,
};
use super::openai_compatible::OpenAiCompatibleProvider;
use crate::config::Config;
use crate::conversation::message::Message;
use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use base64::Engine;
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use goose_providers::errors::ProviderError;
use goose_providers::model::ModelConfig;
use reqwest::StatusCode;
use rmcp::model::Tool;
use serde::Deserialize;
use serde_json::json;
use sha2::Digest;
use std::time::Duration;
use url::Url;

pub(crate) const MISTRAL_VIBE_PROVIDER_NAME: &str = "mistral_vibe";
pub(crate) const MISTRAL_VIBE_API_KEY: &str = "MISTRAL_VIBE_API_KEY";
const MISTRAL_API_HOST: &str = "https://api.mistral.ai/v1";
const MISTRAL_VIBE_DEFAULT_MODEL: &str = "devstral-2512";
const MISTRAL_VIBE_KNOWN_MODELS: &[&str] =
    &["devstral-2512", "mistral-medium-latest", "codestral-2508"];
const MISTRAL_VIBE_DOC_URL: &str = "https://docs.mistral.ai/vibe/code/cli/api-keys-profiles";

// The browser sign-in Mistral Vibe uses: console.mistral.ai mints a workspace
// API key once the user approves, released only against this process's PKCE
// verifier. Keys minted this way draw from the workspace's Le Chat plan.
const SIGN_IN_BROWSER_ORIGIN: &str = "https://console.mistral.ai";
const SIGN_IN_API_BASE: &str = "https://console.mistral.ai/api";
const SIGN_IN_POLL_INTERVAL: Duration = Duration::from_secs(3);
const MAX_CONSECUTIVE_POLL_FAILURES: u32 = 3;
const WORKSPACE_NOT_ELIGIBLE: &str = "this Mistral workspace can't be used for Mistral Vibe. \
     Sign in again and pick another workspace, or ask your administrator";

#[derive(Deserialize)]
struct SignInProcess {
    process_id: String,
    sign_in_url: String,
    poll_url: String,
    expires_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct SignInPoll {
    status: String,
    exchange_token: Option<String>,
    message: Option<String>,
}

#[derive(Deserialize)]
struct SignInExchange {
    api_key: String,
}

struct BrowserSignIn {
    client: reqwest::Client,
    browser_origin: String,
    api_base: String,
    poll_interval: Duration,
}

impl BrowserSignIn {
    fn mistral() -> Self {
        Self {
            client: reqwest::Client::new(),
            browser_origin: SIGN_IN_BROWSER_ORIGIN.to_string(),
            api_base: SIGN_IN_API_BASE.to_string(),
            poll_interval: SIGN_IN_POLL_INTERVAL,
        }
    }

    async fn run(&self, open_browser: impl Fn(&str)) -> Result<String> {
        let verifier = nanoid::nanoid!(64);
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(sha2::Sha256::digest(verifier.as_bytes()));
        let process: SignInProcess = self
            .client
            .post(format!("{}/vibe/sign-in", self.api_base))
            .json(&json!({"code_challenge": challenge, "code_challenge_method": "S256"}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        require_under(&process.sign_in_url, &self.browser_origin)?;
        require_under(&process.poll_url, &self.api_base)?;

        open_browser(&process.sign_in_url);
        let exchange_token = self.wait_for_exchange_token(&process).await?;

        let response = self
            .client
            .post(format!(
                "{}/vibe/sign-in/{}/exchange",
                self.api_base, process.process_id
            ))
            .json(&json!({"exchange_token": exchange_token, "code_verifier": verifier}))
            .send()
            .await?;
        if response.status() == StatusCode::FORBIDDEN {
            bail!(WORKSPACE_NOT_ELIGIBLE);
        }
        let exchange: SignInExchange = response.error_for_status()?.json().await?;
        Ok(exchange.api_key)
    }

    async fn wait_for_exchange_token(&self, process: &SignInProcess) -> Result<String> {
        let mut failures = 0;
        while Utc::now() < process.expires_at {
            match self.poll(&process.poll_url).await {
                Ok(poll) => {
                    failures = 0;
                    match poll.status.as_str() {
                        "pending" => {}
                        "completed" => {
                            return poll.exchange_token.ok_or_else(|| {
                                anyhow!("sign-in completed without an exchange token")
                            })
                        }
                        "expired" => bail!("sign-in expired"),
                        "denied" => bail!("sign-in was denied"),
                        "error" => bail!(poll.message.unwrap_or_else(|| "sign-in failed".into())),
                        other => bail!("sign-in returned an unknown state: {other}"),
                    }
                }
                Err(error) => {
                    failures += 1;
                    if failures >= MAX_CONSECUTIVE_POLL_FAILURES {
                        return Err(error);
                    }
                }
            }
            tokio::time::sleep(self.poll_interval).await;
        }
        bail!("sign-in expired")
    }

    async fn poll(&self, poll_url: &str) -> Result<SignInPoll> {
        let response = self.client.get(poll_url).send().await?;
        if response.status() == StatusCode::GONE {
            return Ok(SignInPoll {
                status: "expired".to_string(),
                exchange_token: None,
                message: None,
            });
        }
        Ok(response.error_for_status()?.json().await?)
    }
}

/// The sign-in service names the page to open and the URL to poll; refuse
/// anything outside its own origin before a browser or poll touches it.
fn require_under(candidate: &str, base: &str) -> Result<()> {
    let candidate_url = Url::parse(candidate)?;
    let base_url = Url::parse(base)?;
    let base_path = base_url.path().trim_end_matches('/');
    let path = candidate_url.path();
    let under_base = path == base_path || path.starts_with(&format!("{base_path}/"));
    if candidate_url.origin() == base_url.origin() && under_base {
        Ok(())
    } else {
        bail!("sign-in returned an unexpected URL: {candidate}")
    }
}

fn open_sign_in_page(url: &str) {
    if let Err(error) = webbrowser::open(url) {
        tracing::warn!("Failed to open browser: {}", error);
    }
    eprintln!("\nTo sign in to Mistral, open this URL in any browser:\n  {url}\n");
}

struct StoredApiKey;

#[async_trait]
impl AuthProvider for StoredApiKey {
    async fn get_auth_header(&self) -> Result<(String, String)> {
        let api_key: String = Config::global()
            .get_secret(MISTRAL_VIBE_API_KEY)
            .map_err(|_| anyhow!("Not signed in to Mistral: sign in from the provider settings"))?;
        Ok(("Authorization".to_string(), format!("Bearer {api_key}")))
    }
}

/// `mistral`'s OpenAI-compatible API behind Mistral Vibe's browser sign-in,
/// so the key comes from the user's Le Chat workspace instead of a paste.
#[derive(serde::Serialize)]
pub struct MistralVibeProvider {
    #[serde(skip)]
    inner: OpenAiCompatibleProvider,
}

#[async_trait]
impl Provider for MistralVibeProvider {
    fn get_name(&self) -> &str {
        self.inner.get_name()
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        self.inner
            .stream(model_config, system, messages, tools)
            .await
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        self.inner.fetch_supported_models().await
    }

    async fn configure_oauth(&self) -> Result<(), ProviderError> {
        let api_key = BrowserSignIn::mistral()
            .run(open_sign_in_page)
            .await
            .map_err(|e| ProviderError::Authentication(format!("Mistral sign-in failed: {e}")))?;
        Config::global()
            .set_secret(MISTRAL_VIBE_API_KEY, &api_key)
            .map_err(|e| ProviderError::ExecutionError(e.to_string()))
    }
}

impl goose_providers::base::ProviderDescriptor for MistralVibeProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            MISTRAL_VIBE_PROVIDER_NAME,
            "Mistral Vibe (Le Chat Pro/Team)",
            "Sign in with your Mistral account to use your Le Chat Pro or Team plan, as Mistral Vibe does.",
            MISTRAL_VIBE_DEFAULT_MODEL,
            MISTRAL_VIBE_KNOWN_MODELS.to_vec(),
            MISTRAL_VIBE_DOC_URL,
            vec![ConfigKey::new_oauth(MISTRAL_VIBE_API_KEY, true, true, None, false)],
        )
        .with_setup(
            ProviderSetupMetadata::new(
                ProviderSetupCategory::Model,
                ProviderSetupMethod::OauthBrowser,
                ProviderSetupGroup::Default,
            )
            .with_docs_url(MISTRAL_VIBE_DOC_URL)
            .with_capabilities(false, true, false),
        )
    }
}

impl ProviderDef for MistralVibeProvider {
    type Provider = Self;

    fn from_env(
        _extensions: Vec<crate::config::ExtensionConfig>,
        tls_config: Option<crate::providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(async move {
            let api_client = ApiClient::new_with_tls(
                MISTRAL_API_HOST.to_string(),
                AuthMethod::Custom(Box::new(StoredApiKey)),
                tls_config,
            )?
            .with_request_builder(crate::session_context::session_id_request_builder());
            Ok(Self {
                inner: OpenAiCompatibleProvider::new(
                    MISTRAL_VIBE_PROVIDER_NAME.to_string(),
                    api_client,
                    String::new(),
                ),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn sign_in_server(sign_in_url: Option<String>) -> MockServer {
        let server = MockServer::start().await;
        let base = server.uri();
        Mock::given(method("POST"))
            .and(path("/api/vibe/sign-in"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "process_id": "p1",
                "sign_in_url": sign_in_url.unwrap_or_else(|| format!("{base}/vibe/sign-in/p1")),
                "poll_url": format!("{base}/api/vibe/sign-in/p1"),
                "expires_at": (Utc::now() + chrono::Duration::minutes(5)).to_rfc3339(),
            })))
            .mount(&server)
            .await;
        server
    }

    fn sign_in_against(server: &MockServer) -> BrowserSignIn {
        BrowserSignIn {
            client: reqwest::Client::new(),
            browser_origin: server.uri(),
            api_base: format!("{}/api", server.uri()),
            poll_interval: Duration::ZERO,
        }
    }

    async fn mock_poll(server: &MockServer, body: serde_json::Value, times: Option<u64>) {
        let mock = Mock::given(method("GET"))
            .and(path("/api/vibe/sign-in/p1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body));
        match times {
            Some(n) => mock.up_to_n_times(n).mount(server).await,
            None => mock.mount(server).await,
        }
    }

    #[tokio::test]
    async fn sign_in_exchanges_the_pkce_verifier_for_an_api_key() {
        let server = sign_in_server(None).await;
        mock_poll(&server, json!({"status": "pending"}), Some(1)).await;
        mock_poll(
            &server,
            json!({"status": "completed", "exchange_token": "t1"}),
            None,
        )
        .await;
        Mock::given(method("POST"))
            .and(path("/api/vibe/sign-in/p1/exchange"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"api_key": "key-1"})))
            .mount(&server)
            .await;

        let opened = Mutex::new(Vec::new());
        let api_key = sign_in_against(&server)
            .run(|url| opened.lock().unwrap().push(url.to_string()))
            .await
            .unwrap();

        assert_eq!(api_key, "key-1");
        assert_eq!(
            *opened.lock().unwrap(),
            [format!("{}/vibe/sign-in/p1", server.uri())]
        );
        let requests = server.received_requests().await.unwrap();
        let body = |p: &str| -> serde_json::Value {
            let request = requests.iter().find(|r| r.url.path() == p).unwrap();
            serde_json::from_slice(&request.body).unwrap()
        };
        let verifier = body("/api/vibe/sign-in/p1/exchange")["code_verifier"]
            .as_str()
            .unwrap()
            .to_string();
        let expected_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(sha2::Sha256::digest(verifier.as_bytes()));
        assert_eq!(
            body("/api/vibe/sign-in")["code_challenge"],
            expected_challenge
        );
        assert_eq!(
            body("/api/vibe/sign-in/p1/exchange")["exchange_token"],
            "t1"
        );
    }

    #[tokio::test]
    async fn denied_sign_in_fails_without_exchanging() {
        let server = sign_in_server(None).await;
        mock_poll(&server, json!({"status": "denied"}), None).await;

        let error = sign_in_against(&server).run(|_| {}).await.unwrap_err();

        assert_eq!(error.to_string(), "sign-in was denied");
    }

    #[tokio::test]
    async fn ineligible_workspace_is_explained() {
        let server = sign_in_server(None).await;
        mock_poll(
            &server,
            json!({"status": "completed", "exchange_token": "t1"}),
            None,
        )
        .await;
        Mock::given(method("POST"))
            .and(path("/api/vibe/sign-in/p1/exchange"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;

        let error = sign_in_against(&server).run(|_| {}).await.unwrap_err();

        assert_eq!(error.to_string(), WORKSPACE_NOT_ELIGIBLE);
    }

    #[tokio::test]
    async fn foreign_sign_in_url_is_never_opened() {
        let server = sign_in_server(Some("https://attacker.example/login".to_string())).await;
        let opened = Mutex::new(false);

        let result = sign_in_against(&server)
            .run(|_| *opened.lock().unwrap() = true)
            .await;

        assert!(result.is_err());
        assert!(!*opened.lock().unwrap());
    }
}
