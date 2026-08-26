mod config;
mod endpoint;
mod link;
mod session;
mod shutdown;

fn install_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

pub use config::Config;
pub use link::{FatalError, Report, run};
pub use shutdown::Shutdown;
