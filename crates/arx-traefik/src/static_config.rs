use serde::Serialize;

pub struct StaticConfigInput {
    pub dynamic_config_path: String,
    pub acme_email: String,
    pub acme_storage_path: String,
    pub api_listen: String,
}

#[derive(Serialize)]
struct StaticConfig {
    api: Api,
    entryPoints: EntryPoints,
    providers: Providers,
    certificatesResolvers: CertResolvers,
    log: LogCfg,
    accessLog: AccessLog,
}

#[derive(Serialize)]
struct Api {
    insecure: bool,
    dashboard: bool,
}

#[derive(Serialize)]
struct EntryPoints {
    web: EntryPoint,
    websecure: EntryPoint,
    traefik: EntryPoint,
}

#[derive(Serialize)]
struct EntryPoint {
    address: String,
}

#[derive(Serialize)]
struct Providers {
    file: FileProvider,
}

#[derive(Serialize)]
struct FileProvider {
    filename: String,
    watch: bool,
}

#[derive(Serialize)]
struct CertResolvers {
    le: CertResolver,
}

#[derive(Serialize)]
struct CertResolver {
    acme: Acme,
}

#[derive(Serialize)]
struct Acme {
    email: String,
    storage: String,
    httpChallenge: HttpChallenge,
}

#[derive(Serialize)]
struct HttpChallenge {
    entryPoint: String,
}

#[derive(Serialize)]
struct LogCfg {
    level: String,
}

#[derive(Serialize)]
struct AccessLog {
    format: String,
}

pub fn render_static_yaml(input: &StaticConfigInput) -> String {
    let cfg = StaticConfig {
        api: Api {
            insecure: true,
            dashboard: true,
        },
        entryPoints: EntryPoints {
            web: EntryPoint {
                address: ":80".into(),
            },
            websecure: EntryPoint {
                address: ":443".into(),
            },
            traefik: EntryPoint {
                address: input.api_listen.clone(),
            },
        },
        providers: Providers {
            file: FileProvider {
                filename: input.dynamic_config_path.clone(),
                watch: true,
            },
        },
        certificatesResolvers: CertResolvers {
            le: CertResolver {
                acme: Acme {
                    email: input.acme_email.clone(),
                    storage: input.acme_storage_path.clone(),
                    httpChallenge: HttpChallenge {
                        entryPoint: "web".into(),
                    },
                },
            },
        },
        log: LogCfg {
            level: "INFO".into(),
        },
        accessLog: AccessLog {
            format: "json".into(),
        },
    };
    serde_yaml::to_string(&cfg).expect("yaml render")
}
