use crate::deploy::{DeployContext, deploy_docker_image};
use crate::error::ApiResult;
use crate::state::AppState;
use arx_core::model::{DbTemplate, Deployment, Environment, Project, Service, Workspace};
use arx_db::queries::variables;
use arx_docker::VolumeMount;
use rand::Rng;
use rand::distributions::Alphanumeric;

pub async fn deploy(
    app: &AppState,
    workspace: &Workspace,
    project: &Project,
    service: &Service,
    environment: &Environment,
    template: DbTemplate,
    version: Option<&str>,
) -> ApiResult<Deployment> {
    let creds = ensure_credentials(app, service, environment, template).await?;

    let image = image_for(template, version);
    let port = default_port(template);

    let mut env = Vec::new();
    let data_dir = match template {
        DbTemplate::Postgres => {
            env.push(("POSTGRES_USER".into(), creds.user.clone()));
            env.push(("POSTGRES_PASSWORD".into(), creds.password.clone()));
            env.push(("POSTGRES_DB".into(), creds.db.clone()));
            "/var/lib/postgresql/data".to_string()
        }
        DbTemplate::Mysql => {
            env.push(("MYSQL_ROOT_PASSWORD".into(), creds.password.clone()));
            env.push(("MYSQL_DATABASE".into(), creds.db.clone()));
            "/var/lib/mysql".to_string()
        }
        DbTemplate::Mongodb => {
            env.push(("MONGO_INITDB_ROOT_USERNAME".into(), creds.user.clone()));
            env.push(("MONGO_INITDB_ROOT_PASSWORD".into(), creds.password.clone()));
            "/data/db".to_string()
        }
        DbTemplate::Redis => "/data".to_string(),
    };

    env.push(("PORT".into(), port.to_string()));

    let host_path = app
        .config
        .paths
        .volumes_dir
        .join(service.id.as_uuid().to_string())
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(&host_path).ok();

    let mounts = vec![VolumeMount {
        host_path,
        container_path: data_dir,
        read_only: false,
    }];

    let connection_url = connection_url(template, &creds);
    let _ = variables::set(
        &app.db,
        &app.master_key,
        service.id,
        environment.id,
        "DATABASE_URL",
        &connection_url,
        true,
    )
    .await;
    let _ = variables::set(
        &app.db,
        &app.master_key,
        service.id,
        environment.id,
        "INTERNAL_HOST",
        &container_hostname(service, environment),
        false,
    )
    .await;

    deploy_docker_image(
        app,
        DeployContext {
            workspace,
            project,
            service,
            environment,
            existing_dep_id: None,
            image,
            extra_env: env,
            extra_mounts: mounts,
        },
    )
    .await
}

struct Creds {
    user: String,
    password: String,
    db: String,
}

async fn ensure_credentials(
    app: &AppState,
    service: &Service,
    environment: &Environment,
    template: DbTemplate,
) -> ApiResult<Creds> {
    let existing = variables::list(&app.db, &app.master_key, service.id, environment.id).await?;
    let mut user = existing
        .iter()
        .find(|v| v.key == "ARX_DB_USER")
        .and_then(|v| v.plaintext.clone());
    let mut password = existing
        .iter()
        .find(|v| v.key == "ARX_DB_PASSWORD")
        .and_then(|v| v.plaintext.clone());
    let mut db = existing
        .iter()
        .find(|v| v.key == "ARX_DB_NAME")
        .and_then(|v| v.plaintext.clone());

    if user.is_none() || password.is_none() || db.is_none() {
        let injected =
            variables::for_injection(&app.db, &app.master_key, service.id, environment.id).await?;
        for (k, v) in injected {
            match k.as_str() {
                "ARX_DB_USER" => user = Some(v),
                "ARX_DB_PASSWORD" => password = Some(v),
                "ARX_DB_NAME" => db = Some(v),
                _ => {}
            }
        }
    }

    if user.is_none() {
        let u = format!("arx_{}", random_lower(8));
        variables::set(
            &app.db,
            &app.master_key,
            service.id,
            environment.id,
            "ARX_DB_USER",
            &u,
            false,
        )
        .await?;
        user = Some(u);
    }
    if password.is_none() {
        let p = random_alnum(24);
        variables::set(
            &app.db,
            &app.master_key,
            service.id,
            environment.id,
            "ARX_DB_PASSWORD",
            &p,
            true,
        )
        .await?;
        password = Some(p);
    }
    if db.is_none() {
        let d = match template {
            DbTemplate::Postgres | DbTemplate::Mysql => service.slug.replace('-', "_"),
            _ => service.slug.clone(),
        };
        variables::set(
            &app.db,
            &app.master_key,
            service.id,
            environment.id,
            "ARX_DB_NAME",
            &d,
            false,
        )
        .await?;
        db = Some(d);
    }

    Ok(Creds {
        user: user.unwrap(),
        password: password.unwrap(),
        db: db.unwrap(),
    })
}

fn image_for(t: DbTemplate, version: Option<&str>) -> String {
    let v = version.unwrap_or(default_version(t));
    match t {
        DbTemplate::Postgres => format!("postgres:{v}"),
        DbTemplate::Mysql => format!("mysql:{v}"),
        DbTemplate::Mongodb => format!("mongo:{v}"),
        DbTemplate::Redis => format!("redis:{v}"),
    }
}

fn default_version(t: DbTemplate) -> &'static str {
    match t {
        DbTemplate::Postgres => "16-alpine",
        DbTemplate::Mysql => "8",
        DbTemplate::Mongodb => "7",
        DbTemplate::Redis => "7-alpine",
    }
}

fn default_port(t: DbTemplate) -> u16 {
    match t {
        DbTemplate::Postgres => 5432,
        DbTemplate::Mysql => 3306,
        DbTemplate::Mongodb => 27017,
        DbTemplate::Redis => 6379,
    }
}

fn connection_url(t: DbTemplate, c: &Creds) -> String {
    let host = "__INTERNAL_HOST__";
    let port = default_port(t);
    match t {
        DbTemplate::Postgres => format!(
            "postgresql://{user}:{pw}@{host}:{port}/{db}",
            user = urlencode(&c.user),
            pw = urlencode(&c.password),
            db = c.db
        ),
        DbTemplate::Mysql => format!(
            "mysql://root:{pw}@{host}:{port}/{db}",
            pw = urlencode(&c.password),
            db = c.db
        ),
        DbTemplate::Mongodb => format!(
            "mongodb://{user}:{pw}@{host}:{port}/{db}?authSource=admin",
            user = urlencode(&c.user),
            pw = urlencode(&c.password),
            db = c.db
        ),
        DbTemplate::Redis => format!("redis://{host}:{port}"),
    }
}

fn urlencode(s: &str) -> String {
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

fn container_hostname(service: &Service, environment: &Environment) -> String {
    format!("arx-{}-{}", service.slug, environment.slug)
}

fn random_lower(n: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..n)
        .map(|_| {
            let c = rng.gen_range(b'a'..=b'z');
            c as char
        })
        .collect()
}

fn random_alnum(n: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(n)
        .map(char::from)
        .collect()
}
