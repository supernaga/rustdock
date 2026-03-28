#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use russh::keys::known_hosts::known_host_keys;
use rustdock_core::domain::{
    generate_session_id, now_timestamp, AuthMethod, SessionProfile, SessionSyncState, TransferEventRecord,
    TransferJobRecord,
};
use rustdock_core::runtime::AppRuntime;
use rustdock_core::sftp::{
    create_remote_dir, delete_remote_path, download_remote_file, list_remote_dir, rename_remote_path,
    upload_local_file, MutationResult, RemoteDirectoryListing, TransferProgress, TransferResult,
    download_remote_file_with_progress, upload_local_file_with_progress,
};
use rustdock_core::ssh::{
    RusshSessionService, SessionController, SessionEvent, SessionService, SessionStatus,
    TerminalSize,
};
use rustdock_core::storage::{SessionRepository, SqliteSessionStore};
use std::fs;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::image::Image;
use tauri::ipc::Channel;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};
use tokio::sync::watch;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use keyring::{Entry, Error as KeyringError};

const SECRET_SERVICE: &str = "rustdock/session-secret";

#[derive(Clone)]
struct AppState {
    store: SqliteSessionStore,
    _runtime: Arc<AppRuntime>,
    session_service: RusshSessionService,
    controllers: Arc<Mutex<HashMap<String, SessionController>>>,
    transfer_controls: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
    background_on_close: Arc<AtomicBool>,
    quitting: Arc<AtomicBool>,
}

impl AppState {
    fn new() -> Result<Self, String> {
        let store = SqliteSessionStore::open_default()?;
        let runtime = Arc::new(AppRuntime::new()?);
        let session_service = RusshSessionService::new(runtime.handle());

        Ok(Self {
            store,
            _runtime: runtime,
            session_service,
            controllers: Arc::new(Mutex::new(HashMap::new())),
            transfer_controls: Arc::new(Mutex::new(HashMap::new())),
            background_on_close: Arc::new(AtomicBool::new(true)),
            quitting: Arc::new(AtomicBool::new(false)),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum AuthMethodPayload {
    Password,
    PrivateKey { path: String },
    Agent,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SessionDraftPayload {
    id: Option<String>,
    name: String,
    host: String,
    port: u16,
    username: String,
    auth_method: AuthMethodPayload,
    tags: Vec<String>,
    notes: String,
    local_roots: Vec<String>,
    remote_roots: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct TerminalConnectionPayload {
    session_id: String,
    session_name: String,
}

#[derive(Clone, Debug, Deserialize)]
struct SftpRequestPayload {
    session_id: String,
    path: String,
    secret: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct DownloadRequestPayload {
    session_id: String,
    remote_path: String,
    local_path: String,
    secret: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct UploadRequestPayload {
    session_id: String,
    local_path: String,
    remote_path: String,
    secret: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct RemoteDirectoryMutationPayload {
    session_id: String,
    remote_path: String,
    secret: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct RemoteRenamePayload {
    session_id: String,
    source_path: String,
    target_path: String,
    secret: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct RemoteDeletePayload {
    session_id: String,
    remote_path: String,
    is_dir: bool,
    secret: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum TransferKindPayload {
    Upload,
    Download,
}

#[derive(Clone, Debug, Deserialize)]
struct TransferStartPayload {
    job_id: String,
    kind: TransferKindPayload,
    session_id: String,
    local_path: String,
    remote_path: String,
    secret: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct CancelTransferPayload {
    job_id: String,
}

#[derive(Clone, Debug, Serialize)]
struct KnownHostEntryPayload {
    line: usize,
    hosts: String,
    key_type: String,
    hashed: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum TerminalStreamMessage {
    Status { status: SessionStatus },
    Output { data: String },
    Closed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum TransferStreamMessage {
    Started { job_id: String },
    Progress {
        job_id: String,
        transferred: u64,
        total: Option<u64>,
    },
    Completed {
        job_id: String,
        bytes: u64,
        path: String,
    },
    Failed {
        job_id: String,
        error: String,
    },
    Cancelled { job_id: String },
}

#[tauri::command]
fn list_sessions(state: tauri::State<'_, AppState>) -> Result<Vec<SessionProfile>, String> {
    state.store.list_sessions()
}

#[tauri::command]
fn list_transfer_jobs(state: tauri::State<'_, AppState>) -> Result<Vec<TransferJobRecord>, String> {
    state.store.list_transfer_jobs()
}

#[tauri::command]
fn save_transfer_job(
    state: tauri::State<'_, AppState>,
    job: TransferJobRecord,
) -> Result<TransferJobRecord, String> {
    state.store.save_transfer_job(&job)?;
    Ok(job)
}

#[tauri::command]
fn delete_transfer_job(
    state: tauri::State<'_, AppState>,
    job_id: String,
) -> Result<(), String> {
    state.store.delete_transfer_job(&job_id)
}

#[tauri::command]
fn list_transfer_events(
    state: tauri::State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<TransferEventRecord>, String> {
    state.store.list_transfer_events(limit.unwrap_or(200))
}

#[tauri::command]
fn save_transfer_event(
    state: tauri::State<'_, AppState>,
    event: TransferEventRecord,
) -> Result<TransferEventRecord, String> {
    state.store.save_transfer_event(&event)?;
    Ok(event)
}

#[tauri::command]
fn clear_transfer_events(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.store.clear_transfer_events()
}

#[tauri::command]
fn save_session(
    state: tauri::State<'_, AppState>,
    draft: SessionDraftPayload,
) -> Result<SessionProfile, String> {
    validate_session_draft(&draft)?;

    let existing = match draft.id.as_ref() {
        Some(session_id) => state.store.find_session(session_id)?,
        None => None,
    };

    let session = SessionProfile {
        id: draft.id.unwrap_or_else(generate_session_id),
        name: draft.name.trim().to_string(),
        host: draft.host.trim().to_string(),
        port: draft.port,
        username: draft.username.trim().to_string(),
        auth_method: map_auth_method(draft.auth_method)?,
        tags: normalize_list(draft.tags),
        notes: draft.notes.trim().to_string(),
        local_roots: normalize_list(draft.local_roots),
        remote_roots: normalize_list(draft.remote_roots),
        created_at: existing
            .as_ref()
            .map(|session| session.created_at)
            .unwrap_or_else(now_timestamp),
        updated_at: now_timestamp(),
        last_connected_at: existing.as_ref().and_then(|session| session.last_connected_at),
        sync_state: existing
            .map(|session| session.sync_state)
            .unwrap_or(SessionSyncState::LocalOnly),
    };

    state.store.save_session(&session)?;
    Ok(session)
}

#[tauri::command]
fn delete_session(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    if let Some(controller) = remove_controller(&state, &session_id)? {
        let _ = controller.disconnect();
    }

    state.store.delete_session(&session_id)?;
    delete_session_secret_internal(&session_id)
}

#[tauri::command]
fn load_session_secret(session_id: String) -> Result<Option<String>, String> {
    load_session_secret_internal(&session_id)
}

#[tauri::command]
fn save_session_secret(session_id: String, secret: String) -> Result<(), String> {
    save_session_secret_internal(&session_id, &secret)
}

#[tauri::command]
fn delete_session_secret(session_id: String) -> Result<(), String> {
    delete_session_secret_internal(&session_id)
}

#[tauri::command]
fn get_background_on_close(state: tauri::State<'_, AppState>) -> bool {
    state.background_on_close.load(Ordering::Relaxed)
}

#[tauri::command]
fn set_background_on_close(state: tauri::State<'_, AppState>, enabled: bool) {
    state.background_on_close.store(enabled, Ordering::Relaxed);
}

#[tauri::command]
fn update_tray_status(
    app: tauri::AppHandle,
    title: Option<String>,
    tooltip: Option<String>,
) -> Result<(), String> {
    let Some(tray) = app.tray_by_id("main-tray") else {
        return Ok(());
    };
    tray.set_title(title).map_err(|error| error.to_string())?;
    tray.set_tooltip(tooltip).map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
fn list_known_hosts_entries() -> Result<Vec<KnownHostEntryPayload>, String> {
    let path = known_hosts_path()?;
    let content = fs::read_to_string(path).unwrap_or_default();
    let mut entries = Vec::new();

    for (index, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(hosts) = parts.next() else {
            continue;
        };
        let Some(key_type) = parts.next() else {
            continue;
        };

        entries.push(KnownHostEntryPayload {
            line: index + 1,
            hosts: hosts.to_string(),
            key_type: key_type.to_string(),
            hashed: hosts.starts_with("|1|"),
        });
    }

    Ok(entries)
}

#[tauri::command]
fn remove_known_host_entry(line: usize) -> Result<(), String> {
    remove_known_host_lines(&[line])
}

#[tauri::command]
fn forget_session_known_host(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<usize, String> {
    let Some(session) = state.store.find_session(&session_id)? else {
        return Err("Session not found".to_string());
    };

    let matches = known_host_keys(&session.host, session.port)
        .map_err(|error| error.to_string())?;
    if matches.is_empty() {
        return Ok(0);
    }

    let mut lines: Vec<usize> = matches.into_iter().map(|(line, _)| line).collect();
    lines.sort_unstable();
    lines.dedup();
    remove_known_host_lines(&lines)?;
    Ok(lines.len())
}

#[tauri::command]
fn connect_terminal(
    state: tauri::State<'_, AppState>,
    session_id: String,
    cols: u16,
    rows: u16,
    secret: Option<String>,
    channel: Channel<TerminalStreamMessage>,
) -> Result<TerminalConnectionPayload, String> {
    let Some(session) = state.store.find_session(&session_id)? else {
        return Err("Session not found".to_string());
    };

    if let Some(existing) = remove_controller(&state, &session_id)? {
        let _ = existing.disconnect();
    }

    let size = TerminalSize { cols, rows };
    let active = state.session_service.connect(&session, size, secret)?;
    let (controller, events) = active.split();

    let mut updated = session.clone();
    updated.last_connected_at = Some(now_timestamp());
    updated.updated_at = now_timestamp();
    state.store.save_session(&updated)?;

    {
        let mut controllers = state
            .controllers
            .lock()
            .map_err(|_| "controller registry is poisoned".to_string())?;
        controllers.insert(session_id.clone(), controller.clone());
    }

    let controllers = state.controllers.clone();
    let tracked_id = session_id.clone();
    let controller_for_pump = controller.clone();
    std::thread::spawn(move || {
        while let Ok(event) = events.recv() {
            let should_stop = matches!(event, SessionEvent::Closed);
            if channel.send(map_terminal_event(event)).is_err() {
                let _ = controller_for_pump.disconnect();
                break;
            }
            if should_stop {
                break;
            }
        }

        if let Ok(mut registry) = controllers.lock() {
            registry.remove(&tracked_id);
        }
    });

    Ok(TerminalConnectionPayload {
        session_id: controller.session_id().to_string(),
        session_name: controller.session_name().to_string(),
    })
}

#[tauri::command]
fn send_terminal_input(
    state: tauri::State<'_, AppState>,
    session_id: String,
    input: String,
) -> Result<(), String> {
    let controller = get_controller(&state, &session_id)?;
    controller.send_input(input)
}

#[tauri::command]
fn resize_terminal(
    state: tauri::State<'_, AppState>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let controller = get_controller(&state, &session_id)?;
    controller.resize(TerminalSize { cols, rows })
}

#[tauri::command]
fn disconnect_terminal(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    if let Some(controller) = remove_controller(&state, &session_id)? {
        controller.disconnect()?;
    }
    Ok(())
}

#[tauri::command]
async fn list_sftp_dir(
    state: tauri::State<'_, AppState>,
    request: SftpRequestPayload,
) -> Result<RemoteDirectoryListing, String> {
    let Some(session) = state.store.find_session(&request.session_id)? else {
        return Err("Session not found".to_string());
    };

    list_remote_dir(session, request.path, request.secret)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn download_sftp_file(
    state: tauri::State<'_, AppState>,
    request: DownloadRequestPayload,
) -> Result<TransferResult, String> {
    let Some(session) = state.store.find_session(&request.session_id)? else {
        return Err("Session not found".to_string());
    };

    download_remote_file(session, request.remote_path, request.local_path, request.secret)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn upload_sftp_file(
    state: tauri::State<'_, AppState>,
    request: UploadRequestPayload,
) -> Result<TransferResult, String> {
    let Some(session) = state.store.find_session(&request.session_id)? else {
        return Err("Session not found".to_string());
    };

    upload_local_file(session, request.local_path, request.remote_path, request.secret)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn create_sftp_dir(
    state: tauri::State<'_, AppState>,
    request: RemoteDirectoryMutationPayload,
) -> Result<MutationResult, String> {
    let Some(session) = state.store.find_session(&request.session_id)? else {
        return Err("Session not found".to_string());
    };

    create_remote_dir(session, request.remote_path, request.secret)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn rename_sftp_path(
    state: tauri::State<'_, AppState>,
    request: RemoteRenamePayload,
) -> Result<MutationResult, String> {
    let Some(session) = state.store.find_session(&request.session_id)? else {
        return Err("Session not found".to_string());
    };

    rename_remote_path(session, request.source_path, request.target_path, request.secret)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn delete_sftp_path(
    state: tauri::State<'_, AppState>,
    request: RemoteDeletePayload,
) -> Result<MutationResult, String> {
    let Some(session) = state.store.find_session(&request.session_id)? else {
        return Err("Session not found".to_string());
    };

    delete_remote_path(session, request.remote_path, request.is_dir, request.secret)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn start_sftp_transfer(
    state: tauri::State<'_, AppState>,
    request: TransferStartPayload,
    channel: Channel<TransferStreamMessage>,
) -> Result<(), String> {
    let Some(session) = state.store.find_session(&request.session_id)? else {
        return Err("Session not found".to_string());
    };

    let (cancel_tx, cancel_rx) = watch::channel(false);
    {
        let mut controls = state
            .transfer_controls
            .lock()
            .map_err(|_| "transfer registry is poisoned".to_string())?;
        controls.insert(request.job_id.clone(), cancel_tx);
    }

    let controls = state.transfer_controls.clone();
    let job_id = request.job_id.clone();
    let kind = request.kind.clone();
    let secret = request.secret.clone();
    let local_path = request.local_path.clone();
    let remote_path = request.remote_path.clone();
    let runtime = state._runtime.clone();

    runtime.handle().spawn(async move {
        let _ = channel.send(TransferStreamMessage::Started {
            job_id: job_id.clone(),
        });

        let result = match kind {
            TransferKindPayload::Upload => {
                upload_local_file_with_progress(
                    session,
                    local_path,
                    remote_path,
                    secret,
                    Some(cancel_rx),
                    |progress: TransferProgress| {
                        let _ = channel.send(TransferStreamMessage::Progress {
                            job_id: job_id.clone(),
                            transferred: progress.transferred,
                            total: progress.total,
                        });
                    },
                )
                .await
            }
            TransferKindPayload::Download => {
                download_remote_file_with_progress(
                    session,
                    remote_path,
                    local_path,
                    secret,
                    Some(cancel_rx),
                    |progress: TransferProgress| {
                        let _ = channel.send(TransferStreamMessage::Progress {
                            job_id: job_id.clone(),
                            transferred: progress.transferred,
                            total: progress.total,
                        });
                    },
                )
                .await
            }
        };

        match result {
            Ok(result) => {
                let _ = channel.send(TransferStreamMessage::Completed {
                    job_id: job_id.clone(),
                    bytes: result.bytes,
                    path: result.path,
                });
            }
            Err(error) => {
                let message = error.to_string();
                if message.contains("transfer cancelled") {
                    let _ = channel.send(TransferStreamMessage::Cancelled {
                        job_id: job_id.clone(),
                    });
                } else {
                    let _ = channel.send(TransferStreamMessage::Failed {
                        job_id: job_id.clone(),
                        error: message,
                    });
                }
            }
        }

        if let Ok(mut controls) = controls.lock() {
            controls.remove(&job_id);
        }
    });

    Ok(())
}

#[tauri::command]
fn cancel_sftp_transfer(
    state: tauri::State<'_, AppState>,
    request: CancelTransferPayload,
) -> Result<(), String> {
    let controls = state
        .transfer_controls
        .lock()
        .map_err(|_| "transfer registry is poisoned".to_string())?;
    let Some(cancel) = controls.get(&request.job_id) else {
        return Err("Transfer job not found".to_string());
    };
    cancel
        .send(true)
        .map_err(|_| "Failed to cancel transfer".to_string())
}

fn validate_session_draft(draft: &SessionDraftPayload) -> Result<(), String> {
    if draft.name.trim().is_empty() {
        return Err("Session name is required".to_string());
    }
    if draft.host.trim().is_empty() {
        return Err("Host is required".to_string());
    }
    if draft.username.trim().is_empty() {
        return Err("Username is required".to_string());
    }
    if draft.port == 0 {
        return Err("Port must be greater than zero".to_string());
    }
    Ok(())
}

fn map_auth_method(value: AuthMethodPayload) -> Result<AuthMethod, String> {
    match value {
        AuthMethodPayload::Password => Ok(AuthMethod::Password),
        AuthMethodPayload::PrivateKey { path } => {
            let key_path = path.trim();
            if key_path.is_empty() {
                return Err("Private key path is required".to_string());
            }
            Ok(AuthMethod::PrivateKey {
                path: key_path.to_string(),
            })
        }
        AuthMethodPayload::Agent => Ok(AuthMethod::Agent),
    }
}

fn normalize_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn map_terminal_event(event: SessionEvent) -> TerminalStreamMessage {
    match event {
        SessionEvent::StatusChanged(status) => TerminalStreamMessage::Status { status },
        SessionEvent::Output(data) => TerminalStreamMessage::Output { data },
        SessionEvent::Closed => TerminalStreamMessage::Closed,
    }
}

fn get_controller(
    state: &tauri::State<'_, AppState>,
    session_id: &str,
) -> Result<SessionController, String> {
    let controllers = state
        .controllers
        .lock()
        .map_err(|_| "controller registry is poisoned".to_string())?;

    controllers
        .get(session_id)
        .cloned()
        .ok_or_else(|| "Terminal session is not connected".to_string())
}

fn remove_controller(
    state: &tauri::State<'_, AppState>,
    session_id: &str,
) -> Result<Option<SessionController>, String> {
    let mut controllers = state
        .controllers
        .lock()
        .map_err(|_| "controller registry is poisoned".to_string())?;
    Ok(controllers.remove(session_id))
}

fn main() {
    let state = AppState::new().expect("failed to initialize desktop state");
    let background_on_close = state.background_on_close.clone();
    let quitting = state.quitting.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(state)
        .setup(|app| {
            setup_tray(app.handle())?;
            Ok(())
        })
        .on_window_event(move |window, event| {
            if window.label() != "main" {
                return;
            }

            if let WindowEvent::CloseRequested { api, .. } = event {
                if !quitting.load(Ordering::Relaxed) && background_on_close.load(Ordering::Relaxed) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_sessions,
            list_transfer_jobs,
            save_transfer_job,
            delete_transfer_job,
            list_transfer_events,
            save_transfer_event,
            clear_transfer_events,
            get_background_on_close,
            set_background_on_close,
            update_tray_status,
            load_session_secret,
            save_session_secret,
            delete_session_secret,
            list_known_hosts_entries,
            remove_known_host_entry,
            forget_session_known_host,
            save_session,
            delete_session,
            connect_terminal,
            send_terminal_input,
            resize_terminal,
            disconnect_terminal,
            list_sftp_dir,
            download_sftp_file,
            upload_sftp_file,
            create_sftp_dir,
            rename_sftp_path,
            delete_sftp_path,
            start_sftp_transfer,
            cancel_sftp_transfer
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn keyring_entry(session_id: &str) -> Result<Entry, String> {
    Entry::new(SECRET_SERVICE, session_id).map_err(|error| error.to_string())
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn load_session_secret_internal(session_id: &str) -> Result<Option<String>, String> {
    let entry = keyring_entry(session_id)?;
    match entry.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn save_session_secret_internal(session_id: &str, secret: &str) -> Result<(), String> {
    if secret.trim().is_empty() {
        return Err("Secret cannot be empty".to_string());
    }
    let entry = keyring_entry(session_id)?;
    entry.set_password(secret).map_err(|error| error.to_string())
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn delete_session_secret_internal(session_id: &str) -> Result<(), String> {
    let entry = keyring_entry(session_id)?;
    match entry.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn load_session_secret_internal(_session_id: &str) -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn save_session_secret_internal(_session_id: &str, _secret: &str) -> Result<(), String> {
    Err("System keychain is not supported on this platform".to_string())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn delete_session_secret_internal(_session_id: &str) -> Result<(), String> {
    Ok(())
}

fn setup_tray<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "tray_show", "Show", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray_quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let icon = Image::from_bytes(include_bytes!("../icons/icon.png"))?;

    TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .icon(icon)
        .show_menu_on_left_click(true)
        .tooltip("RustDock")
        .on_menu_event(|app, event| match event.id().0.as_str() {
            "tray_show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "tray_quit" => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.quitting.store(true, Ordering::Relaxed);
                }
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

fn known_hosts_path() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    Ok(PathBuf::from(home).join(".ssh").join("known_hosts"))
}

fn remove_known_host_lines(lines: &[usize]) -> Result<(), String> {
    let path = known_hosts_path()?;
    let content = fs::read_to_string(&path).unwrap_or_default();
    let retained: Vec<String> = content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line_number = index + 1;
            if lines.contains(&line_number) {
                None
            } else {
                Some(line.to_string())
            }
        })
        .collect();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut rewritten = retained.join("\n");
    if !rewritten.is_empty() {
        rewritten.push('\n');
    }
    fs::write(path, rewritten).map_err(|error| error.to_string())
}
