//! Cliente mínimo de la API de Mensajes de Claude por HTTP (no hay SDK oficial de Rust).
//! Pide salida estructurada (JSON schema) y activa los respaldos del servidor por defecto.

use std::time::Duration;

use serde_json::{Value, json};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2026-10-21";
/// `fallbacks: "default"`: si un filtro de seguridad declina la petición, la API la reintenta en
/// el modelo que Anthropic recomienda para esa categoría, dentro de la misma llamada.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const TIMEOUT: Duration = Duration::from_secs(90);

pub struct Claude {
    http: reqwest::Client,
    api_key: String,
    pub model: String,
    pub effort: String,
}

pub struct StructuredReply {
    pub json: Value,
    pub model: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
}

#[derive(Debug, thiserror::Error)]
pub enum ClaudeError {
    #[error("no se pudo hablar con la API de Claude: {0}")]
    Http(#[from] reqwest::Error),
    #[error("la API de Claude respondió {status}: {message}")]
    Api { status: u16, message: String },
    #[error("la IA declinó la petición")]
    Refusal,
    #[error("la respuesta de la IA quedó incompleta")]
    Truncated,
    #[error("la respuesta de la IA no trae el JSON esperado")]
    InvalidJson,
}

impl Claude {
    pub fn new(api_key: String, model: String, effort: String) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder().timeout(TIMEOUT).build()?;
        Ok(Self { http, api_key, model, effort })
    }

    /// Una sola petición cuya respuesta debe cumplir `schema`.
    pub async fn structured(
        &self,
        system: &str,
        content: Value,
        schema: Value,
        max_tokens: u32,
    ) -> Result<StructuredReply, ClaudeError> {
        let body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "system": system,
            "fallbacks": "default",
            "output_config": {
                "effort": self.effort,
                "format": { "type": "json_schema", "schema": schema },
            },
            "messages": [{ "role": "user", "content": content }],
        });

        let response = self
            .http
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", FALLBACK_BETA)
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        let value: Value = response.json().await?;
        if !status.is_success() {
            let message = value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("sin detalle")
                .to_owned();
            return Err(ClaudeError::Api { status: status.as_u16(), message });
        }
        parse_reply(&value, &self.model)
    }
}

/// Revisa `stop_reason` antes de leer el contenido y junta los bloques de texto en un JSON.
fn parse_reply(value: &Value, requested_model: &str) -> Result<StructuredReply, ClaudeError> {
    match value["stop_reason"].as_str() {
        Some("refusal") => return Err(ClaudeError::Refusal),
        Some("max_tokens") => return Err(ClaudeError::Truncated),
        _ => {}
    }
    let text: String = value["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect();
    let json = serde_json::from_str(&text).map_err(|_| ClaudeError::InvalidJson)?;
    let usage = |field: &str| {
        value
            .pointer(&format!("/usage/{field}"))
            .and_then(Value::as_i64)
            .map_or(0, |n| i32::try_from(n).unwrap_or(i32::MAX))
    };
    Ok(StructuredReply {
        json,
        model: value["model"].as_str().unwrap_or(requested_model).to_owned(),
        input_tokens: usage("input_tokens"),
        output_tokens: usage("output_tokens"),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn reads_json_from_text_blocks_and_skips_thinking() {
        let reply = parse_reply(
            &json!({
                "model": "claude-opus-5-5",
                "stop_reason": "end_turn",
                "content": [
                    { "type": "thinking", "thinking": "" },
                    { "type": "text", "text": "{\"hypotheses\": []}" }
                ],
                "usage": { "input_tokens": 1500, "output_tokens": 300 }
            }),
            "claude-opus-5-5",
        )
        .unwrap();
        assert_eq!(reply.json, json!({ "hypotheses": [] }));
        assert_eq!((reply.input_tokens, reply.output_tokens), (1500, 300));
    }

    #[test]
    fn refusal_and_truncation_are_errors_not_content() {
        let refusal = json!({ "stop_reason": "refusal", "content": [] });
        assert!(matches!(parse_reply(&refusal, "m"), Err(ClaudeError::Refusal)));
        let cut = json!({ "stop_reason": "max_tokens", "content": [{ "type": "text", "text": "{" }] });
        assert!(matches!(parse_reply(&cut, "m"), Err(ClaudeError::Truncated)));
    }
}
