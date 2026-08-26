use std::time::Duration;

use rand::Rng;
use thiserror::Error;
use tokio::time::Instant;

use crate::{config::Config, endpoint::RemoteDialer, session::Session, shutdown::Shutdown};

const BACKOFF_FIRST: Duration = Duration::from_millis(500);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

#[allow(
    clippy::large_enum_variant,
    reason = "one Link exists and Up must own Session directly"
)]
enum Link {
    Down {
        not_before: Instant,
        run: FailureRun,
    },
    Up(Session),
    Stopped,
}

#[derive(Clone, Copy, Default)]
struct FailureRun {
    attempts: u32,
}

struct Context {
    config: Config,
    remote: RemoteDialer,
    shutdown: Shutdown,
    report: Report,
}

#[derive(Debug, Default)]
pub struct Report {
    pub sessions: u64,
    pub bytes_to_server: u64,
    pub bytes_to_socket: u64,
}

#[derive(Debug, Error)]
pub enum FatalError {
    #[error("runtime failure: {0}")]
    Runtime(String),
}

pub async fn run(config: Config, shutdown: Shutdown) -> Result<Report, FatalError> {
    let remote = RemoteDialer::new(&config);
    let mut context = Context {
        config,
        remote,
        shutdown,
        report: Report::default(),
    };
    let mut link = Link::Down {
        not_before: Instant::now(),
        run: FailureRun::default(),
    };

    loop {
        link = link.step(&mut context).await;
        if matches!(link, Link::Stopped) {
            return Ok(context.report);
        }
    }
}

impl Link {
    async fn step(self, context: &mut Context) -> Self {
        match self {
            Self::Down { not_before, run } => {
                if context.shutdown.is_set() {
                    return Self::Stopped;
                }

                let mut shutdown = context.shutdown.clone();
                tokio::select! {
                    _ = tokio::time::sleep_until(not_before) => {}
                    _ = shutdown.wait() => return Self::Stopped,
                }

                if !context.config.socket.exists() {
                    return failed(context, run, "unix socket file is absent");
                }

                let mut shutdown = context.shutdown.clone();
                let established = tokio::select! {
                    result = Session::establish(&context.config, &context.remote) => result,
                    _ = shutdown.wait() => return Self::Stopped,
                };

                match established {
                    Ok(session) => {
                        context.report.sessions += 1;
                        eprintln!("session {} up", context.report.sessions);
                        Self::Up(session)
                    }
                    Err(error) => failed(context, run, &error.to_string()),
                }
            }
            Self::Up(session) => {
                let mut shutdown = context.shutdown.clone();
                let outcome = tokio::select! {
                    outcome = session.run() => outcome,
                    _ = shutdown.wait() => return Self::Stopped,
                };

                context.report.bytes_to_server += outcome.to_remote;
                context.report.bytes_to_socket += outcome.to_local;
                match &outcome.error {
                    Some(error) => eprintln!(
                        "session ended by {:?} after {:?}: {}",
                        outcome.ended_by, outcome.lasted, error
                    ),
                    None => eprintln!(
                        "session ended by {:?} after {:?}",
                        outcome.ended_by, outcome.lasted
                    ),
                }

                if context.config.once {
                    Self::Stopped
                } else {
                    let run = if outcome.productive() {
                        FailureRun::default()
                    } else {
                        FailureRun { attempts: 1 }
                    };
                    Self::Down {
                        not_before: Instant::now() + backoff(run),
                        run,
                    }
                }
            }
            Self::Stopped => Self::Stopped,
        }
    }
}

fn failed(context: &Context, run: FailureRun, error: &str) -> Link {
    eprintln!("dial failed: {error}");
    if context.config.once {
        return Link::Stopped;
    }
    let run = FailureRun {
        attempts: run.attempts.saturating_add(1),
    };
    Link::Down {
        not_before: Instant::now() + backoff(run),
        run,
    }
}

fn backoff(run: FailureRun) -> Duration {
    let exponent = run.attempts.saturating_sub(1).min(16);
    let base = BACKOFF_FIRST
        .saturating_mul(1_u32 << exponent)
        .min(BACKOFF_MAX);
    let jitter = rand::rng().random_range(0.8..=1.0);
    base.mul_f64(jitter)
}
