use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "arx",
    version,
    about = "arx CLI",
    disable_help_subcommand = true
)]
pub(crate) struct Cli {
    #[arg(
        long,
        env = "ARX_SERVER",
        default_value = "http://127.0.0.1:7878",
        global = true
    )]
    pub server: String,

    #[arg(short = 'w', long, env = "ARX_WORKSPACE", global = true)]
    pub workspace: Option<String>,

    #[arg(short = 'p', long, env = "ARX_PROJECT", global = true)]
    pub project: Option<String>,

    #[arg(short = 'e', long, env = "ARX_ENV", global = true)]
    pub env: Option<String>,

    #[arg(long, global = true)]
    pub json: bool,

    #[arg(short = 'q', long, global = true)]
    pub quiet: bool,

    #[arg(long, env = "ARX_CREDENTIALS", global = true)]
    pub credentials: Option<PathBuf>,

    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    Setup {
        #[arg(long)]
        no_browser: bool,

        #[arg(long)]
        headless: bool,

        #[arg(long)]
        public_ip: Option<String>,

        #[arg(long)]
        root_domain: Option<String>,

        #[arg(long)]
        admin_domain: Option<String>,

        #[arg(long)]
        acme_email: Option<String>,
    },

    Login {
        #[arg(long)]
        device: bool,

        #[arg(long, conflicts_with = "device")]
        token: Option<String>,
    },

    Logout,

    Whoami,

    #[command(subcommand)]
    Workspace(WorkspaceCmd),

    #[command(subcommand)]
    Project(ProjectCmd),

    #[command(subcommand)]
    Environment(EnvironmentCmd),

    #[command(subcommand)]
    Service(ServiceCmd),

    #[command(subcommand)]
    Var(VarCmd),

    #[command(subcommand)]
    Domain(DomainCmd),

    Deploy {
        service: String,
    },

    Rollback {
        service: String,

        deployment_id: String,
    },

    /// Restart a service in place (re-run the current image, no rebuild).
    Restart {
        service: String,
    },

    Deployments {
        service: String,
    },

    Logs {
        service: String,

        #[arg(short, long)]
        follow: bool,
    },

    #[command(subcommand)]
    Config(ConfigCmd),

    #[command(subcommand)]
    Backup(BackupCmd),

    #[command(subcommand)]
    Server(ServerCmd),

    #[command(subcommand)]
    Volume(VolumeCmd),
}

#[derive(Debug, Subcommand)]
pub(crate) enum VolumeCmd {
    /// List all arx-managed docker volumes with classification.
    List,

    /// Remove orphan volumes (volumes whose owning service no longer exists).
    /// Default is dry-run; pass --execute to actually remove.
    Prune {
        #[arg(long)]
        execute: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ServerCmd {
    Install,

    Upgrade,

    Status,

    #[command(subcommand)]
    Config(ServerConfigCmd),

    #[command(subcommand)]
    Cert(ServerCertCmd),

    /// Re-sync GitHub App installations and their repositories from GitHub
    Sync {
        /// Also re-point the GitHub App's webhook URL at the current domain
        #[arg(long)]
        app: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ServerConfigCmd {
    Show,

    Domain { value: String },
    AcmeEmail { value: String },
    PublicIp { value: String },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ServerCertCmd {
    Retry,
}

#[derive(Debug, Subcommand)]
pub(crate) enum BackupCmd {
    List {
        service: String,
    },

    Now {
        service: String,
    },

    Restore {
        service: String,
        storage_uri: String,
    },

    ScheduleShow {
        service: String,
    },

    ScheduleSet {
        service: String,
        #[arg(long, default_value = "0 3 * * *")]
        cron: String,
        #[arg(long, default_value_t = 7)]
        retention: i32,
        #[arg(long, default_value = "local")]
        storage: String,
        #[arg(long)]
        disabled: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ConfigCmd {
    Show {
        service: String,
    },
    Set {
        service: String,
        #[arg(long)]
        cpu: Option<f64>,
        #[arg(long)]
        memory_mb: Option<i64>,
        #[arg(long)]
        healthcheck_path: Option<String>,
        #[arg(long)]
        healthcheck_timeout: Option<i32>,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum WorkspaceCmd {
    List,
    Create {
        #[arg(long)]
        slug: String,
        #[arg(long)]
        name: String,
    },
    Delete {
        slug: String,
        #[arg(long)]
        force: bool,
        /// Also remove docker named volumes and backup files for every affected service.
        #[arg(long)]
        with_data: bool,
    },
    Rename {
        slug: String,
        name: String,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProjectCmd {
    List,
    Create {
        #[arg(long)]
        slug: String,
        #[arg(long)]
        name: String,
    },
    Delete {
        slug: String,
        #[arg(long)]
        force: bool,
        /// Also remove docker named volumes and backup files for every affected service.
        #[arg(long)]
        with_data: bool,
    },
    Rename {
        slug: String,
        name: String,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum EnvironmentCmd {
    List,
    Create {
        #[arg(long)]
        slug: String,
        #[arg(long)]
        name: String,
    },
    Delete {
        slug: String,
        #[arg(long)]
        force: bool,
        /// Also remove docker named volumes for this environment.
        #[arg(long)]
        with_data: bool,
    },
    Rename {
        slug: String,
        name: String,
    },
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ServiceCmd {
    List,
    Show {
        slug: String,
    },
    Create {
        #[arg(long)]
        slug: String,
        #[arg(long)]
        name: String,

        #[arg(long, value_parser = ["git", "image", "db"])]
        kind: String,

        #[arg(long, required_if_eq("kind", "git"))]
        repo: Option<String>,
        #[arg(long, default_value = "main")]
        branch: String,

        #[arg(long, required_if_eq("kind", "image"))]
        image: Option<String>,

        #[arg(long, required_if_eq("kind", "db"), value_parser = ["postgres", "mysql", "mongodb", "redis"])]
        template: Option<String>,

        /// Optional explicit Dockerfile path inside the repo (relative).
        #[arg(long)]
        dockerfile: Option<String>,
        /// Subdirectory of the repo to build. For monorepos, this is the package directory (e.g. `apps/web`).
        #[arg(long)]
        root_directory: Option<String>,
        /// Gitignore-style glob restricting which pushed file paths trigger a redeploy. Pass multiple times.
        #[arg(long = "watch-path")]
        watch_paths: Vec<String>,
        /// Override the auto-detected build command (passed to the builder).
        #[arg(long = "build-cmd")]
        build_command: Option<String>,
        /// Override the container start command. Pass an empty string ("") to clear later via `service config`.
        #[arg(long = "start-cmd")]
        start_command: Option<String>,
    },
    Delete {
        slug: String,
        #[arg(long)]
        force: bool,
        /// Also remove the docker named volume and backup files for this service.
        #[arg(long)]
        with_data: bool,
    },

    Rename {
        slug: String,
        name: String,
    },

    /// Update build/start commands or other service-level settings.
    Config {
        #[command(subcommand)]
        cmd: ServiceConfigCmd,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ServiceConfigCmd {
    /// Set or clear build / start commands. Empty string ("") clears the field.
    Set {
        slug: String,
        #[arg(long = "build-cmd")]
        build_command: Option<String>,
        #[arg(long = "start-cmd")]
        start_command: Option<String>,
        /// Restart policy: no | unless-stopped | always | on-failure.
        #[arg(long = "restart-policy")]
        restart_policy: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum VarCmd {
    List {
        service: String,
    },
    Set {
        service: String,
        #[arg(value_name = "KEY=VALUE")]
        kv: String,
        #[arg(long)]
        sealed: bool,
    },
    Unset {
        service: String,
        key: String,
    },

    Import {
        service: String,

        file: PathBuf,

        #[arg(long)]
        sealed_all: bool,

        #[arg(long)]
        overwrite: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum DomainCmd {
    List { service: String },
    Add { service: String, hostname: String },
    Remove { id: String },
}
