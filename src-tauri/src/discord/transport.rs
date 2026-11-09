//! A Unix-socket IPC client with read timeouts.
//!
//! `discord_rich_presence::DiscordIpcClient` keeps its socket private, so a
//! read can never time out. Against a real Discord this matters: Discord
//! silently drops updates that arrive too fast and then never answers, and a
//! blocking read waits for that answer forever. This type implements the
//! crate's `DiscordIpc` trait with its own socket, so framing, `send`, `recv`,
//! `set_activity` and `clear_activity` still come from the crate.

use std::{
    env,
    io::{ErrorKind, Read, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};

use discord_rich_presence::{error::Error, DiscordIpc};
use serde_json::json;

type Result<T> = std::result::Result<T, Error>;

// Where Discord documents its socket: the first of these that is set (then /tmp).
const ENV_KEYS: [&str; 4] = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"];

const SUBPATHS: [&str; 7] = [
    "",
    "app/com.discordapp.Discord/",
    "app/dev.vencord.Vesktop/",
    ".flatpak/com.discordapp.Discord/xdg-run/",
    ".flatpak/dev.vencord.Vesktop/xdg-run/",
    "snap.discord-canary/",
    "snap.discord/",
];

/// Every `discord-ipc-N` under `base`, in the order Discord would number
/// them. Stale files from a crashed Discord are included: the caller tries
/// each one so a dead socket cannot hide a live one.
pub fn socket_candidates(base: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for index in 0..10 {
        for sub in SUBPATHS {
            let path = base.join(sub).join(format!("discord-ipc-{index}"));
            if path.exists() {
                found.push(path);
            }
        }
    }
    found
}

pub struct TimedClient {
    client_id: String,
    timeout: Duration,
    socket: Option<UnixStream>,
}

impl TimedClient {
    pub fn new(client_id: &str, timeout: Duration) -> Self {
        Self {
            client_id: client_id.to_string(),
            timeout,
            socket: None,
        }
    }

    pub fn connect_to(&mut self, path: &Path) -> Result<()> {
        let socket = UnixStream::connect(path).map_err(|_| Error::IPCConnectionFailed)?;
        socket
            .set_read_timeout(Some(self.timeout))
            .and_then(|()| socket.set_write_timeout(Some(self.timeout)))
            .map_err(|_| Error::IPCConnectionFailed)?;
        self.socket = Some(socket);
        Ok(())
    }

    fn socket(&mut self) -> Result<&mut UnixStream> {
        self.socket.as_mut().ok_or(Error::NotConnected)
    }
}

/// `true` if `error` is a read that ran out of time rather than a broken connection.
pub fn is_timeout(error: &Error) -> bool {
    matches!(error, Error::ReadError(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut))
}

impl DiscordIpc for TimedClient {
    fn get_client_id(&self) -> &str {
        &self.client_id
    }

    fn connect_ipc(&mut self) -> Result<()> {
        let bases = ENV_KEYS
            .iter()
            .filter_map(|key| env::var(key).ok())
            .map(PathBuf::from)
            .chain(std::iter::once(PathBuf::from("/tmp")))
            .filter(|dir| dir.is_dir());
        // The first directory that holds any socket decides, as Discord does.
        let candidates = bases
            .map(|dir| socket_candidates(&dir))
            .find(|found| !found.is_empty())
            .ok_or(Error::IPCNotFound)?;
        for path in &candidates {
            if self.connect_to(path).is_ok() {
                return Ok(());
            }
        }
        Err(Error::IPCConnectionFailed)
    }

    fn write(&mut self, data: &[u8]) -> Result<()> {
        self.socket()?.write_all(data).map_err(Error::WriteError)
    }

    fn read(&mut self, buffer: &mut [u8]) -> Result<()> {
        self.socket()?.read_exact(buffer).map_err(Error::ReadError)
    }

    fn close(&mut self) -> Result<()> {
        let _ = self.send(json!({}), 2);
        let socket = self.socket()?;
        let _ = socket.flush();
        let _ = socket.shutdown(Shutdown::Both);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::net::UnixListener, time::Instant};

    #[test]
    fn every_socket_is_a_candidate_including_stale_ones() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("discord-ipc-0"), b"").unwrap(); // stale
        fs::write(dir.path().join("discord-ipc-3"), b"").unwrap();
        fs::create_dir(dir.path().join("snap.discord")).unwrap();
        fs::write(dir.path().join("snap.discord/discord-ipc-1"), b"").unwrap();

        let found: Vec<_> = socket_candidates(dir.path())
            .iter()
            .map(|p| p.strip_prefix(dir.path()).unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(found, ["discord-ipc-0", "snap.discord/discord-ipc-1", "discord-ipc-3"]);
    }

    #[test]
    fn a_directory_without_sockets_has_no_candidates() {
        assert!(socket_candidates(tempfile::tempdir().unwrap().path()).is_empty());
    }

    #[test]
    fn a_file_that_is_not_a_socket_fails_to_connect() {
        let dir = tempfile::tempdir().unwrap();
        let stale = dir.path().join("discord-ipc-0");
        fs::write(&stale, b"").unwrap();
        let mut client = TimedClient::new("1", Duration::from_millis(100));
        assert!(matches!(client.connect_to(&stale), Err(Error::IPCConnectionFailed)));
    }

    #[test]
    fn a_silent_peer_times_out_instead_of_blocking_forever() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("discord-ipc-0");
        let listener = UnixListener::bind(&path).unwrap();
        let _server = std::thread::spawn(move || {
            // Accept and then say nothing, like Discord dropping a rate-limited update.
            let held = listener.accept();
            std::thread::sleep(Duration::from_secs(2));
            drop(held);
        });

        let mut client = TimedClient::new("1", Duration::from_millis(150));
        client.connect_to(&path).unwrap();
        client.send(json!({"cmd": "SET_ACTIVITY"}), 1).unwrap();

        let started = Instant::now();
        let error = client.recv().unwrap_err();
        assert!(is_timeout(&error), "expected a timeout, got {error:?}");
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_closed_peer_is_not_mistaken_for_a_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("discord-ipc-0");
        let listener = UnixListener::bind(&path).unwrap();
        // The server must keep the connection open until the client has set
        // it up: on macOS, setting a timeout on a socket whose peer is already
        // gone fails, which made this test flaky when the server closed first.
        let (client_ready, wait_for_client) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let held = listener.accept();
            let _ = wait_for_client.recv();
            drop(held);
        });

        let mut client = TimedClient::new("1", Duration::from_millis(500));
        client.connect_to(&path).unwrap();
        client_ready.send(()).unwrap();
        server.join().unwrap();
        let error = client.recv().unwrap_err();
        assert!(!is_timeout(&error), "EOF is a lost connection: {error:?}");
    }
}
