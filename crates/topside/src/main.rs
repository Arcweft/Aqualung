use std::process::ExitCode;

use clap::Parser;
use topside::{Cli, Config, Server, Shutdown};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let config = match Config::load(cli) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("topside: {error}");
            return ExitCode::from(2);
        }
    };
    let shutdown = match Shutdown::on_signals() {
        Ok(shutdown) => shutdown,
        Err(error) => {
            eprintln!("topside: cannot install signal handlers: {error}");
            return ExitCode::FAILURE;
        }
    };
    let server = match Server::bind(config).await {
        Ok(server) => server,
        Err(error) => {
            eprintln!("topside: {error}");
            return ExitCode::FAILURE;
        }
    };
    match server.serve(shutdown).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("topside: {error}");
            ExitCode::FAILURE
        }
    }
}
