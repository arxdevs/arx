use anyhow::{Context, Result, bail};
use axum::Router;
use axum::extract::{Query, State};
use axum::response::Html;
use axum::routing::get;
use serde::Deserialize;
use serde_json::{Value, json};
use std::net::{SocketAddr, TcpListener as StdListener};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

pub struct LoginContext {
    pub server_url: String,
    pub credentials_path: PathBuf,
    pub quiet: bool,
    pub device: bool,
}

pub async fn run(ctx: LoginContext) -> Result<()> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;

    let info: Value = http
        .get(format!("{}/v1/auth/github/oauth-info", ctx.server_url))
        .send()
        .await
        .context("contact arx-server")?
        .error_for_status()
        .context("/v1/auth/github/oauth-info")?
        .json()
        .await?;
    let client_id = info["client_id"]
        .as_str()
        .context("missing client_id in oauth-info")?
        .to_string();

    let session_token = if ctx.device {
        device_flow(&http, &ctx.server_url, &client_id, ctx.quiet).await?
    } else {
        loopback_flow(&http, &ctx.server_url, &client_id, ctx.quiet).await?
    };

    crate::upsert_and_save(&ctx.credentials_path, &ctx.server_url, &session_token)?;
    if !ctx.quiet {
        eprintln!(
            "✓ logged in. credentials saved to {}",
            ctx.credentials_path.display()
        );
    }
    Ok(())
}

async fn loopback_flow(
    http: &reqwest::Client,
    server_url: &str,
    client_id: &str,
    quiet: bool,
) -> Result<String> {
    let std_listener =
        StdListener::bind("127.0.0.1:0").context("bind loopback listener for OAuth")?;
    std_listener
        .set_nonblocking(true)
        .context("nonblocking listener")?;
    let local_addr: SocketAddr = std_listener.local_addr()?;
    let port = local_addr.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let state_token = random_state();
    let scope = "read:user user:email";
    let authorize_url = format!(
        "https://github.com/login/oauth/authorize?client_id={client_id}\
         &redirect_uri={redirect}&state={state}&scope={scope}",
        redirect = urlencoding(&redirect_uri),
        state = urlencoding(&state_token),
        scope = urlencoding(scope),
    );

    let result_slot: Arc<Mutex<Option<CallbackPayload>>> = Arc::new(Mutex::new(None));
    let app_ctx = LoopbackCtx {
        result: result_slot.clone(),
        expected_state: state_token.clone(),
    };

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let app = Router::new()
        .route("/callback", get(callback))
        .with_state(app_ctx);

    let tokio_listener = tokio::net::TcpListener::from_std(std_listener)?;
    let server_task = tokio::spawn(async move {
        axum::serve(tokio_listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
    });

    if !quiet {
        eprintln!("Opening browser for GitHub login...");
        eprintln!("    {authorize_url}");
    }
    let _ = open_browser(&authorize_url);

    let deadline = std::time::Instant::now() + Duration::from_secs(300);
    let payload = loop {
        if let Some(p) = result_slot.lock().await.clone() {
            break p;
        }
        if std::time::Instant::now() >= deadline {
            let _ = shutdown_tx.send(());
            bail!("timeout waiting for OAuth callback");
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    };

    let _ = shutdown_tx.send(());
    let _ = server_task.await;

    let resp: Value = http
        .post(format!("{server_url}/v1/auth/github/exchange"))
        .json(&json!({ "code": payload.code }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(resp["session_token"]
        .as_str()
        .context("missing session_token")?
        .to_string())
}

async fn device_flow(
    http: &reqwest::Client,
    server_url: &str,
    client_id: &str,
    quiet: bool,
) -> Result<String> {
    let init: Value = http
        .post("https://github.com/login/device/code")
        .header("accept", "application/json")
        .form(&[("client_id", client_id), ("scope", "read:user user:email")])
        .send()
        .await
        .context("github /device/code")?
        .error_for_status()?
        .json()
        .await?;

    let device_code = init["device_code"]
        .as_str()
        .context("missing device_code")?
        .to_string();
    let user_code = init["user_code"].as_str().context("missing user_code")?;
    let verification_uri = init["verification_uri"]
        .as_str()
        .unwrap_or("https://github.com/login/device");
    let interval_secs = init["interval"].as_u64().unwrap_or(5);
    let expires_in = init["expires_in"].as_u64().unwrap_or(900);

    if !quiet {
        eprintln!();
        eprintln!("Open this URL in any browser:");
        eprintln!("    {verification_uri}");
        eprintln!();
        eprintln!("Then enter this code:");
        eprintln!("    {user_code}");
        eprintln!();
        eprintln!("Waiting for completion (Ctrl+C to cancel)...");
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(expires_in);
    let mut interval = Duration::from_secs(interval_secs.max(5));
    loop {
        tokio::time::sleep(interval).await;
        if std::time::Instant::now() >= deadline {
            bail!("device code expired");
        }
        let r: Value = http
            .post("https://github.com/login/oauth/access_token")
            .header("accept", "application/json")
            .form(&[
                ("client_id", client_id),
                ("device_code", device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await?
            .json()
            .await?;
        if let Some(token) = r["access_token"].as_str() {
            let resp: Value = http
                .post(format!("{server_url}/v1/auth/github/device"))
                .json(&json!({ "access_token": token }))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            return Ok(resp["session_token"]
                .as_str()
                .context("missing session_token")?
                .to_string());
        }
        if let Some(err) = r["error"].as_str() {
            match err {
                "authorization_pending" => continue,
                "slow_down" => {
                    interval = Duration::from_secs(interval.as_secs() + 5);
                    continue;
                }
                "expired_token" => bail!("device code expired"),
                "access_denied" => bail!("authorization denied by user"),
                other => bail!("github device flow error: {other}"),
            }
        }
    }
}

#[derive(Clone)]
struct LoopbackCtx {
    result: Arc<Mutex<Option<CallbackPayload>>>,
    expected_state: String,
}

#[derive(Clone, Debug)]
struct CallbackPayload {
    code: String,
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn callback(Query(q): Query<CallbackQuery>, State(ctx): State<LoopbackCtx>) -> Html<String> {
    if let Some(err) = q.error {
        return Html(format!(
            "<html><body style=\"font-family: sans-serif; text-align: center; margin-top: 4em\">\
             <h1>OAuth error</h1><p>{err}: {desc}</p></body></html>",
            desc = q.error_description.unwrap_or_default()
        ));
    }
    if q.state.as_deref() != Some(ctx.expected_state.as_str()) {
        return Html(
            "<html><body><h1>OAuth state mismatch.</h1><p>Possible CSRF; aborting.</p></body></html>".into(),
        );
    }
    let Some(code) = q.code else {
        return Html("<html><body><h1>missing code</h1></body></html>".into());
    };
    *ctx.result.lock().await = Some(CallbackPayload { code });
    Html(
        "<!doctype html><html><body style=\"font-family: sans-serif; text-align: center; margin-top: 4em\">\
         <h1>✓ Signed in.</h1><p>You can close this tab and return to the terminal.</p></body></html>".into()
    )
}

fn random_state() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                c.to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect()
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let cmd = "";
    if cmd.is_empty() {
        bail!("no browser-opener known for this platform");
    }
    std::process::Command::new(cmd)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    Ok(())
}
