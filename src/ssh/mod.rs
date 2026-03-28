use crate::domain::SessionProfile;
use anyhow::{anyhow, Context, Result};
use russh::client::{self, Handle as RusshHandle};
use russh::keys::agent::client::AgentClient;
use russh::keys::{self, HashAlg, PrivateKeyWithHashAlg, PublicKey};
use russh::ChannelMsg;
use serde::Serialize;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use tokio::io::AsyncWriteExt;
use tokio::runtime::Handle as RuntimeHandle;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::time::{self, Duration, MissedTickBehavior};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self { cols: 120, rows: 32 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum SessionStatus {
    Idle,
    Connecting,
    Connected,
    Disconnected,
    Failed,
}

impl SessionStatus {
    pub fn as_label(&self) -> &'static str {
        match self {
            SessionStatus::Idle => "Idle",
            SessionStatus::Connecting => "Connecting",
            SessionStatus::Connected => "Connected",
            SessionStatus::Disconnected => "Disconnected",
            SessionStatus::Failed => "Failed",
        }
    }
}

#[derive(Debug)]
pub enum SessionEvent {
    StatusChanged(SessionStatus),
    Output(String),
    Closed,
}

enum SessionCommand {
    SendInput(String),
    Resize(TerminalSize),
    Disconnect,
}

#[derive(Clone)]
pub struct SessionController {
    session_id: String,
    session_name: String,
    commands: tokio_mpsc::UnboundedSender<SessionCommand>,
}

impl SessionController {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn session_name(&self) -> &str {
        &self.session_name
    }

    pub fn send_input(&self, input: String) -> Result<(), String> {
        self.commands
            .send(SessionCommand::SendInput(input))
            .map_err(|_| "session command channel is closed".to_string())
    }

    pub fn resize(&self, size: TerminalSize) -> Result<(), String> {
        self.commands
            .send(SessionCommand::Resize(size))
            .map_err(|_| "session command channel is closed".to_string())
    }

    pub fn disconnect(&self) -> Result<(), String> {
        self.commands
            .send(SessionCommand::Disconnect)
            .map_err(|_| "session command channel is closed".to_string())
    }
}

pub struct ActiveSession {
    controller: SessionController,
    events: Receiver<SessionEvent>,
}

impl ActiveSession {
    pub fn controller(&self) -> SessionController {
        self.controller.clone()
    }

    pub fn try_recv(&self) -> Result<SessionEvent, TryRecvError> {
        self.events.try_recv()
    }

    pub fn split(self) -> (SessionController, Receiver<SessionEvent>) {
        (self.controller, self.events)
    }
}

pub trait SessionService {
    fn connect(
        &self,
        profile: &SessionProfile,
        size: TerminalSize,
        secret: Option<String>,
    ) -> Result<ActiveSession, String>;
}

#[derive(Clone)]
pub struct MockSessionService {
    handle: RuntimeHandle,
}

impl MockSessionService {
    pub fn new(handle: RuntimeHandle) -> Self {
        Self { handle }
    }
}

impl SessionService for MockSessionService {
    fn connect(
        &self,
        profile: &SessionProfile,
        size: TerminalSize,
        _secret: Option<String>,
    ) -> Result<ActiveSession, String> {
        let (event_tx, event_rx) = mpsc::channel();
        let (command_tx, mut command_rx) = tokio_mpsc::unbounded_channel();

        let session_id = profile.id.clone();
        let session_name = profile.name.clone();
        let host = profile.host.clone();
        let port = profile.port;
        let username = profile.username.clone();

        self.handle.spawn(async move {
            let _ = event_tx.send(SessionEvent::StatusChanged(SessionStatus::Connecting));
            time::sleep(Duration::from_millis(350)).await;

            if event_tx
                .send(SessionEvent::StatusChanged(SessionStatus::Connected))
                .is_err()
            {
                return;
            }

            if send_output(
                &event_tx,
                format!(
                    "Connected to {}@{}:{}\r\n",
                    username,
                    host,
                    port
                ),
            )
            .is_err()
            {
                return;
            }

            if send_output(
                &event_tx,
                format!(
                    "PTY requested: TERM=xterm-256color {}x{}\r\n",
                    size.cols,
                    size.rows
                ),
            )
            .is_err()
            {
                return;
            }

            if send_output(
                &event_tx,
                "Mock transport active. This will be replaced by a real russh-backed session.\r\n".to_string(),
            )
            .is_err()
            {
                return;
            }

            let mut heartbeat = time::interval(Duration::from_secs(15));
            heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    _ = heartbeat.tick() => {
                        if send_output(&event_tx, "[keepalive tick]\r\n".to_string()).is_err() {
                            break;
                        }
                    }
                    command = command_rx.recv() => {
                        match command {
                            Some(SessionCommand::SendInput(input)) => {
                                let line = input.trim().to_string();
                                let rendered = if line.is_empty() {
                                    "[empty input]".to_string()
                                } else {
                                    line
                                };
                                if send_output(
                                    &event_tx,
                                    format!(
                                        "$ {}\r\nmock-exec: command accepted by the session layer\r\n",
                                        rendered
                                    ),
                                ).is_err() {
                                    break;
                                }
                            }
                            Some(SessionCommand::Resize(next_size)) => {
                                if send_output(
                                    &event_tx,
                                    format!(
                                        "[window-change {}x{}]\r\n",
                                        next_size.cols,
                                        next_size.rows
                                    ),
                                ).is_err() {
                                    break;
                                }
                            }
                            Some(SessionCommand::Disconnect) | None => {
                                let _ = send_output(&event_tx, "[session closed]\r\n".to_string());
                                let _ = event_tx.send(SessionEvent::StatusChanged(SessionStatus::Disconnected));
                                let _ = event_tx.send(SessionEvent::Closed);
                                break;
                            }
                        }
                    }
                }
            }
        });

        let controller = SessionController {
            session_id,
            session_name,
            commands: command_tx,
        };

        Ok(ActiveSession {
            controller,
            events: event_rx,
        })
    }
}

fn send_output(sender: &mpsc::Sender<SessionEvent>, text: String) -> Result<(), ()> {
    sender.send(SessionEvent::Output(text)).map_err(|_| ())
}

#[derive(Clone)]
pub struct RusshSessionService {
    handle: RuntimeHandle,
}

impl RusshSessionService {
    pub fn new(handle: RuntimeHandle) -> Self {
        Self { handle }
    }
}

impl SessionService for RusshSessionService {
    fn connect(
        &self,
        profile: &SessionProfile,
        size: TerminalSize,
        secret: Option<String>,
    ) -> Result<ActiveSession, String> {
        let (event_tx, event_rx) = mpsc::channel();
        let (command_tx, command_rx) = tokio_mpsc::unbounded_channel();

        let controller = SessionController {
            session_id: profile.id.clone(),
            session_name: profile.name.clone(),
            commands: command_tx,
        };

        let profile = profile.clone();
        let runtime = self.handle.clone();
        self.handle.spawn(async move {
            if let Err(error) = run_russh_session(runtime, profile, size, secret, event_tx.clone(), command_rx).await {
                let _ = event_tx.send(SessionEvent::StatusChanged(SessionStatus::Failed));
                let _ = send_output(&event_tx, format!("[error] {error:#}\r\n"));
                let _ = event_tx.send(SessionEvent::Closed);
            }
        });

        Ok(ActiveSession {
            controller,
            events: event_rx,
        })
    }
}

async fn run_russh_session(
    runtime: RuntimeHandle,
    profile: SessionProfile,
    size: TerminalSize,
    secret: Option<String>,
    event_tx: mpsc::Sender<SessionEvent>,
    mut command_rx: tokio_mpsc::UnboundedReceiver<SessionCommand>,
) -> Result<()> {
    let _ = event_tx.send(SessionEvent::StatusChanged(SessionStatus::Connecting));

    let handler = TrustOnFirstUseHandler {
        host: profile.host.clone(),
        port: profile.port,
        event_tx: Some(event_tx.clone()),
    };
    let session = connect_authenticated(profile.clone(), secret.as_deref(), handler).await?;

    let channel = session
        .channel_open_session()
        .await
        .context("failed to open SSH session channel")?;

    let (mut reader, writer) = channel.split();
    writer
        .request_pty(
            true,
            "xterm-256color",
            u32::from(size.cols),
            u32::from(size.rows),
            0,
            0,
            &[],
        )
        .await
        .context("failed to request PTY")?;
    writer
        .request_shell(true)
        .await
        .context("failed to request remote shell")?;

    let _ = event_tx.send(SessionEvent::StatusChanged(SessionStatus::Connected));
    let _ = send_output(
        &event_tx,
        format!(
            "SSH connected to {}@{}:{}\r\n",
            profile.username, profile.host, profile.port
        ),
    );

    let _connection_driver = runtime.spawn(async move {
        let _ = session.await;
    });

    loop {
        tokio::select! {
            command = command_rx.recv() => {
                match command {
                    Some(SessionCommand::SendInput(input)) => {
                        let mut writer_handle = writer.make_writer();
                        writer_handle
                            .write_all(input.as_bytes())
                            .await
                            .context("failed to send channel data")?;
                        writer_handle.flush().await.context("failed to flush channel data")?;
                    }
                    Some(SessionCommand::Resize(next_size)) => {
                        writer
                            .window_change(
                                u32::from(next_size.cols),
                                u32::from(next_size.rows),
                                0,
                                0,
                            )
                            .await
                            .context("failed to resize PTY")?;
                    }
                    Some(SessionCommand::Disconnect) | None => {
                        let _ = writer.eof().await;
                        let _ = writer.close().await;
                        break;
                    }
                }
            }
            message = reader.wait() => {
                match message {
                    Some(ChannelMsg::Data { data }) => {
                        let _ = send_output(&event_tx, String::from_utf8_lossy(&data).to_string());
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        let _ = send_output(&event_tx, String::from_utf8_lossy(&data).to_string());
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        let _ = send_output(
                            &event_tx,
                            format!("\r\n[remote exited with status {exit_status}]\r\n"),
                        );
                    }
                    Some(ChannelMsg::ExitSignal { signal_name, error_message, .. }) => {
                        let _ = send_output(
                            &event_tx,
                            format!("\r\n[remote signal {:?}: {}]\r\n", signal_name, error_message),
                        );
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                        break;
                    }
                    Some(ChannelMsg::Failure) => {
                        let _ = send_output(&event_tx, "\r\n[channel request failed]\r\n".to_string());
                    }
                    Some(ChannelMsg::Success) => {}
                    Some(_) => {}
                }
            }
        }
    }

    let _ = event_tx.send(SessionEvent::StatusChanged(SessionStatus::Disconnected));
    let _ = event_tx.send(SessionEvent::Closed);
    Ok(())
}

async fn authenticate(
    session: &mut RusshHandle<TrustOnFirstUseHandler>,
    profile: &SessionProfile,
    secret: Option<&str>,
) -> Result<()> {
    match &profile.auth_method {
        crate::domain::AuthMethod::Password => {
            let password = secret
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow!("password is required for password authentication"))?;
            let result = session
                .authenticate_password(profile.username.clone(), password.to_string())
                .await
                .context("password authentication request failed")?;
            if result.success() {
                Ok(())
            } else {
                Err(anyhow!("server rejected password authentication"))
            }
        }
        crate::domain::AuthMethod::PrivateKey { path } => {
            let key = keys::load_secret_key(path, secret)
                .with_context(|| format!("failed to load private key from {path}"))?;
            let hash_alg = if key.algorithm().is_rsa() {
                session
                    .best_supported_rsa_hash()
                    .await
                    .context("failed to negotiate RSA hash algorithm")?
                    .flatten()
                    .or(Some(HashAlg::Sha512))
            } else {
                None
            };

            let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);
            let result = session
                .authenticate_publickey(profile.username.clone(), key)
                .await
                .context("public key authentication request failed")?;

            if result.success() {
                Ok(())
            } else {
                Err(anyhow!("server rejected public key authentication"))
            }
        }
        crate::domain::AuthMethod::Agent => authenticate_with_agent(session, &profile.username).await,
    }
}

fn client_config() -> client::Config {
    client::Config {
        inactivity_timeout: Some(Duration::from_secs(300)),
        keepalive_interval: Some(Duration::from_secs(15)),
        keepalive_max: 3,
        ..Default::default()
    }
}

async fn authenticate_with_agent(
    session: &mut RusshHandle<TrustOnFirstUseHandler>,
    username: &str,
) -> Result<()> {
    #[cfg(unix)]
    let mut agent = AgentClient::connect_env()
        .await
        .context("failed to connect to ssh-agent via SSH_AUTH_SOCK")?;

    #[cfg(windows)]
    let mut agent = AgentClient::connect_pageant()
        .await
        .context("failed to connect to Pageant agent")?;

    #[cfg(not(any(unix, windows)))]
    {
        return Err(anyhow!("SSH agent authentication is not supported on this platform"));
    }

    let identities = agent
        .request_identities()
        .await
        .context("failed to request identities from SSH agent")?;

    if identities.is_empty() {
        return Err(anyhow!("SSH agent has no loaded identities"));
    }

    let rsa_hash = session
        .best_supported_rsa_hash()
        .await
        .context("failed to negotiate RSA hash algorithm")?
        .flatten()
        .or(Some(HashAlg::Sha512));

    for identity in identities {
        let hash_alg = if identity.algorithm().is_rsa() {
            rsa_hash
        } else {
            None
        };

        let result = session
            .authenticate_publickey_with(username.to_string(), identity, hash_alg, &mut agent)
            .await
            .map_err(|error| anyhow!(error.to_string()))?;

        if result.success() {
            return Ok(());
        }
    }

    Err(anyhow!("SSH agent identities were rejected by the server"))
}

pub(crate) async fn connect_authenticated(
    profile: SessionProfile,
    secret: Option<&str>,
    handler: TrustOnFirstUseHandler,
) -> Result<RusshHandle<TrustOnFirstUseHandler>> {
    let config = Arc::new(client_config());
    let address = format!("{}:{}", profile.host, profile.port);
    let mut session = client::connect(config, address, handler)
        .await
        .context("failed to open SSH transport")?;

    authenticate(&mut session, &profile, secret)
        .await
        .context("authentication failed")?;

    Ok(session)
}

#[derive(Clone)]
pub(crate) struct TrustOnFirstUseHandler {
    host: String,
    port: u16,
    event_tx: Option<mpsc::Sender<SessionEvent>>,
}

impl TrustOnFirstUseHandler {
    pub(crate) fn new(
        host: String,
        port: u16,
        event_tx: Option<mpsc::Sender<SessionEvent>>,
    ) -> Self {
        Self { host, port, event_tx }
    }
}

impl client::Handler for TrustOnFirstUseHandler {
    type Error = anyhow::Error;

    async fn check_server_key(&mut self, server_public_key: &PublicKey) -> Result<bool> {
        match keys::check_known_hosts(&self.host, self.port, server_public_key) {
            Ok(true) => Ok(true),
            Ok(false) => {
                keys::known_hosts::learn_known_hosts(&self.host, self.port, server_public_key)
                    .context("failed to record host key in known_hosts")?;
                emit_optional(
                    &self.event_tx,
                    format!(
                        "[known_hosts] learned host key for {}:{} (trust on first use)\r\n",
                        self.host, self.port
                    ),
                );
                Ok(true)
            }
            Err(error) => {
                emit_optional(
                    &self.event_tx,
                    format!(
                        "[known_hosts] rejected host key for {}:{}: {error}\r\n",
                        self.host, self.port
                    ),
                );
                Err(error.into())
            }
        }
    }

    async fn disconnected(
        &mut self,
        reason: client::DisconnectReason<Self::Error>,
    ) -> Result<()> {
        emit_optional(&self.event_tx, format!("[disconnect] {:?}\r\n", reason));
        Ok(())
    }
}

fn emit_optional(sender: &Option<mpsc::Sender<SessionEvent>>, text: String) {
    if let Some(sender) = sender {
        let _ = send_output(sender, text);
    }
}
