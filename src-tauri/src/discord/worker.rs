use std::{
    collections::VecDeque,
    sync::{
        mpsc::{self, RecvTimeoutError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use serde::Serialize;
use serde_json::{json, Value};

use super::activity::ActivityPayload;
use crate::util::{lock, now_ms};

/// Discord answers an unknown application id with CLOSE code 4000.
const INVALID_CLIENT_ID: i64 = 4000;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnState {
    /// Not asked to connect (no application id, or disconnected by the user).
    Idle,
    Connecting,
    Connected,
    /// Wanted, but Discord is not reachable. The worker keeps retrying.
    Disconnected,
    /// A problem retrying cannot fix (e.g. an invalid application id).
    Error,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiscordUser {
    pub id: String,
    pub username: String,
    pub global_name: Option<String>,
    pub avatar: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PresenceOutcome {
    pub ok: bool,
    pub message: Option<String>,
    pub at: i64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiscordStatus {
    pub state: ConnState,
    pub application_id: Option<String>,
    pub user: Option<DiscordUser>,
    pub message: Option<String>,
    /// An activity is currently set on the connected Discord client.
    pub presence_active: bool,
    pub last_presence: Option<PresenceOutcome>,
    pub attempts: u32,
    pub updated_at: i64,
}

impl Default for DiscordStatus {
    fn default() -> Self {
        Self {
            state: ConnState::Idle,
            application_id: None,
            user: None,
            message: None,
            presence_active: false,
            last_presence: None,
            attempts: 0,
            updated_at: now_ms(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WorkerConfig {
    /// Delay between attempts while Discord is not running.
    pub reconnect_interval: Duration,
    /// The presence is re-sent this often; a failed send reveals a dead connection.
    pub heartbeat: Duration,
    pub min_push_spacing: Duration,
    /// Discord allows 5 activity updates per 20 s; stay below it.
    pub rate_window: Duration,
    pub rate_max: usize,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            reconnect_interval: Duration::from_secs(3),
            heartbeat: Duration::from_secs(15),
            min_push_spacing: Duration::from_secs(1),
            rate_window: Duration::from_secs(20),
            rate_max: 4,
        }
    }
}

enum Cmd {
    Connect(String),
    Disconnect,
    Set(Option<ActivityPayload>),
    Shutdown,
}

/// Cheap handle to the Discord IPC worker thread.
///
/// The worker owns the blocking IPC client and the *desired* presence. The
/// session layer only states what should be shown; whether Discord is
/// currently reachable is the worker's concern, so a dropped connection never
/// affects a game session and the presence is restored on reconnect.
pub struct DiscordHandle {
    tx: mpsc::Sender<Cmd>,
    status: Arc<Mutex<DiscordStatus>>,
    done: Mutex<Option<mpsc::Receiver<()>>>,
}

impl DiscordHandle {
    pub fn spawn(config: WorkerConfig, notify: impl Fn(&DiscordStatus) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(DiscordStatus::default()));
        let worker = Worker::new(config, status.clone(), Box::new(notify));
        thread::Builder::new()
            .name("discord-ipc".into())
            .spawn(move || {
                worker.run(rx);
                let _ = done_tx.send(());
            })
            .expect("failed to spawn the discord worker thread");
        Self {
            tx,
            status,
            done: Mutex::new(Some(done_rx)),
        }
    }

    pub fn connect(&self, application_id: &str) {
        let _ = self.tx.send(Cmd::Connect(application_id.to_string()));
    }

    pub fn disconnect(&self) {
        let _ = self.tx.send(Cmd::Disconnect);
    }

    /// `None` clears the presence.
    pub fn set_activity(&self, activity: Option<ActivityPayload>) {
        let _ = self.tx.send(Cmd::Set(activity));
    }

    pub fn status(&self) -> DiscordStatus {
        lock(&self.status).clone()
    }

    /// Clears the presence, closes the socket and stops the thread. Waits at
    /// most `timeout`: Discord being unresponsive must not hang app exit.
    pub fn shutdown(&self, timeout: Duration) {
        let _ = self.tx.send(Cmd::Shutdown);
        if let Some(done) = lock(&self.done).take() {
            let _ = done.recv_timeout(timeout);
        }
    }
}

enum ConnectError {
    /// Worth retrying (Discord not running yet, still starting, ...).
    Retryable(String),
    /// Retrying cannot help until the user changes something.
    Fatal(String),
}

impl ConnectError {
    fn message(&self) -> &str {
        match self {
            Self::Retryable(m) | Self::Fatal(m) => m,
        }
    }
}

fn close_message(frame: &Value) -> (Option<i64>, String) {
    (
        frame["code"].as_i64(),
        frame["message"].as_str().unwrap_or("알 수 없는 오류").to_string(),
    )
}

/// Connects and performs the RPC handshake.
///
/// `DiscordIpcClient::connect` discards the handshake reply (the crate marks
/// rejection handling as a TODO), so the handshake is done by hand here to
/// read the READY event or the CLOSE frame that explains a rejection.
fn connect_client(application_id: &str) -> Result<(DiscordIpcClient, DiscordUser), ConnectError> {
    let mut client = DiscordIpcClient::new(application_id);
    client.connect_ipc().map_err(|_| {
        ConnectError::Retryable(
            "Discord 데스크톱 앱이 실행 중이 아니거나 IPC에 연결할 수 없습니다.".into(),
        )
    })?;
    client
        .send(json!({ "v": 1, "client_id": application_id }), 0)
        .map_err(|e| ConnectError::Retryable(format!("핸드셰이크를 보내지 못했습니다: {e}")))?;

    match client.recv() {
        Ok((1, frame)) if frame["evt"] == "READY" => {
            let user = &frame["data"]["user"];
            let user = DiscordUser {
                id: user["id"].as_str().unwrap_or_default().to_string(),
                username: user["username"].as_str().unwrap_or_default().to_string(),
                global_name: user["global_name"].as_str().map(str::to_string),
                avatar: user["avatar"].as_str().map(str::to_string),
            };
            Ok((client, user))
        }
        Ok((2, frame)) => {
            let (code, message) = close_message(&frame);
            let _ = client.close();
            if code == Some(INVALID_CLIENT_ID) {
                Err(ConnectError::Fatal(format!(
                    "Application ID가 올바르지 않습니다 (Discord: {message}). \
                     Developer Portal에서 ID를 다시 확인하세요."
                )))
            } else {
                Err(ConnectError::Retryable(format!(
                    "Discord가 연결을 거부했습니다: {message}"
                )))
            }
        }
        Ok((op, _)) => Err(ConnectError::Retryable(format!(
            "예상하지 못한 핸드셰이크 응답입니다 (opcode {op})."
        ))),
        Err(e) => Err(ConnectError::Retryable(format!("핸드셰이크 응답을 읽지 못했습니다: {e}"))),
    }
}

/// One-off connection check that does not touch the worker or any presence.
pub fn probe(application_id: &str) -> Result<DiscordUser, String> {
    match connect_client(application_id) {
        Ok((mut client, user)) => {
            let _ = client.close();
            Ok(user)
        }
        Err(e) => Err(e.message().to_string()),
    }
}

enum Reply {
    Ok,
    /// Discord answered, but refused the activity. The connection is fine.
    Rejected(String),
    /// The socket closed or broke.
    Lost(String),
}

/// Reads the answer to the command just sent. Discord answers every
/// SET_ACTIVITY; the crate itself never reads it, which would hide both
/// rejected payloads and a closed connection.
fn read_reply(client: &mut DiscordIpcClient) -> Reply {
    for _ in 0..8 {
        match client.recv() {
            Ok((1, frame)) => {
                if frame["evt"] == "ERROR" {
                    let (code, message) = close_message(&frame["data"]);
                    let code = code.map(|c| format!(" ({c})")).unwrap_or_default();
                    return Reply::Rejected(format!("Discord가 Presence를 거부했습니다{code}: {message}"));
                }
                return Reply::Ok;
            }
            Ok((2, frame)) => {
                let (_, message) = close_message(&frame);
                return Reply::Lost(format!("Discord가 연결을 닫았습니다: {message}"));
            }
            Ok((3, ping)) => {
                let _ = client.send(ping, 4);
            }
            Ok(_) => {}
            Err(e) => return Reply::Lost(format!("Discord와의 연결이 끊어졌습니다: {e}")),
        }
    }
    Reply::Ok
}

struct Worker {
    config: WorkerConfig,
    status: Arc<Mutex<DiscordStatus>>,
    notify: Box<dyn Fn(&DiscordStatus) + Send>,
    app_id: Option<String>,
    enabled: bool,
    /// A fatal handshake error: wait for the next explicit `Connect`.
    blocked: bool,
    desired: Option<ActivityPayload>,
    client: Option<DiscordIpcClient>,
    dirty: bool,
    next_connect_at: Instant,
    last_push: Option<Instant>,
    push_log: VecDeque<Instant>,
}

impl Worker {
    fn new(
        config: WorkerConfig,
        status: Arc<Mutex<DiscordStatus>>,
        notify: Box<dyn Fn(&DiscordStatus) + Send>,
    ) -> Self {
        Self {
            config,
            status,
            notify,
            app_id: None,
            enabled: false,
            blocked: false,
            desired: None,
            client: None,
            dirty: false,
            next_connect_at: Instant::now(),
            last_push: None,
            push_log: VecDeque::new(),
        }
    }

    fn run(mut self, rx: mpsc::Receiver<Cmd>) {
        loop {
            let wait = self.next_wait();
            match rx.recv_timeout(wait) {
                Ok(Cmd::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                    self.close_client(true);
                    return;
                }
                Ok(cmd) => {
                    self.handle(cmd);
                    // Coalesce a burst of commands into a single update.
                    while let Ok(cmd) = rx.try_recv() {
                        if matches!(cmd, Cmd::Shutdown) {
                            self.close_client(true);
                            return;
                        }
                        self.handle(cmd);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.tick();
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Connect(id) => {
                let changed = self.app_id.as_deref() != Some(id.as_str());
                if self.client.is_some() && !changed {
                    return;
                }
                self.close_client(false);
                self.app_id = Some(id.clone());
                self.enabled = true;
                self.blocked = false;
                self.next_connect_at = Instant::now();
                self.update(|s| {
                    s.state = ConnState::Connecting;
                    s.application_id = Some(id);
                    s.user = None;
                    s.message = None;
                    s.presence_active = false;
                    s.attempts = 0;
                });
            }
            Cmd::Disconnect => {
                self.enabled = false;
                self.blocked = false;
                self.close_client(true);
                self.update(|s| {
                    s.state = ConnState::Idle;
                    s.user = None;
                    s.message = Some("연결이 해제되었습니다.".into());
                    s.presence_active = false;
                });
            }
            Cmd::Set(activity) => {
                self.desired = activity;
                self.dirty = true;
            }
            Cmd::Shutdown => {}
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        if self.enabled && !self.blocked && self.client.is_none() && now >= self.next_connect_at {
            self.try_connect();
        }
        if self.client.is_some() && self.push_due(now) {
            self.push();
        }
    }

    fn try_connect(&mut self) {
        let Some(app_id) = self.app_id.clone() else {
            return;
        };
        lock(&self.status).attempts += 1;
        match connect_client(&app_id) {
            Ok((client, user)) => {
                self.client = Some(client);
                self.last_push = None;
                self.dirty = self.desired.is_some();
                self.update(|s| {
                    s.state = ConnState::Connected;
                    s.user = Some(user);
                    s.message = None;
                    s.attempts = 0;
                });
            }
            Err(ConnectError::Retryable(message)) => {
                self.next_connect_at = Instant::now() + self.config.reconnect_interval;
                self.set_state(ConnState::Disconnected, Some(message));
            }
            Err(ConnectError::Fatal(message)) => {
                self.blocked = true;
                self.set_state(ConnState::Error, Some(message));
            }
        }
    }

    /// Earliest moment the next presence update may be sent.
    fn push_allowed_at(&mut self, now: Instant) -> Instant {
        while let Some(&oldest) = self.push_log.front() {
            if now.duration_since(oldest) >= self.config.rate_window {
                self.push_log.pop_front();
            } else {
                break;
            }
        }
        let mut at = now;
        if let Some(last) = self.last_push {
            at = at.max(last + self.config.min_push_spacing);
        }
        if self.push_log.len() >= self.config.rate_max {
            if let Some(&oldest) = self.push_log.front() {
                at = at.max(oldest + self.config.rate_window);
            }
        }
        at
    }

    fn heartbeat_at(&self, now: Instant) -> Instant {
        self.last_push.map_or(now, |t| t + self.config.heartbeat)
    }

    fn push_due(&mut self, now: Instant) -> bool {
        let wanted = self.dirty || now >= self.heartbeat_at(now);
        wanted && self.push_allowed_at(now) <= now
    }

    fn next_wait(&mut self) -> Duration {
        let now = Instant::now();
        let mut wait = Duration::from_secs(3600);
        if self.enabled && !self.blocked && self.client.is_none() {
            wait = wait.min(self.next_connect_at.saturating_duration_since(now));
        }
        if self.client.is_some() {
            let wanted_at = if self.dirty { now } else { self.heartbeat_at(now) };
            let due = wanted_at.max(self.push_allowed_at(now));
            wait = wait.min(due.saturating_duration_since(now));
        }
        wait
    }

    fn push(&mut self) {
        let now = Instant::now();
        let desired = self.desired.clone();
        let reply = match self.client.as_mut() {
            Some(client) => {
                let sent = match &desired {
                    Some(payload) => client.set_activity(payload.to_rpc()),
                    None => client.clear_activity(),
                };
                match sent {
                    Ok(()) => read_reply(client),
                    Err(e) => Reply::Lost(format!("Discord로 전송하지 못했습니다: {e}")),
                }
            }
            None => return,
        };
        self.push_log.push_back(now);
        self.last_push = Some(now);
        self.dirty = false;

        match reply {
            Reply::Ok => {
                let active = desired.is_some();
                self.update(|s| {
                    s.presence_active = active;
                    s.last_presence = Some(PresenceOutcome {
                        ok: true,
                        message: None,
                        at: now_ms(),
                    });
                });
            }
            Reply::Rejected(message) => {
                self.update(|s| {
                    s.presence_active = false;
                    s.last_presence = Some(PresenceOutcome {
                        ok: false,
                        message: Some(message),
                        at: now_ms(),
                    });
                });
            }
            Reply::Lost(message) => self.lost(message),
        }
    }

    /// The connection broke. The desired presence is kept and sent again once
    /// Discord is back.
    fn lost(&mut self, message: String) {
        self.close_client(false);
        self.next_connect_at = Instant::now() + self.config.reconnect_interval;
        self.update(|s| {
            s.state = ConnState::Disconnected;
            s.user = None;
            s.message = Some(message);
            s.presence_active = false;
        });
    }

    fn close_client(&mut self, clear: bool) {
        if let Some(mut client) = self.client.take() {
            if clear {
                let _ = client.clear_activity();
            }
            let _ = client.close();
        }
    }

    fn set_state(&self, state: ConnState, message: Option<String>) {
        {
            let current = lock(&self.status);
            if current.state == state && current.message == message {
                return;
            }
        }
        self.update(|s| {
            s.state = state;
            s.message = message;
        });
    }

    fn update(&self, change: impl FnOnce(&mut DiscordStatus)) {
        let snapshot = {
            let mut status = lock(&self.status);
            change(&mut status);
            status.updated_at = now_ms();
            status.clone()
        };
        (self.notify)(&snapshot);
    }
}

#[cfg(test)]
mod rate_tests {
    use super::*;

    fn worker(config: WorkerConfig) -> Worker {
        Worker::new(config, Arc::new(Mutex::new(DiscordStatus::default())), Box::new(|_| {}))
    }

    #[test]
    fn updates_beyond_the_rate_limit_wait_for_the_window_to_slide() {
        let mut w = worker(WorkerConfig {
            min_push_spacing: Duration::ZERO,
            rate_window: Duration::from_secs(20),
            rate_max: 2,
            ..WorkerConfig::default()
        });
        let t0 = Instant::now();
        w.push_log.push_back(t0);
        w.push_log.push_back(t0);
        assert_eq!(w.push_allowed_at(t0), t0 + Duration::from_secs(20));
    }

    #[test]
    fn updates_are_spaced_by_the_minimum_gap() {
        let mut w = worker(WorkerConfig::default());
        let t0 = Instant::now();
        w.last_push = Some(t0);
        assert_eq!(w.push_allowed_at(t0), t0 + Duration::from_secs(1));
    }

    #[test]
    fn old_entries_leave_the_window() {
        let mut w = worker(WorkerConfig {
            min_push_spacing: Duration::ZERO,
            rate_window: Duration::from_millis(50),
            rate_max: 1,
            ..WorkerConfig::default()
        });
        let past = Instant::now() - Duration::from_millis(200);
        w.push_log.push_back(past);
        let now = Instant::now();
        assert_eq!(w.push_allowed_at(now), now);
        assert!(w.push_log.is_empty());
    }
}

/// Integration tests against a fake Discord IPC server, so the whole worker
/// (socket discovery, handshake, replies, reconnects) runs without Discord.
#[cfg(all(test, unix))]
mod ipc_tests {
    use super::*;
    use std::{
        fs,
        io::{Read, Write},
        os::unix::net::{UnixListener, UnixStream},
        path::PathBuf,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    /// The crate finds the socket through process-wide env vars, so these
    /// tests cannot run concurrently.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[derive(Clone, Copy, PartialEq)]
    enum Behavior {
        Normal,
        RejectClientId,
        RejectActivity,
    }

    struct Fake {
        dir: tempfile::TempDir,
        behavior: Arc<Mutex<Behavior>>,
        received: Arc<Mutex<Vec<Value>>>,
        handshakes: Arc<Mutex<Vec<Value>>>,
        connections: Arc<AtomicUsize>,
        generation: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
    }

    fn read_frame(stream: &mut UnixStream) -> Option<(u32, Value)> {
        let mut header = [0u8; 8];
        stream.read_exact(&mut header).ok()?;
        let op = u32::from_le_bytes(header[0..4].try_into().unwrap());
        let len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).ok()?;
        Some((op, serde_json::from_slice(&body).ok()?))
    }

    fn write_frame(stream: &mut UnixStream, op: u32, value: &Value) {
        let body = value.to_string();
        let mut frame = op.to_le_bytes().to_vec();
        frame.extend((body.len() as u32).to_le_bytes());
        frame.extend(body.as_bytes());
        let _ = stream.write_all(&frame);
    }

    impl Fake {
        /// Points the crate at a private directory where Discord is "not
        /// running": `discord-ipc-0` exists but is not a socket, so connecting
        /// fails, and the search never falls through to a real Discord.
        ///
        /// Only `XDG_RUNTIME_DIR` is changed. The crate consults it first and
        /// `tempfile` never reads it; changing `TMPDIR` would break every
        /// other test that creates a temporary directory.
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join("discord-ipc-0"), b"").unwrap();
            // SAFETY: serialised by ENV_LOCK; no other thread reads this variable concurrently.
            unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
            Self {
                dir,
                behavior: Arc::new(Mutex::new(Behavior::Normal)),
                received: Arc::new(Mutex::new(Vec::new())),
                handshakes: Arc::new(Mutex::new(Vec::new())),
                connections: Arc::new(AtomicUsize::new(0)),
                generation: Arc::new(AtomicUsize::new(0)),
                stop: Arc::new(AtomicBool::new(false)),
            }
        }

        fn socket_path(&self) -> PathBuf {
            self.dir.path().join("discord-ipc-0")
        }

        /// Discord "starts": the socket appears.
        fn start(&self) {
            let _ = fs::remove_file(self.socket_path());
            let listener = UnixListener::bind(self.socket_path()).unwrap();
            listener.set_nonblocking(true).unwrap();
            let (behavior, received, handshakes) =
                (self.behavior.clone(), self.received.clone(), self.handshakes.clone());
            let (connections, generation, stop) =
                (self.connections.clone(), self.generation.clone(), self.stop.clone());
            thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            connections.fetch_add(1, Ordering::SeqCst);
                            let my_generation = generation.load(Ordering::SeqCst);
                            let (behavior, received, handshakes) =
                                (behavior.clone(), received.clone(), handshakes.clone());
                            let (generation, stop) = (generation.clone(), stop.clone());
                            thread::spawn(move || {
                                serve(stream, my_generation, behavior, received, handshakes, generation, stop)
                            });
                        }
                        Err(_) => thread::sleep(Duration::from_millis(10)),
                    }
                }
            });
        }

        /// Discord "quits": every open connection is closed.
        fn kill_connections(&self) {
            self.generation.fetch_add(1, Ordering::SeqCst);
        }

        fn set_behavior(&self, b: Behavior) {
            *lock(&self.behavior) = b;
        }

        /// Every SET_ACTIVITY frame, including "clear" ones (a null activity).
        fn activities(&self) -> Vec<Value> {
            lock(&self.received)
                .iter()
                .filter(|f| f["cmd"] == "SET_ACTIVITY")
                .cloned()
                .collect()
        }

        /// Only the frames that actually set an activity. While no activity is
        /// wanted, the heartbeat sends harmless clears that may arrive first.
        fn shown(&self) -> Vec<Value> {
            self.activities()
                .into_iter()
                .filter(|f| !f["args"]["activity"].is_null())
                .collect()
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    fn serve(
        mut stream: UnixStream,
        my_generation: usize,
        behavior: Arc<Mutex<Behavior>>,
        received: Arc<Mutex<Vec<Value>>>,
        handshakes: Arc<Mutex<Vec<Value>>>,
        generation: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
    ) {
        let _ = stream.set_nonblocking(false);
        let Some((0, handshake)) = read_frame(&mut stream) else {
            return;
        };
        lock(&handshakes).push(handshake);
        if *lock(&behavior) == Behavior::RejectClientId {
            write_frame(&mut stream, 2, &json!({"code": 4000, "message": "Invalid Client ID"}));
            return;
        }
        write_frame(
            &mut stream,
            1,
            &json!({"cmd":"DISPATCH","evt":"READY","nonce":null,
                    "data":{"v":1,"user":{"id":"42","username":"tester","global_name":"Tester","avatar":null}}}),
        );
        let _ = stream.set_read_timeout(Some(Duration::from_millis(30)));
        loop {
            if stop.load(Ordering::SeqCst) || generation.load(Ordering::SeqCst) != my_generation {
                let _ = stream.shutdown(std::net::Shutdown::Both);
                return;
            }
            let mut header = [0u8; 8];
            match stream.read_exact(&mut header) {
                Ok(()) => {}
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    continue
                }
                Err(_) => return,
            }
            let op = u32::from_le_bytes(header[0..4].try_into().unwrap());
            let len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
            let mut body = vec![0u8; len];
            let _ = stream.set_read_timeout(None);
            if stream.read_exact(&mut body).is_err() {
                return;
            }
            let _ = stream.set_read_timeout(Some(Duration::from_millis(30)));
            let frame: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            match op {
                2 => return,
                1 => {
                    let nonce = frame["nonce"].clone();
                    lock(&received).push(frame);
                    if *lock(&behavior) == Behavior::RejectActivity {
                        write_frame(
                            &mut stream,
                            1,
                            &json!({"cmd":"SET_ACTIVITY","evt":"ERROR","nonce":nonce,
                                    "data":{"code":4000,"message":"Invalid activity"}}),
                        );
                    } else {
                        write_frame(
                            &mut stream,
                            1,
                            &json!({"cmd":"SET_ACTIVITY","evt":null,"nonce":nonce,"data":{}}),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn fast() -> WorkerConfig {
        WorkerConfig {
            reconnect_interval: Duration::from_millis(80),
            heartbeat: Duration::from_millis(120),
            min_push_spacing: Duration::from_millis(5),
            rate_window: Duration::from_secs(1),
            rate_max: 100,
        }
    }

    fn payload(details: &str) -> ActivityPayload {
        ActivityPayload {
            name: Some("Pokémon Legends: Z-A".into()),
            details: Some(details.into()),
            state: Some("Lumiose City".into()),
            large_image: Some("pokemon-za".into()),
            large_text: Some("Pokémon Legends: Z-A".into()),
            small_image: None,
            small_text: None,
            start_ms: Some(1_762_646_400_000),
        }
    }

    fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if cond() {
                return;
            }
            thread::sleep(Duration::from_millis(15));
        }
        panic!("timed out waiting for: {what}");
    }

    const APP_ID: &str = "123456789012345678";

    #[test]
    fn handshake_reads_ready_and_activity_reaches_discord() {
        let _guard = lock(&ENV_LOCK);
        let fake = Fake::new();
        fake.start();
        let handle = DiscordHandle::spawn(fast(), |_| {});

        handle.connect(APP_ID);
        wait_until("connected", || handle.status().state == ConnState::Connected);
        let status = handle.status();
        assert_eq!(status.user.as_ref().unwrap().username, "tester");
        assert_eq!(status.application_id.as_deref(), Some(APP_ID));
        assert_eq!(lock(&fake.handshakes)[0]["client_id"], APP_ID);
        assert_eq!(lock(&fake.handshakes)[0]["v"], 1);

        handle.set_activity(Some(payload("Story Progress")));
        wait_until("activity received", || !fake.shown().is_empty());
        let frames = fake.shown();
        let frame = &frames[0];
        let activity = &frame["args"]["activity"];
        assert_eq!(activity["name"], "Pokémon Legends: Z-A");
        assert_eq!(activity["details"], "Story Progress");
        assert_eq!(activity["state"], "Lumiose City");
        assert_eq!(activity["assets"]["large_image"], "pokemon-za");
        assert_eq!(activity["timestamps"]["start"], 1_762_646_400);
        assert_eq!(frame["cmd"], "SET_ACTIVITY");
        assert!(frame["args"]["pid"].as_u64().is_some());

        wait_until("outcome recorded", || {
            let s = handle.status();
            s.presence_active && s.last_presence.as_ref().is_some_and(|p| p.ok)
        });
        handle.shutdown(Duration::from_secs(2));
    }

    #[test]
    fn reconnects_and_restores_the_presence_after_discord_restarts() {
        let _guard = lock(&ENV_LOCK);
        let fake = Fake::new();
        fake.start();
        let handle = DiscordHandle::spawn(fast(), |_| {});
        handle.connect(APP_ID);
        handle.set_activity(Some(payload("Story Progress")));
        wait_until("first activity", || !fake.shown().is_empty());
        let before = fake.shown().len();

        fake.kill_connections();
        wait_until("disconnect detected", || handle.status().state == ConnState::Disconnected);
        assert!(!handle.status().presence_active);

        wait_until("reconnected", || handle.status().state == ConnState::Connected);
        wait_until("presence restored", || fake.shown().len() > before);
        assert!(fake.connections.load(Ordering::SeqCst) >= 2);
        assert_eq!(
            fake.shown().last().unwrap()["args"]["activity"]["details"],
            "Story Progress"
        );
        handle.shutdown(Duration::from_secs(2));
    }

    #[test]
    fn waits_for_discord_to_start_and_connects_by_itself() {
        let _guard = lock(&ENV_LOCK);
        let fake = Fake::new();
        let handle = DiscordHandle::spawn(fast(), |_| {});
        handle.set_activity(Some(payload("Story Progress")));
        handle.connect(APP_ID);

        wait_until("reported as unreachable", || {
            handle.status().state == ConnState::Disconnected
        });
        assert!(handle.status().message.unwrap().contains("Discord"));

        fake.start();
        wait_until("connected once Discord appears", || {
            handle.status().state == ConnState::Connected
        });
        wait_until("presence sent", || !fake.shown().is_empty());
        handle.shutdown(Duration::from_secs(2));
    }

    #[test]
    fn invalid_application_id_is_reported_and_not_retried() {
        let _guard = lock(&ENV_LOCK);
        let fake = Fake::new();
        fake.start();
        fake.set_behavior(Behavior::RejectClientId);
        let handle = DiscordHandle::spawn(fast(), |_| {});

        handle.connect("999999999999999999");
        wait_until("error state", || handle.status().state == ConnState::Error);
        assert!(handle.status().message.unwrap().contains("Application ID"));

        thread::sleep(Duration::from_millis(400));
        assert_eq!(
            fake.connections.load(Ordering::SeqCst),
            1,
            "an invalid id must not be retried in a loop"
        );

        // Fixing the id (an explicit Connect) unblocks the worker.
        fake.set_behavior(Behavior::Normal);
        handle.connect(APP_ID);
        wait_until("connected after fixing the id", || {
            handle.status().state == ConnState::Connected
        });
        handle.shutdown(Duration::from_secs(2));
    }

    #[test]
    fn a_rejected_activity_is_reported_but_keeps_the_connection() {
        let _guard = lock(&ENV_LOCK);
        let fake = Fake::new();
        fake.start();
        fake.set_behavior(Behavior::RejectActivity);
        let handle = DiscordHandle::spawn(fast(), |_| {});
        handle.connect(APP_ID);
        handle.set_activity(Some(payload("Story Progress")));

        wait_until("rejection recorded", || {
            handle.status().last_presence.is_some_and(|p| !p.ok)
        });
        let status = handle.status();
        assert_eq!(status.state, ConnState::Connected);
        assert!(!status.presence_active);
        assert!(status
            .last_presence
            .unwrap()
            .message
            .unwrap()
            .contains("Invalid activity"));
        handle.shutdown(Duration::from_secs(2));
    }

    #[test]
    fn clearing_and_disconnecting_stop_the_presence_and_the_retries() {
        let _guard = lock(&ENV_LOCK);
        let fake = Fake::new();
        fake.start();
        let handle = DiscordHandle::spawn(fast(), |_| {});
        handle.connect(APP_ID);
        handle.set_activity(Some(payload("Story Progress")));
        wait_until("activity set", || handle.status().presence_active);

        handle.set_activity(None);
        wait_until("presence cleared", || !handle.status().presence_active);
        assert!(
            fake.activities().last().unwrap()["args"]["activity"].is_null(),
            "clearing sends SET_ACTIVITY with a null activity"
        );

        handle.disconnect();
        wait_until("idle", || handle.status().state == ConnState::Idle);
        let connections = fake.connections.load(Ordering::SeqCst);
        thread::sleep(Duration::from_millis(400));
        assert_eq!(fake.connections.load(Ordering::SeqCst), connections, "no reconnect after disconnect");
        handle.shutdown(Duration::from_secs(2));
    }

    #[test]
    fn status_changes_are_pushed_to_the_notifier() {
        let _guard = lock(&ENV_LOCK);
        let fake = Fake::new();
        fake.start();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let handle = DiscordHandle::spawn(fast(), move |s| lock(&sink).push(s.state));
        handle.connect(APP_ID);
        wait_until("connected", || handle.status().state == ConnState::Connected);
        let states = lock(&seen).clone();
        assert_eq!(states.first(), Some(&ConnState::Connecting));
        assert_eq!(states.last(), Some(&ConnState::Connected));
        handle.shutdown(Duration::from_secs(2));
    }

    #[test]
    fn probe_reports_the_user_without_leaving_a_presence() {
        let _guard = lock(&ENV_LOCK);
        let fake = Fake::new();
        fake.start();
        let user = probe(APP_ID).unwrap();
        assert_eq!(user.username, "tester");
        assert!(fake.activities().is_empty());

        fake.set_behavior(Behavior::RejectClientId);
        let err = probe("999999999999999999").unwrap_err();
        assert!(err.contains("Application ID"));
    }
}
