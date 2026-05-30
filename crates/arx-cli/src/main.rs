use anyhow::Result;
use std::process::ExitCode;

mod cli;
mod client;
mod commands;
mod credentials;
mod error;
mod login_cmd;
mod server_cmd;
mod setup_cmd;
mod update_check;
mod update_cmd;

pub(crate) use credentials::{load_credentials, upsert_and_save};

use crate::cli::{Cli, Command, ServerCmd};
use crate::client::Client;
use crate::credentials::{credentials_path, load_credentials as load_creds, resolve_target};
use crate::error::{CliError, exit};
use clap::Parser;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(()) => ExitCode::from(exit::SUCCESS),
        Err(e) => {
            eprintln!("error: {e:#}");
            if let Some(code) = e.downcast_ref::<CliError>() {
                ExitCode::from(code.code())
            } else {
                ExitCode::from(exit::SERVER_ERROR)
            }
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    let cred_path = credentials_path(cli.credentials.as_ref())?;
    let creds = load_creds(&cred_path)?;

    let cli_server_explicit =
        std::env::var_os("ARX_SERVER").is_some() || std::env::args().any(|a| a == "--server");

    let (server, token) = resolve_target(&creds, &cli.server, cli_server_explicit)
        .unwrap_or_else(|_| (cli.server.clone(), None));
    let client = Client::new(server.clone(), token.clone());

    // Skip the background version notice for non-interactive/JSON output and for
    // commands that already deal with versions (avoid double messaging).
    let suppress_notice = cli.json
        || cli.quiet
        || matches!(
            cli.cmd,
            Command::Update { .. } | Command::Server(ServerCmd::Upgrade) | Command::Setup { .. }
        );

    let result = commands::dispatch(cli, server, token, creds, cred_path, client).await;
    if result.is_ok() && !suppress_notice {
        update_check::notify().await;
    }
    result
}
