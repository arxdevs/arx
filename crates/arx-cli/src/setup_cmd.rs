use anyhow::{Context, Result, bail};
use axum::Router;
use axum::extract::{Query, State};
use axum::response::Html;
use axum::routing::get;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

const LOOPBACK_PORT_DEFAULT: u16 = 7919;

pub struct SetupContext {
    pub server_url: String,
    pub credentials_path: PathBuf,
    pub quiet: bool,
    pub headless: bool,
    pub no_browser: bool,
    pub public_ip: Option<String>,
    pub root_domain: Option<String>,
    pub admin_domain_override: Option<String>,
    pub acme_email: Option<String>,
}

pub async fn run(ctx: SetupContext) -> Result<()> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    if !ctx.quiet {
        eprintln!("[1/5] checking daemon at {}", ctx.server_url);
    }
    if !is_alive(&http, &ctx.server_url).await {
        if !ctx.quiet {
            eprintln!("    daemon not running — installing via docker compose");
        }
        crate::server_cmd::install(ctx.quiet)
            .await
            .context("daemon install")?;
    }
    wait_for_daemon(&http, &ctx.server_url).await?;

    if !ctx.quiet {
        eprintln!("[2/5] checking setup state");
    }
    let status: Value = http
        .get(format!("{}/v1/setup-status", ctx.server_url))
        .send()
        .await
        .context("setup-status")?
        .json()
        .await?;
    let eligible = status["eligible"].as_bool().unwrap_or(false);

    let session_token: String;
    if eligible {
        if !ctx.quiet {
            eprintln!("[3/5] GitHub App manifest flow (first-time setup)");
        }
        let creds = manifest_flow(&ctx, &http).await?;
        session_token = finalize_install(&ctx, &http, creds).await?;
    } else {
        if !ctx.quiet {
            eprintln!("    daemon already configured — skipping GitHub App step");
        }
        let entries = crate::load_credentials(&ctx.credentials_path)?;
        let entry = entries
            .iter()
            .find(|e| e.url == ctx.server_url)
            .or_else(|| entries.first())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no credentials for {} — run `arx login --server {}` first",
                    ctx.server_url,
                    ctx.server_url
                )
            })?;
        session_token = entry.token.clone();

        let existing_settings = fetch_settings(&http, &ctx.server_url, &session_token)
            .await
            .ok();
        return run_settings_wizard(&ctx, &http, &session_token, existing_settings).await;
    }

    let root_domain = ctx.root_domain.clone().or_else(|| {
        prompt("Root domain (blank to skip, e.g. me.com): ")
            .ok()
            .filter(|s| !s.is_empty())
    });

    let admin_domain = if let Some(root) = root_domain.as_deref() {
        let default_admin = format!("arx.{root}");
        let admin = ctx.admin_domain_override.clone().or_else(|| {
            prompt(&format!("arx admin domain [{default_admin}]: "))
                .ok()
                .map(|s| {
                    if s.is_empty() {
                        default_admin.clone()
                    } else {
                        s
                    }
                })
        });
        if !ctx.quiet {
            eprintln!(
                "[4/5] root domain `{root}`, admin domain `{}`\n      add DNS:  *.{root}  A  <your-public-ip>",
                admin.as_deref().unwrap_or(&default_admin)
            );
        }
        admin
    } else {
        if !ctx.quiet {
            eprintln!(
                "[4/5] root domain — skipped (no public webhooks / external access until set)"
            );
        }
        None
    };

    let acme_email = ctx.acme_email.clone().or_else(|| {
        prompt("ACME email for Let's Encrypt expiry notices (blank to skip): ")
            .ok()
            .filter(|s| !s.is_empty())
    });

    if !ctx.quiet {
        eprintln!(
            "[5/5] ACME email — {}",
            acme_email.as_deref().unwrap_or("(skipped)")
        );
    }

    if admin_domain.is_some() || ctx.public_ip.is_some() || acme_email.is_some() {
        let patch = json!({
            "admin_domain": admin_domain,
            "acme_email": acme_email,
            "public_ip": ctx.public_ip,
        });
        let _ = http
            .patch(format!("{}/v1/server/settings", ctx.server_url))
            .bearer_auth(&session_token)
            .json(&patch)
            .send()
            .await?
            .error_for_status();
    }

    if !ctx.quiet {
        eprintln!();
        eprintln!("✓ setup complete.");
        eprintln!();
        eprintln!(
            "  arx is now accessible. Credentials saved to {}",
            ctx.credentials_path.display()
        );
        eprintln!();
        eprintln!("  ⚠ security note: this daemon has access to the Docker socket. Anyone with");
        eprintln!("    valid arx credentials can run containers and access host filesystem via");
        eprintln!("    mounted volumes. Keep arx credentials secret. Treat arx admin = host root.");
    }

    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ManifestResult {
    id: i64,
    slug: String,
    name: String,
    client_id: String,
    client_secret: String,
    webhook_secret: String,
    pem: String,
    html_url: String,
    owner: GhOwner,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct GhOwner {
    id: i64,
    login: String,
    avatar_url: Option<String>,
}

async fn manifest_flow(ctx: &SetupContext, http: &reqwest::Client) -> Result<ManifestResult> {
    let public_url = if let Some(domain) = ctx.admin_domain_override.as_deref() {
        format!("https://{domain}")
    } else if let Some(root) = ctx.root_domain.as_deref() {
        format!("https://arx.{root}")
    } else {
        "https://arx-pending.example.com".to_string()
    };

    let redirect_url = format!("http://127.0.0.1:{LOOPBACK_PORT_DEFAULT}/callback");

    let suffix: String = {
        use rand::RngCore;
        let mut b = [0u8; 3];
        rand::rngs::OsRng.fill_bytes(&mut b);
        b.iter().map(|x| format!("{:02x}", x)).collect()
    };

    let manifest = json!({
        "name": format!(
            "arx-{user}-{suffix}",
            user = std::env::var("USER").unwrap_or_else(|_| "self-hosted".into())
        ),
        "url": public_url,
        "hook_attributes": {
            "url": format!("{public_url}/v1/webhooks/github"),
            "active": true,
        },
        "redirect_url": redirect_url.clone(),
        "callback_urls": [format!("{public_url}/v1/auth/github/callback")],
        "public": true,


        "default_permissions": {
            "contents": "read",
            "metadata": "read",
            "pull_requests": "read",
            "statuses": "write",
            "deployments": "write",
        },
        "default_events": ["push", "pull_request", "release"],
    });

    let result_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let shutdown_tx_slot: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>> =
        Arc::new(Mutex::new(None));

    let app_ctx = AppCtxImpl {
        result: result_slot.clone(),
        manifest_json: serde_json::to_string(&manifest)?,
    };

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    *shutdown_tx_slot.lock().await = Some(shutdown_tx);

    let router = Router::new()
        .route("/", get(home))
        .route("/callback", get(callback))
        .with_state(app_ctx);

    let addr = SocketAddr::from(([127, 0, 0, 1], LOOPBACK_PORT_DEFAULT));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            bail!(
                "port {LOOPBACK_PORT_DEFAULT} is already in use on 127.0.0.1.\n\
                 \n\
                 Another `arx setup` may still be running, or another process is bound to it.\n\
                 Find it with `lsof -iTCP:{LOOPBACK_PORT_DEFAULT} -sTCP:LISTEN`, stop it, and re-run `arx setup`."
            );
        }
        Err(e) => return Err(anyhow::Error::new(e).context(format!("bind {addr}"))),
    };

    let server_task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
    });

    let start_url = format!("http://127.0.0.1:{LOOPBACK_PORT_DEFAULT}/");

    if ctx.headless || ctx.no_browser {
        eprintln!();
        eprintln!("Headless / no-browser mode. Open this URL on a machine WITH a browser:");
        eprintln!();
        eprintln!("    {start_url}");
        eprintln!();
        eprintln!("If you're SSH'd in, you can use port forwarding instead:");
        eprintln!(
            "    ssh -L {LOOPBACK_PORT_DEFAULT}:127.0.0.1:{LOOPBACK_PORT_DEFAULT} <your-server>"
        );
        eprintln!();
        eprintln!("After GitHub redirects, you may see an 'unable to connect' page. Copy the");
        eprintln!("`code` query parameter from the URL bar and paste it here:");
        eprintln!();
        eprint!("    code: ");
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        let code = buf.trim().to_string();
        if code.is_empty() {
            bail!("no code provided");
        }
        let creds = exchange_code(http, &code).await?;
        shutdown_tx_slot.lock().await.take().map(|s| s.send(()));
        let _ = server_task.await;
        return Ok(creds);
    }

    let _ = open_browser(&start_url);
    eprintln!("Opened {start_url} in your browser. Waiting for completion...");

    let code = loop {
        if let Some(c) = result_slot.lock().await.clone() {
            break c;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    };

    let creds = exchange_code(http, &code).await?;
    shutdown_tx_slot.lock().await.take().map(|s| s.send(()));
    let _ = server_task.await;
    Ok(creds)
}

async fn home(State(ctx): State<AppCtxImpl>) -> Html<String> {
    let manifest = ctx.manifest_json.clone();
    let escaped = html_attr_escape(&manifest);

    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>arx — redirecting…</title></head>\
<body style=\"font-family: sans-serif; text-align: center; margin-top: 4em\">\
<p>Redirecting to GitHub…</p>\
<form id=\"f\" action=\"https://github.com/settings/apps/new\" method=\"post\">\
<input type=\"hidden\" name=\"manifest\" value='{escaped}'>\
<noscript><button type=\"submit\">Continue to GitHub</button></noscript>\
</form>\
<script>document.getElementById('f').submit()</script>\
</body></html>"
    );
    Html(body)
}

async fn callback(Query(q): Query<CallbackQuery>, State(ctx): State<AppCtxImpl>) -> Html<String> {
    *ctx.result.lock().await = Some(q.code);
    Html(
        "<!doctype html><html><body style=\"font-family: sans-serif; text-align: center; margin-top: 4em\">\
<h1>✓ arx received the App credentials.</h1>\
<p>You can close this tab and return to the terminal.</p>\
</body></html>".to_string()
    )
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: String,
}

#[derive(Clone)]
struct AppCtxImpl {
    result: Arc<Mutex<Option<String>>>,
    manifest_json: String,
}

fn html_attr_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

async fn exchange_code(http: &reqwest::Client, code: &str) -> Result<ManifestResult> {
    let url = format!("https://api.github.com/app-manifests/{code}/conversions");
    let resp = http
        .post(&url)
        .header("user-agent", "arx-cli")
        .header("accept", "application/vnd.github+json")
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!(
            "GitHub manifest exchange failed: {} — {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
    }
    Ok(resp.json().await?)
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
    let status = std::process::Command::new(cmd)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if !status.success() {
        bail!("browser-opener failed");
    }
    Ok(())
}

async fn is_alive(http: &reqwest::Client, server_url: &str) -> bool {
    http.get(format!("{server_url}/health"))
        .timeout(Duration::from_secs(1))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .is_ok()
}

async fn finalize_install(
    ctx: &SetupContext,
    http: &reqwest::Client,
    creds: ManifestResult,
) -> Result<String> {
    let payload = json!({
        "app": {
            "id": creds.id,
            "slug": creds.slug,
            "name": creds.name,
            "client_id": creds.client_id,
            "client_secret": creds.client_secret,
            "webhook_secret": creds.webhook_secret,
            "pem": creds.pem,
            "html_url": creds.html_url,
        },
        "owner": {
            "github_user_id": creds.owner.id,
            "github_login": creds.owner.login,
            "display_name": creds.owner.login,
            "avatar_url": creds.owner.avatar_url,
        },
    });

    let resp: Value = http
        .post(format!("{}/v1/setup/github-app/install", ctx.server_url))
        .json(&payload)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let session_token = resp["session_token"]
        .as_str()
        .context("missing session_token in setup install response")?
        .to_string();

    crate::upsert_and_save(&ctx.credentials_path, &ctx.server_url, &session_token)?;

    if !ctx.quiet {
        eprintln!(
            "    ✓ user {} registered, workspace `{}` created",
            resp["user"]["github_login"].as_str().unwrap_or("?"),
            resp["workspace"]["slug"].as_str().unwrap_or("?")
        );
    }
    Ok(session_token)
}

#[derive(Deserialize, Default)]
struct ExistingSettings {
    admin_domain: Option<String>,
    acme_email: Option<String>,
}

async fn fetch_settings(
    http: &reqwest::Client,
    server_url: &str,
    token: &str,
) -> Result<ExistingSettings> {
    let mut r: ExistingSettings = http
        .get(format!("{server_url}/v1/server/settings"))
        .bearer_auth(token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    r.admin_domain = r.admin_domain.filter(|s| !s.is_empty());
    r.acme_email = r.acme_email.filter(|s| !s.is_empty());
    Ok(r)
}

async fn run_settings_wizard(
    ctx: &SetupContext,
    http: &reqwest::Client,
    session_token: &str,
    existing: Option<ExistingSettings>,
) -> Result<()> {
    let existing = existing.unwrap_or_default();

    let admin_domain = if let Some(d) = ctx.admin_domain_override.clone() {
        Some(d)
    } else if let Some(root) = ctx.root_domain.as_deref() {
        Some(format!("arx.{root}"))
    } else {
        let label = match existing.admin_domain.as_deref() {
            Some(curr) => format!("arx admin domain [{curr}, blank to keep]: "),
            None => "arx admin domain (blank to skip, e.g. arx.me.com): ".to_string(),
        };
        let entered = prompt(&label).ok().unwrap_or_default();
        if entered.is_empty() {
            existing.admin_domain.clone()
        } else {
            Some(entered)
        }
    };
    if !ctx.quiet {
        eprintln!(
            "[4/5] admin domain — {}",
            admin_domain.as_deref().unwrap_or("(skipped)")
        );
    }

    let acme_email = if let Some(e) = ctx.acme_email.clone() {
        Some(e)
    } else {
        let label = match existing.acme_email.as_deref() {
            Some(curr) => format!("ACME email [{curr}, blank to keep]: "),
            None => "ACME email (blank to skip): ".to_string(),
        };
        let entered = prompt(&label).ok().unwrap_or_default();
        if entered.is_empty() {
            existing.acme_email.clone()
        } else {
            Some(entered)
        }
    };
    if !ctx.quiet {
        eprintln!(
            "[5/5] ACME email — {}",
            acme_email.as_deref().unwrap_or("(skipped)")
        );
    }

    let patch = json!({
        "admin_domain": admin_domain,
        "acme_email": acme_email,
        "public_ip": ctx.public_ip,
    });
    let _ = http
        .patch(format!("{}/v1/server/settings", ctx.server_url))
        .bearer_auth(session_token)
        .json(&patch)
        .send()
        .await?
        .error_for_status();

    if !ctx.quiet {
        eprintln!("\n✓ settings updated.");
    }
    Ok(())
}

fn prompt(label: &str) -> Result<String> {
    use std::io::Write;
    eprint!("{label}");
    std::io::stderr().flush()?;
    let mut s = String::new();
    std::io::stdin().read_line(&mut s)?;
    Ok(s.trim().to_string())
}

async fn wait_for_daemon(http: &reqwest::Client, server_url: &str) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        match http
            .get(format!("{server_url}/health"))
            .send()
            .await
            .and_then(|r| r.error_for_status())
        {
            Ok(_) => return Ok(()),
            Err(_) if std::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(e) => bail!("daemon at {server_url} not reachable: {e}"),
        }
    }
}
