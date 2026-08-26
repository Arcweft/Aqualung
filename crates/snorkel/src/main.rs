use std::{path::PathBuf, process::ExitCode};

use clap::Parser;
use snorkel::{Config, Shutdown};

#[derive(Parser)]
#[command(
    version,
    about = "Dial out from home and copy bytes to aqualung's topside",
    after_help = "The server port defaults to 1943. Exit status 0 means a signal stopped snorkel or --once completed. Bad configuration exits 2."
)]
struct Cli {
    #[arg(long, env = "SNORKEL_SOCKET", value_name = "PATH")]
    socket: PathBuf,

    #[arg(
        long,
        env = "SNORKEL_SERVER",
        value_name = "HOST[:PORT]",
        help = "mTLS server; port defaults to 1943"
    )]
    server: String,

    #[arg(long, env = "SNORKEL_CERT", value_name = "PEM")]
    cert: PathBuf,

    #[arg(long, env = "SNORKEL_KEY", value_name = "PEM")]
    key: PathBuf,

    #[arg(long, env = "SNORKEL_CA", value_name = "PEM")]
    ca: PathBuf,

    #[arg(long, help = "Run one connection attempt and do not reconnect")]
    once: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let config = match Config::load(
        cli.socket,
        &cli.server,
        &cli.cert,
        &cli.key,
        &cli.ca,
        cli.once,
    ) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("snorkel: {error}");
            return ExitCode::from(2);
        }
    };
    let shutdown = match Shutdown::on_signals() {
        Ok(shutdown) => shutdown,
        Err(error) => {
            eprintln!("snorkel: cannot install signal handlers: {error}");
            return ExitCode::FAILURE;
        }
    };

    match snorkel::run(config, shutdown).await {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("snorkel: {error}");
            ExitCode::FAILURE
        }
    }
}
