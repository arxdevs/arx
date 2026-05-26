use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct AppManifest {
    pub name: String,
    pub url: String,
    pub hook_attributes: HookAttributes,
    pub redirect_url: String,
    pub callback_urls: Vec<String>,
    pub public: bool,
    pub default_permissions: Permissions,
    pub default_events: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct HookAttributes {
    pub url: String,
    pub active: bool,
}

#[derive(Debug, Serialize)]
pub struct Permissions {
    pub contents: &'static str,
    pub metadata: &'static str,
    pub pull_requests: &'static str,
    pub statuses: &'static str,
    pub deployments: &'static str,
    pub members: &'static str,
    pub emails: &'static str,
}

impl AppManifest {
    pub fn for_arx(public_url: &str, instance_name: &str) -> Self {
        let public_url = public_url.trim_end_matches('/');
        Self {
            name: format!("arx ({instance_name})"),
            url: public_url.to_string(),
            hook_attributes: HookAttributes {
                url: format!("{public_url}/v1/webhooks/github"),
                active: true,
            },
            redirect_url: format!("{public_url}/v1/setup/github-app/callback"),
            callback_urls: vec![format!("{public_url}/v1/auth/github/callback")],
            public: false,
            default_permissions: Permissions {
                contents: "read",
                metadata: "read",
                pull_requests: "read",
                statuses: "write",
                deployments: "write",
                members: "read",
                emails: "read",
            },
            default_events: vec!["push", "pull_request", "release"],
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ManifestConversionResp {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub client_id: String,
    pub client_secret: String,
    pub webhook_secret: String,
    pub pem: String,
    pub html_url: String,
}

pub async fn exchange_manifest_code(
    http: &reqwest::Client,
    code: &str,
) -> Result<ManifestConversionResp, arx_core::Error> {
    let url = format!("https://api.github.com/app-manifests/{code}/conversions");
    let res = http
        .post(&url)
        .header("accept", "application/vnd.github+json")
        .header("user-agent", "arx")
        .send()
        .await
        .map_err(|e| arx_core::Error::Internal(format!("github manifest exchange: {e}")))?;
    if !res.status().is_success() {
        return Err(arx_core::Error::Internal(format!(
            "github manifest exchange returned {}",
            res.status()
        )));
    }
    let body: ManifestConversionResp = res
        .json()
        .await
        .map_err(|e| arx_core::Error::Internal(format!("decode manifest resp: {e}")))?;
    Ok(body)
}
