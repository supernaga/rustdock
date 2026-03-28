use rustdock_core::domain::{now_timestamp, AuthMethod, SessionProfile, SessionSyncState};
use rustdock_core::runtime::AppRuntime;
use rustdock_core::ssh::{
    RusshSessionService, SessionEvent, SessionService, SessionStatus, TerminalSize,
};
use std::env;
use std::thread;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = required_env("SSH_SMOKE_HOST")?;
    let username = required_env("SSH_SMOKE_USER")?;
    let password = required_env("SSH_SMOKE_PASSWORD")?;
    let port = env::var("SSH_SMOKE_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(22);

    let profile = SessionProfile {
        id: "smoke-session".to_string(),
        name: "Smoke Session".to_string(),
        host,
        port,
        username,
        auth_method: AuthMethod::Password,
        tags: vec!["smoke".to_string()],
        notes: "Transient smoke test profile".to_string(),
        local_roots: Vec::new(),
        remote_roots: Vec::new(),
        created_at: now_timestamp(),
        updated_at: now_timestamp(),
        last_connected_at: None,
        sync_state: SessionSyncState::LocalOnly,
    };

    let runtime = AppRuntime::new()?;
    let service = RusshSessionService::new(runtime.handle());
    let active = service.connect(&profile, TerminalSize::default(), Some(password))?;
    let controller = active.controller();

    let mut transcript = String::new();
    let mut sent_probe = false;
    let mut saw_connected = false;
    let marker = "__RUSTDOCK_RUSSH_OK__";
    let deadline = Instant::now() + Duration::from_secs(20);

    while Instant::now() < deadline {
        match active.try_recv() {
            Ok(SessionEvent::StatusChanged(SessionStatus::Connected)) => {
                saw_connected = true;
                println!("connected");
            }
            Ok(SessionEvent::StatusChanged(status)) => {
                println!("status={}", status.as_label());
            }
            Ok(SessionEvent::Output(chunk)) => {
                print!("{chunk}");
                transcript.push_str(&chunk);
                if transcript.contains(marker) {
                    controller.disconnect()?;
                    println!("\nsmoke test passed");
                    return Ok(());
                }
            }
            Ok(SessionEvent::Closed) => break,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
        }

        if saw_connected && !sent_probe {
            controller.send_input(format!("printf '{}\\n'\n", marker))?;
            sent_probe = true;
        }

        thread::sleep(Duration::from_millis(100));
    }

    controller.disconnect().ok();
    Err(format!(
        "smoke test did not observe marker before timeout; transcript:\n{}",
        transcript
    )
    .into())
}

fn required_env(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    env::var(name)
        .map_err(|_| format!("missing required environment variable {name}").into())
}
