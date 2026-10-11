use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::env;
use thiserror::Error;

const MAX_COMMENT_CHARS: usize = 2_000;

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PublicMarketContext {
    pub market: String,
    pub interval: String,
    pub last_price: String,
    pub last_signal: String,
    pub feed_state: String,
    pub candle_close_time_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdvisoryComment {
    pub provider: &'static str,
    pub model: String,
    pub text: String,
}

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("variável de ambiente ausente ou vazia: {0}")]
    MissingEnvironment(&'static str),
    #[error("URL do modelo inválida ou insegura")]
    InvalidBaseUrl,
    #[error("nome de modelo inválido")]
    InvalidModel,
    #[error("chave de API inválida")]
    InvalidApiKey,
    #[error("falha de rede ao consultar adaptador")]
    Request(#[from] reqwest::Error),
    #[error("provedor retornou status HTTP {0}")]
    HttpStatus(reqwest::StatusCode),
    #[error("resposta do provedor não corresponde ao contrato de chat completion")]
    InvalidResponse,
    #[error("provedor retornou comentário vazio")]
    EmptyComment,
}

/// Adaptador sem ferramentas, com entrada tipada limitada a contexto público do mercado.
/// Não depende de ode-risk ou ode-execution e seu resultado nunca é executável pelo bot.
pub struct OpenAiCompatibleAdapter {
    client: Client,
    endpoint: Url,
    model: String,
    api_key: String,
}

impl OpenAiCompatibleAdapter {
    pub fn from_env() -> Result<Self, AdapterError> {
        let api_key = env::var("ODE_MODEL_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or(AdapterError::MissingEnvironment("ODE_MODEL_API_KEY"))?;
        let base_url = env::var("ODE_MODEL_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_owned());
        let model = env::var("ODE_MODEL_NAME").unwrap_or_else(|_| "gpt-4o-mini".to_owned());
        Self::new(&base_url, &model, &api_key)
    }

    pub fn new(base_url: &str, model: &str, api_key: &str) -> Result<Self, AdapterError> {
        let mut base = Url::parse(base_url).map_err(|_| AdapterError::InvalidBaseUrl)?;
        let host = base.host_str().unwrap_or_default();
        let local_http =
            base.scheme() == "http" && matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1");
        if (base.scheme() != "https" && !local_http)
            || base.username() != ""
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(AdapterError::InvalidBaseUrl);
        }
        let path = base.path().trim_end_matches('/');
        base.set_path(&format!("{path}/"));
        let endpoint = base
            .join("chat/completions")
            .map_err(|_| AdapterError::InvalidBaseUrl)?;
        if model.trim().is_empty()
            || model.len() > 128
            || model
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err(AdapterError::InvalidModel);
        }
        if api_key.trim().is_empty()
            || api_key.len() > 8_192
            || api_key
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err(AdapterError::InvalidApiKey);
        }
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(25))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()?;
        Ok(Self {
            client,
            endpoint,
            model: model.to_owned(),
            api_key: api_key.to_owned(),
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub async fn analyze(
        &self,
        context: &PublicMarketContext,
    ) -> Result<AdvisoryComment, AdapterError> {
        let response = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(&self.api_key)
            .json(&request_body(&self.model, context))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(AdapterError::HttpStatus(response.status()));
        }
        let completion: CompletionResponse = response
            .json()
            .await
            .map_err(|_| AdapterError::InvalidResponse)?;
        let raw = completion
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
            .ok_or(AdapterError::InvalidResponse)?;
        let text = sanitize_comment(raw)?;
        Ok(AdvisoryComment {
            provider: "openai-compatible",
            model: self.model.clone(),
            text,
        })
    }
}

fn sanitize_comment(raw: &str) -> Result<String, AdapterError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(AdapterError::EmptyComment);
    }
    Ok(raw.chars().take(MAX_COMMENT_CHARS).collect())
}

fn request_body(model: &str, context: &PublicMarketContext) -> Value {
    let public_context = serde_json::to_string(context).expect("public market context serializes");
    json!({
        "model": model,
        "temperature": 0,
        "max_tokens": 220,
        "messages": [
            {
                "role": "system",
                "content": "Você é um comentarista de mercado somente para leitura. Use apenas o snapshot público recebido, declare incerteza e seja conciso. Não recomende compra/venda, não defina tamanho de posição, não solicite credenciais e não instrua execução ou alteração de risco. Sua resposta é texto consultivo não vinculante; o sistema não a executa."
            },
            {
                "role": "user",
                "content": public_context
            }
        ]
    })
}

#[derive(Debug, Deserialize)]
struct CompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Debug, Deserialize)]
struct Message {
    content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> PublicMarketContext {
        PublicMarketContext {
            market: "BTCUSDT".into(),
            interval: "1m".into(),
            last_price: "65000.12".into(),
            last_signal: "HOLD".into(),
            feed_state: "healthy".into(),
            candle_close_time_ms: Some(1_700_000_000_000),
        }
    }

    #[test]
    fn external_plain_http_is_rejected_but_loopback_is_allowed() {
        assert!(matches!(
            OpenAiCompatibleAdapter::new("http://example.com/v1", "model", "secret"),
            Err(AdapterError::InvalidBaseUrl)
        ));
        assert!(
            OpenAiCompatibleAdapter::new("http://127.0.0.1:1234/v1", "model", "secret").is_ok()
        );
        assert!(
            OpenAiCompatibleAdapter::new("https://provider.example/v1", "model", "secret").is_ok()
        );
    }

    #[test]
    fn request_contains_only_public_market_context_and_no_tools() {
        let body = request_body("model", &context());
        assert!(body.get("tools").is_none());
        assert!(
            body["messages"][1]["content"]
                .as_str()
                .unwrap()
                .contains("BTCUSDT")
        );
        assert!(!body.to_string().contains("portfolio"));
        assert!(!body.to_string().contains("api_key"));
    }

    #[test]
    fn output_is_bounded_and_trimmed() {
        let long = format!("  {}  ", "x".repeat(MAX_COMMENT_CHARS + 20));
        let bounded = sanitize_comment(&long).unwrap();
        assert_eq!(bounded.len(), MAX_COMMENT_CHARS);
        assert!(matches!(
            sanitize_comment(" \n "),
            Err(AdapterError::EmptyComment)
        ));
    }
}
