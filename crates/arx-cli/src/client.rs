use crate::error::CliError;
use anyhow::Result;
use serde_json::Value;

pub(crate) struct Client {
    pub server: String,
    pub token: Option<String>,
    pub http: reqwest::Client,
}

impl Client {
    pub(crate) fn new(server: String, token: Option<String>) -> Self {
        Self {
            server,
            token,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("reqwest client builder with static config should not fail"),
        }
    }

    pub(crate) async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Option<Value>> {
        let url = format!("{}{}", self.server.trim_end_matches('/'), path);
        let mut req = self.http.request(method, &url);
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let res = req
            .send()
            .await
            .map_err(|e| CliError::Network(e.to_string()))?;
        let status = res.status();
        let text = res
            .text()
            .await
            .map_err(|e| CliError::Network(e.to_string()))?;

        if status.is_success() {
            if text.trim().is_empty() {
                return Ok(None);
            }
            let v: Value = serde_json::from_str(&text)
                .map_err(|e| CliError::Server(format!("decode body: {e} (raw: {text})")))?;
            return Ok(Some(v));
        }

        let parsed: Option<Value> = serde_json::from_str(&text).ok();
        let message = parsed
            .as_ref()
            .and_then(|v| v.get("message").and_then(|m| m.as_str()))
            .map(|s| s.to_string())
            .unwrap_or_else(|| text.clone());

        let err = match status.as_u16() {
            401 => CliError::Unauthorized,
            403 => CliError::Unauthorized,
            404 => CliError::NotFound(message),
            409 => CliError::AlreadyExists(message),
            400 => CliError::BadRequest(message),
            500..=599 => CliError::Server(message),
            other => CliError::Server(format!("status {other}: {message}")),
        };
        Err(err.into())
    }
}

pub(crate) fn print_value(v: &Value, _json: bool) {
    println!(
        "{}",
        serde_json::to_string_pretty(v).expect("Value is always serializable")
    );
}

pub(crate) fn push_delete_query(path: &mut String, force: bool, with_data: bool) {
    let mut parts: Vec<&str> = Vec::new();
    if force {
        parts.push("force=true");
    }
    if with_data {
        parts.push("with_data=true");
    }
    if !parts.is_empty() {
        path.push('?');
        path.push_str(&parts.join("&"));
    }
}
