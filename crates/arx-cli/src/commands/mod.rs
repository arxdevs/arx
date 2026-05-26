use crate::cli::{
    BackupCmd, Cli, Command, ConfigCmd, DomainCmd, ProjectCmd, ServerCertCmd, ServerCmd,
    ServerConfigCmd, ServiceCmd, VarCmd, WorkspaceCmd,
};
use crate::client::{Client, print_value, push_delete_query};
use crate::credentials::{CredentialEntry, remove_credential, save_credentials, upsert_credential};
use crate::error::CliError;
use crate::{login_cmd, server_cmd, setup_cmd};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::PathBuf;

fn ws(opt: Option<&str>) -> Result<&str> {
    opt.ok_or_else(|| CliError::Usage("workspace not set (-w / ARX_WORKSPACE)".into()).into())
}

fn pr(opt: Option<&str>) -> Result<&str> {
    opt.ok_or_else(|| CliError::Usage("project not set (-p / ARX_PROJECT)".into()).into())
}

pub(crate) async fn dispatch(
    cli: Cli,
    server: String,
    token: Option<String>,
    mut creds: Vec<CredentialEntry>,
    cred_path: PathBuf,
    client: Client,
) -> Result<()> {
    match cli.cmd {
        Command::Setup {
            no_browser,
            headless,
            public_ip,
            root_domain,
            admin_domain,
            acme_email,
        } => {
            setup_cmd::run(setup_cmd::SetupContext {
                server_url: server.clone(),
                credentials_path: cred_path.clone(),
                quiet: cli.quiet,
                headless,
                no_browser,
                public_ip,
                root_domain,
                admin_domain_override: admin_domain,
                acme_email,
            })
            .await?;
        }
        Command::Login { device, token: tok } => {
            if let Some(t) = tok {
                upsert_credential(&mut creds, &server, &t);
                save_credentials(&cred_path, &creds)?;
                if !cli.quiet {
                    eprintln!("token stored at {}", cred_path.display());
                }
            } else {
                login_cmd::run(login_cmd::LoginContext {
                    server_url: server.clone(),
                    credentials_path: cred_path.clone(),
                    quiet: cli.quiet,
                    device,
                })
                .await?;
            }
        }
        Command::Logout => {
            if let Some(t) = &token {
                let cl = Client::new(client.server.clone(), Some(t.clone()));
                let _ = cl
                    .request(reqwest::Method::POST, "/v1/auth/logout", None)
                    .await;
            }
            remove_credential(&mut creds, &server);
            save_credentials(&cred_path, &creds)?;
            if !cli.quiet {
                eprintln!("logged out");
            }
        }
        Command::Whoami => {
            if token.is_none() {
                bail!(CliError::Unauthorized);
            }
            let v = client
                .request(reqwest::Method::GET, "/v1/auth/me", None)
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Workspace(WorkspaceCmd::List) => {
            let v = client
                .request(reqwest::Method::GET, "/v1/workspaces", None)
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Workspace(WorkspaceCmd::Delete {
            slug,
            force,
            with_data,
        }) => {
            let mut path = format!("/v1/workspaces/{slug}");
            push_delete_query(&mut path, force, with_data);
            client.request(reqwest::Method::DELETE, &path, None).await?;
            if !cli.quiet {
                eprintln!("deleted workspace {slug}");
            }
        }
        Command::Workspace(WorkspaceCmd::Create { slug, name }) => {
            let v = client
                .request(
                    reqwest::Method::POST,
                    "/v1/workspaces",
                    Some(json!({"slug": slug, "name": name})),
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Project(ProjectCmd::List) => {
            let w = ws(cli.workspace.as_deref())?;
            let v = client
                .request(
                    reqwest::Method::GET,
                    &format!("/v1/workspaces/{w}/projects"),
                    None,
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Project(ProjectCmd::Delete {
            slug,
            force,
            with_data,
        }) => {
            let w = ws(cli.workspace.as_deref())?;
            let mut path = format!("/v1/workspaces/{w}/projects/{slug}");
            push_delete_query(&mut path, force, with_data);
            client.request(reqwest::Method::DELETE, &path, None).await?;
            if !cli.quiet {
                eprintln!("deleted project {slug}");
            }
        }
        Command::Project(ProjectCmd::Create { slug, name }) => {
            let w = ws(cli.workspace.as_deref())?;
            let v = client
                .request(
                    reqwest::Method::POST,
                    &format!("/v1/workspaces/{w}/projects"),
                    Some(json!({"slug": slug, "name": name})),
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Service(ServiceCmd::List) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let v = client
                .request(
                    reqwest::Method::GET,
                    &format!("/v1/workspaces/{w}/projects/{p}/services"),
                    None,
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Service(ServiceCmd::Rename { slug, name }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let v = client
                .request(
                    reqwest::Method::PATCH,
                    &format!("/v1/workspaces/{w}/projects/{p}/services/{slug}"),
                    Some(json!({ "name": name })),
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Service(ServiceCmd::Delete {
            slug,
            force,
            with_data,
        }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let mut path = format!("/v1/workspaces/{w}/projects/{p}/services/{slug}");
            push_delete_query(&mut path, force, with_data);
            client.request(reqwest::Method::DELETE, &path, None).await?;
            if !cli.quiet {
                eprintln!("deleted service {slug}");
            }
        }
        Command::Service(ServiceCmd::Show { slug }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let v = client
                .request(
                    reqwest::Method::GET,
                    &format!("/v1/workspaces/{w}/projects/{p}/services/{slug}"),
                    None,
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Service(ServiceCmd::Create {
            slug,
            name,
            kind,
            repo,
            branch,
            image,
            template,
            dockerfile,
            root_directory,
            build_command,
            start_command,
        }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let source = match kind.as_str() {
                "git" => {
                    let repo = repo
                        .ok_or_else(|| CliError::Usage("--repo is required for kind=git".into()))?;
                    json!({
                        "kind": "git_source",
                        "github_repo": repo,
                        "branch": branch,
                        "dockerfile": dockerfile,
                        "root_directory": root_directory,
                    })
                }
                "image" => {
                    let image = image.ok_or_else(|| {
                        CliError::Usage("--image is required for kind=image".into())
                    })?;
                    json!({
                        "kind": "docker_image",
                        "image": image,
                        "registry_credentials_id": null,
                    })
                }
                "db" => {
                    let template = template.ok_or_else(|| {
                        CliError::Usage("--template is required for kind=db".into())
                    })?;
                    json!({
                        "kind": "db_template",
                        "template": template,
                        "version": null,
                    })
                }
                _ => return Err(CliError::Usage(format!("unknown service kind: {kind}")).into()),
            };
            let mut body = json!({"slug": slug, "name": name, "source": source});
            if let Some(b) = build_command {
                body["build_command"] = Value::String(b);
            }
            if let Some(s) = start_command {
                body["start_command"] = Value::String(s);
            }
            let v = client
                .request(
                    reqwest::Method::POST,
                    &format!("/v1/workspaces/{w}/projects/{p}/services"),
                    Some(body),
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Service(ServiceCmd::Config { cmd }) => match cmd {
            crate::cli::ServiceConfigCmd::Set {
                slug,
                build_command,
                start_command,
            } => {
                let w = ws(cli.workspace.as_deref())?;
                let p = pr(cli.project.as_deref())?;
                let mut body = serde_json::Map::new();
                if let Some(b) = build_command {
                    body.insert(
                        "build_command".into(),
                        if b.is_empty() {
                            Value::Null
                        } else {
                            Value::String(b)
                        },
                    );
                }
                if let Some(s) = start_command {
                    body.insert(
                        "start_command".into(),
                        if s.is_empty() {
                            Value::Null
                        } else {
                            Value::String(s)
                        },
                    );
                }
                if body.is_empty() {
                    return Err(CliError::Usage(
                        "specify at least one of --build-cmd / --start-cmd".into(),
                    )
                    .into());
                }
                let v = client
                    .request(
                        reqwest::Method::PATCH,
                        &format!("/v1/workspaces/{w}/projects/{p}/services/{slug}"),
                        Some(Value::Object(body)),
                    )
                    .await?
                    .unwrap_or(Value::Null);
                print_value(&v, cli.json);
            }
        },
        Command::Var(VarCmd::List { service }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let env = cli.env.unwrap_or_else(|| "production".into());
            let v = client
                .request(
                    reqwest::Method::GET,
                    &format!(
                        "/v1/workspaces/{w}/projects/{p}/services/{service}/variables?env={env}"
                    ),
                    None,
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Var(VarCmd::Set {
            service,
            kv,
            sealed,
        }) => {
            let (k, v) = kv
                .split_once('=')
                .ok_or_else(|| CliError::Usage("KEY=VALUE expected".into()))?;
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let env = cli.env.unwrap_or_else(|| "production".into());
            client
                .request(
                    reqwest::Method::POST,
                    &format!("/v1/workspaces/{w}/projects/{p}/services/{service}/variables"),
                    Some(json!({"key": k, "value": v, "sealed": sealed, "env": env})),
                )
                .await?;
            if !cli.quiet {
                eprintln!(
                    "set {k} ({} chars){}",
                    v.len(),
                    if sealed { ", sealed" } else { "" }
                );
            }
        }
        Command::Var(VarCmd::Unset { service, key }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let env = cli.env.unwrap_or_else(|| "production".into());
            client
                .request(
                    reqwest::Method::DELETE,
                    &format!(
                        "/v1/workspaces/{w}/projects/{p}/services/{service}/variables/{key}?env={env}"
                    ),
                    None,
                )
                .await?;
            if !cli.quiet {
                eprintln!("unset {key}");
            }
        }
        Command::Var(VarCmd::Import {
            service,
            file,
            sealed_all,
            overwrite,
        }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let env = cli.env.clone().unwrap_or_else(|| "production".into());
            let body = std::fs::read_to_string(&file).context("read env file")?;

            let existing: Value = client
                .request(
                    reqwest::Method::GET,
                    &format!(
                        "/v1/workspaces/{w}/projects/{p}/services/{service}/variables?env={env}"
                    ),
                    None,
                )
                .await?
                .unwrap_or(Value::Null);
            let existing_keys: std::collections::HashSet<String> = existing
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.get("key").and_then(|k| k.as_str()).map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            let mut set_count = 0u32;
            let mut skipped = 0u32;
            for (lineno, raw_line) in body.lines().enumerate() {
                let line = raw_line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let Some((k, v)) = line.split_once('=') else {
                    eprintln!("line {}: skipped (no '=')", lineno + 1);
                    continue;
                };
                let key = k.trim().to_string();
                let value = v.trim().trim_matches('"').trim_matches('\'').to_string();
                if !overwrite && existing_keys.contains(&key) {
                    skipped += 1;
                    continue;
                }
                client
                    .request(
                        reqwest::Method::POST,
                        &format!("/v1/workspaces/{w}/projects/{p}/services/{service}/variables"),
                        Some(json!({
                            "key": key,
                            "value": value,
                            "sealed": sealed_all,
                            "env": env,
                        })),
                    )
                    .await?;
                set_count += 1;
            }
            if !cli.quiet {
                eprintln!("imported {set_count} keys (skipped {skipped})");
            }
        }
        Command::Domain(DomainCmd::List { service }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let env = cli.env.unwrap_or_else(|| "production".into());
            let v = client
                .request(
                    reqwest::Method::GET,
                    &format!(
                        "/v1/workspaces/{w}/projects/{p}/services/{service}/domains?env={env}"
                    ),
                    None,
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Domain(DomainCmd::Add { service, hostname }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let env = cli.env.unwrap_or_else(|| "production".into());
            let v = client
                .request(
                    reqwest::Method::POST,
                    &format!("/v1/workspaces/{w}/projects/{p}/services/{service}/domains"),
                    Some(json!({"hostname": hostname, "env": env})),
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Domain(DomainCmd::Remove { id }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;

            let domains = client
                .request(
                    reqwest::Method::GET,
                    &format!("/v1/workspaces/{w}/projects/{p}/services"),
                    None,
                )
                .await?
                .unwrap_or(Value::Null);
            let service_slug = find_service_slug_for_domain(&client, w, p, &domains, &id)
                .await?
                .ok_or_else(|| {
                    CliError::NotFound(format!("domain {id} not found in any service of {w}/{p}"))
                })?;
            client
                .request(
                    reqwest::Method::DELETE,
                    &format!(
                        "/v1/workspaces/{w}/projects/{p}/services/{service_slug}/domains/{id}"
                    ),
                    None,
                )
                .await?;
            if !cli.quiet {
                eprintln!("removed domain {id}");
            }
        }
        Command::Deploy { service } => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let env = cli.env.unwrap_or_else(|| "production".into());
            let v = client
                .request(
                    reqwest::Method::POST,
                    &format!("/v1/workspaces/{w}/projects/{p}/services/{service}/deploy"),
                    Some(json!({"env": env})),
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Rollback {
            service,
            deployment_id,
        } => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let env = cli.env.unwrap_or_else(|| "production".into());
            let v = client
                .request(
                    reqwest::Method::POST,
                    &format!("/v1/workspaces/{w}/projects/{p}/services/{service}/rollback"),
                    Some(json!({"deployment_id": deployment_id, "env": env})),
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Config(ConfigCmd::Show { service }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let env = cli.env.unwrap_or_else(|| "production".into());
            let v = client
                .request(
                    reqwest::Method::GET,
                    &format!("/v1/workspaces/{w}/projects/{p}/services/{service}/config?env={env}"),
                    None,
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Config(ConfigCmd::Set {
            service,
            cpu,
            memory_mb,
            healthcheck_path,
            healthcheck_timeout,
        }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let env = cli.env.unwrap_or_else(|| "production".into());
            let v = client
                .request(
                    reqwest::Method::PATCH,
                    &format!("/v1/workspaces/{w}/projects/{p}/services/{service}/config"),
                    Some(json!({
                        "env": env,
                        "cpu_limit": cpu,
                        "memory_limit_mb": memory_mb,
                        "healthcheck_path": healthcheck_path,
                        "healthcheck_timeout_seconds": healthcheck_timeout,
                    })),
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Backup(BackupCmd::List { service }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let v = client
                .request(
                    reqwest::Method::GET,
                    &format!("/v1/workspaces/{w}/projects/{p}/services/{service}/backups"),
                    None,
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Backup(BackupCmd::Now { service }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let v = client
                .request(
                    reqwest::Method::POST,
                    &format!("/v1/workspaces/{w}/projects/{p}/services/{service}/backups"),
                    None,
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Backup(BackupCmd::Restore {
            service,
            storage_uri,
        }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            client
                .request(
                    reqwest::Method::POST,
                    &format!("/v1/workspaces/{w}/projects/{p}/services/{service}/backups/restore"),
                    Some(json!({"storage_uri": storage_uri})),
                )
                .await?;
            if !cli.quiet {
                eprintln!("restore complete");
            }
        }
        Command::Backup(BackupCmd::ScheduleShow { service }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let v = client
                .request(
                    reqwest::Method::GET,
                    &format!("/v1/workspaces/{w}/projects/{p}/services/{service}/backup-schedule"),
                    None,
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Backup(BackupCmd::ScheduleSet {
            service,
            cron,
            retention,
            storage,
            disabled,
        }) => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            client
                .request(
                    reqwest::Method::PUT,
                    &format!("/v1/workspaces/{w}/projects/{p}/services/{service}/backup-schedule"),
                    Some(json!({
                        "cron_expression": cron,
                        "retention_count": retention,
                        "storage": storage,
                        "enabled": !disabled,
                    })),
                )
                .await?;
            if !cli.quiet {
                eprintln!("schedule updated");
            }
        }
        Command::Deployments { service } => {
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let env = cli.env.unwrap_or_else(|| "production".into());
            let v = client
                .request(
                    reqwest::Method::GET,
                    &format!(
                        "/v1/workspaces/{w}/projects/{p}/services/{service}/deployments?env={env}"
                    ),
                    None,
                )
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
        Command::Logs { service, follow } => {
            use futures::StreamExt;
            let w = ws(cli.workspace.as_deref())?;
            let p = pr(cli.project.as_deref())?;
            let env = cli.env.unwrap_or_else(|| "production".into());
            let url = format!(
                "{}/v1/workspaces/{w}/projects/{p}/services/{service}/logs?env={env}&follow={follow}",
                client.server.trim_end_matches('/')
            );
            let mut req = client.http.get(&url).header("accept", "text/event-stream");
            if let Some(t) = &client.token {
                req = req.bearer_auth(t);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| CliError::Network(e.to_string()))?;
            if !resp.status().is_success() {
                bail!(CliError::Server(format!(
                    "status {}: {}",
                    resp.status(),
                    resp.text().await.unwrap_or_default()
                )));
            }
            let mut stream = resp.bytes_stream();
            let mut buf: Vec<u8> = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| CliError::Network(e.to_string()))?;
                buf.extend_from_slice(&chunk);
                while let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
                    let event_bytes: Vec<u8> = buf.drain(..pos + 2).collect();
                    let text = String::from_utf8_lossy(&event_bytes);
                    for line in text.lines() {
                        if let Some(data) = line
                            .strip_prefix("data: ")
                            .or_else(|| line.strip_prefix("data:"))
                        {
                            match serde_json::from_str::<Value>(data.trim()) {
                                Ok(v) => {
                                    if let Some(line_text) = v.get("line").and_then(|x| x.as_str())
                                    {
                                        println!("{line_text}");
                                    }
                                }
                                Err(_) => println!("{data}"),
                            }
                        }
                    }
                }
            }
        }
        Command::Server(ServerCmd::Install) => {
            server_cmd::install(cli.quiet).await?;
        }
        Command::Server(ServerCmd::Upgrade) => {
            server_cmd::upgrade(cli.quiet).await?;
        }
        Command::Server(ServerCmd::Status) => {
            server_cmd::status().await?;
        }
        Command::Server(ServerCmd::Config(cfg)) => match cfg {
            ServerConfigCmd::Show => {
                let v = client
                    .request(reqwest::Method::GET, "/v1/server/settings", None)
                    .await?
                    .unwrap_or(Value::Null);
                print_value(&v, cli.json);
            }
            ServerConfigCmd::Domain { value } => {
                let v = client
                    .request(
                        reqwest::Method::PATCH,
                        "/v1/server/settings",
                        Some(json!({ "admin_domain": value })),
                    )
                    .await?
                    .unwrap_or(Value::Null);
                print_value(&v, cli.json);
            }
            ServerConfigCmd::AcmeEmail { value } => {
                let v = client
                    .request(
                        reqwest::Method::PATCH,
                        "/v1/server/settings",
                        Some(json!({ "acme_email": value })),
                    )
                    .await?
                    .unwrap_or(Value::Null);
                print_value(&v, cli.json);
            }
            ServerConfigCmd::PublicIp { value } => {
                let v = client
                    .request(
                        reqwest::Method::PATCH,
                        "/v1/server/settings",
                        Some(json!({ "public_ip": value })),
                    )
                    .await?
                    .unwrap_or(Value::Null);
                print_value(&v, cli.json);
            }
        },
        Command::Server(ServerCmd::Cert(ServerCertCmd::Retry)) => {
            let v = client
                .request(reqwest::Method::POST, "/v1/server/cert/retry", None)
                .await?
                .unwrap_or(Value::Null);
            print_value(&v, cli.json);
        }
    }
    Ok(())
}

async fn find_service_slug_for_domain(
    client: &Client,
    w: &str,
    p: &str,
    services: &Value,
    domain_id: &str,
) -> Result<Option<String>> {
    let Some(arr) = services.as_array() else {
        return Ok(None);
    };
    for svc in arr {
        let Some(slug) = svc.get("slug").and_then(|s| s.as_str()) else {
            continue;
        };
        let doms = client
            .request(
                reqwest::Method::GET,
                &format!("/v1/workspaces/{w}/projects/{p}/services/{slug}/domains"),
                None,
            )
            .await?
            .unwrap_or(Value::Null);
        if let Some(list) = doms.as_array() {
            if list
                .iter()
                .any(|d| d.get("id").and_then(|i| i.as_str()) == Some(domain_id))
            {
                return Ok(Some(slug.to_string()));
            }
        }
    }
    Ok(None)
}
