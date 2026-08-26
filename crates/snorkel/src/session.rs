use std::{io, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpStream, UnixStream},
    time::Instant,
};
use tokio_rustls::client::TlsStream;

use crate::{
    config::Config,
    endpoint::{DialError, RemoteDialer, connect_local},
};

const COPY_BUFFER: usize = 64 * 1024;

pub(crate) struct Session {
    local: UnixStream,
    remote: TlsStream<TcpStream>,
    opened: Instant,
}

impl Session {
    pub(crate) async fn establish(
        config: &Config,
        remote: &RemoteDialer,
    ) -> Result<Self, DialError> {
        let remote = remote.dial().await?;
        let local = connect_local(config).await?;
        Ok(Self {
            local,
            remote,
            opened: Instant::now(),
        })
    }

    pub(crate) async fn run(self) -> Outcome {
        let (local_read, local_write) = tokio::io::split(self.local);
        let (remote_read, remote_write) = tokio::io::split(self.remote);
        let mut to_remote = 0;
        let mut to_local = 0;

        let (ended_by, error) = {
            let up = copy_half(local_read, remote_write, &mut to_remote);
            let down = copy_half(remote_read, local_write, &mut to_local);
            tokio::pin!(up, down);
            tokio::select! {
                result = &mut up => (Side::Local, result.err()),
                result = &mut down => (Side::Remote, result.err()),
            }
        };

        Outcome {
            ended_by,
            error,
            to_remote,
            to_local,
            lasted: self.opened.elapsed(),
        }
    }
}

pub(crate) struct Outcome {
    pub(crate) ended_by: Side,
    pub(crate) error: Option<io::Error>,
    pub(crate) to_remote: u64,
    pub(crate) to_local: u64,
    pub(crate) lasted: Duration,
}

impl Outcome {
    pub(crate) fn productive(&self) -> bool {
        self.to_remote != 0 || self.to_local != 0 || self.lasted >= Duration::from_secs(10)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Side {
    Local,
    Remote,
}

async fn copy_half<R, W>(mut reader: R, mut writer: W, moved: &mut u64) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0; COPY_BUFFER];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            let _ = writer.shutdown().await;
            return Ok(());
        }
        writer.write_all(&buffer[..read]).await?;
        *moved += read as u64;
    }
}
