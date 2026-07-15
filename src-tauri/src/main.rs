#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use russh::keys::known_hosts::known_host_keys;
use rustdock_core::domain::{
    generate_session_id, now_timestamp, AuthMethod, SessionProfile, SessionSyncState,
    TransferEventRecord, TransferJobRecord,
};
use rustdock_core::runtime::AppRuntime;
use rustdock_core::sftp::{
    create_remote_dir, delete_remote_path, download_remote_directory_with_progress,
    download_remote_file, download_remote_file_with_progress, list_remote_dir,
    read_remote_text_file, rename_remote_path, upload_local_directory_with_progress,
    upload_local_file, upload_local_file_with_progress, write_remote_text_file, MutationResult,
    RemoteDirectoryListing, RemoteTextFile, SftpPool, TransferProgress, TransferResult,
};
use rustdock_core::ssh::{
    RusshSessionService, SessionController, SessionEvent, SessionService, SessionStatus,
    TerminalSize,
};
use rustdock_core::storage::{SessionRepository, SqliteSessionStore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::ipc::Channel;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};
use tokio::sync::watch;

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use keyring::{Entry, Error as KeyringError};

const SECRET_SERVICE: &str = "rustdock/session-secret";
const TERMINAL_READY_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
struct AppState {
    store: SqliteSessionStore,
    _runtime: Arc<AppRuntime>,
    session_service: RusshSessionService,
    sftp_pool: SftpPool,
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
            sftp_pool: SftpPool::new(),
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
struct RemoteTextReadPayload {
    session_id: String,
    remote_path: String,
    secret: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct RemoteTextWritePayload {
    session_id: String,
    remote_path: String,
    content: String,
    secret: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum TransferKindPayload {
    Upload,
    Download,
    UploadDir,
    DownloadDir,
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
struct LocalPathInspection {
    path: String,
    is_file: bool,
    is_dir: bool,
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
    Started {
        job_id: String,
    },
    Progress {
        job_id: String,
        transferred: u64,
        total: Option<u64>,
        current_path: Option<String>,
        completed_files: Option<usize>,
        total_files: Option<usize>,
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
    Cancelled {
        job_id: String,
    },
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
fn delete_transfer_job(state: tauri::State<'_, AppState>, job_id: String) -> Result<(), String> {
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
        last_connected_at: existing
            .as_ref()
            .and_then(|session| session.last_connected_at),
        sync_state: existing
            .map(|session| session.sync_state)
            .unwrap_or(SessionSyncState::LocalOnly),
    };

    state.store.save_session(&session)?;
    Ok(session)
}

#[tauri::command]
fn delete_session(state: tauri::State<'_, AppState>, session_id: String) -> Result<(), String> {
    if let Some(controller) = remove_controller(&state, &session_id)? {
        let _ = controller.disconnect();
    }
    cancel_transfers_for_session(&state, &session_id)?;
    invalidate_sftp_pool(&state, &session_id);

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
    tray.set_tooltip(tooltip)
        .map_err(|error| error.to_string())?;
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

    let matches =
        known_host_keys(&session.host, session.port).map_err(|error| error.to_string())?;
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
    let buffered_events = match wait_for_terminal_ready(&events) {
        Ok(events) => events,
        Err(error) => {
            let _ = controller.disconnect();
            return Err(error);
        }
    };

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
    let tracked_instance_id = controller.instance_id();
    std::thread::spawn(move || {
        for event in buffered_events {
            if forward_terminal_event(&channel, &controller_for_pump, event) {
                remove_controller_if_instance_matches(
                    &controllers,
                    &tracked_id,
                    tracked_instance_id,
                );
                return;
            }
        }

        while let Ok(event) = events.recv() {
            if forward_terminal_event(&channel, &controller_for_pump, event) {
                break;
            }
        }

        remove_controller_if_instance_matches(&controllers, &tracked_id, tracked_instance_id);
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
    // Terminal and SFTP share the same logical session identity; drop the
    // cached SFTP channel so the next browse re-authenticates cleanly.
    invalidate_sftp_pool(&state, &session_id);
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

    list_remote_dir(Some(&state.sftp_pool), session, request.path, request.secret)
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

    download_remote_file(
        Some(&state.sftp_pool),
        session,
        request.remote_path,
        request.local_path,
        request.secret,
    )
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

    upload_local_file(
        Some(&state.sftp_pool),
        session,
        request.local_path,
        request.remote_path,
        request.secret,
    )
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

    create_remote_dir(Some(&state.sftp_pool), session, request.remote_path, request.secret)
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

    rename_remote_path(
        Some(&state.sftp_pool),
        session,
        request.source_path,
        request.target_path,
        request.secret,
    )
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

    delete_remote_path(
        Some(&state.sftp_pool),
        session,
        request.remote_path,
        request.is_dir,
        request.secret,
    )
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn read_remote_text(
    state: tauri::State<'_, AppState>,
    request: RemoteTextReadPayload,
) -> Result<RemoteTextFile, String> {
    let Some(session) = state.store.find_session(&request.session_id)? else {
        return Err("Session not found".to_string());
    };

    read_remote_text_file(Some(&state.sftp_pool), session, request.remote_path, request.secret)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn write_remote_text(
    state: tauri::State<'_, AppState>,
    request: RemoteTextWritePayload,
) -> Result<MutationResult, String> {
    let Some(session) = state.store.find_session(&request.session_id)? else {
        return Err("Session not found".to_string());
    };

    write_remote_text_file(
        Some(&state.sftp_pool),
        session,
        request.remote_path,
        request.content,
        request.secret,
    )
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
    let sftp_pool = state.sftp_pool.clone();

    runtime.handle().spawn(async move {
        let _ = channel.send(TransferStreamMessage::Started {
            job_id: job_id.clone(),
        });

        let result = match kind {
            TransferKindPayload::Upload => {
                upload_local_file_with_progress(
                    Some(&sftp_pool),
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
                            current_path: progress.current_path.clone(),
                            completed_files: progress.completed_files,
                            total_files: progress.total_files,
                        });
                    },
                )
                .await
            }
            TransferKindPayload::UploadDir => {
                upload_local_directory_with_progress(
                    Some(&sftp_pool),
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
                            current_path: progress.current_path.clone(),
                            completed_files: progress.completed_files,
                            total_files: progress.total_files,
                        });
                    },
                )
                .await
            }
            TransferKindPayload::Download => {
                download_remote_file_with_progress(
                    Some(&sftp_pool),
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
                            current_path: progress.current_path.clone(),
                            completed_files: progress.completed_files,
                            total_files: progress.total_files,
                        });
                    },
                )
                .await
            }
            TransferKindPayload::DownloadDir => {
                download_remote_directory_with_progress(
                    Some(&sftp_pool),
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
                            current_path: progress.current_path.clone(),
                            completed_files: progress.completed_files,
                            total_files: progress.total_files,
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

#[tauri::command]
fn inspect_local_path(path: String) -> Result<LocalPathInspection, String> {
    let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
    Ok(LocalPathInspection {
        path,
        is_file: metadata.is_file(),
        is_dir: metadata.is_dir(),
    })
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

fn remove_controller_if_instance_matches(
    controllers: &Arc<Mutex<HashMap<String, SessionController>>>,
    session_id: &str,
    instance_id: u64,
) {
    if let Ok(mut registry) = controllers.lock() {
        let should_remove = registry
            .get(session_id)
            .map(|controller| controller.instance_id() == instance_id)
            .unwrap_or(false);
        if should_remove {
            registry.remove(session_id);
        }
    }
}

fn cancel_transfers_for_session(
    state: &tauri::State<'_, AppState>,
    session_id: &str,
) -> Result<(), String> {
    let job_ids: Vec<String> = state
        .store
        .list_transfer_jobs()?
        .into_iter()
        .filter(|job| job.session_id == session_id)
        .map(|job| job.id)
        .collect();

    if job_ids.is_empty() {
        return Ok(());
    }

    let controls = state
        .transfer_controls
        .lock()
        .map_err(|_| "transfer registry is poisoned".to_string())?;

    for job_id in job_ids {
        if let Some(cancel) = controls.get(&job_id) {
            let _ = cancel.send(true);
        }
    }

    Ok(())
}


fn invalidate_sftp_pool(state: &tauri::State<'_, AppState>, session_id: &str) {
    let pool = state.sftp_pool.clone();
    let session_id = session_id.to_string();
    // block_on is safe here: AppRuntime is a dedicated multi-thread runtime, and
    // these commands run on Tauri's thread (not on that runtime's workers).
    state._runtime.handle().block_on(async move {
        pool.invalidate(&session_id).await;
    });
}

fn wait_for_terminal_ready(
    events: &std::sync::mpsc::Receiver<SessionEvent>,
) -> Result<Vec<SessionEvent>, String> {
    let deadline = Instant::now() + TERMINAL_READY_TIMEOUT;
    let mut buffered = Vec::new();

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(extract_terminal_startup_error_with_fallback(
                &buffered,
                Some("Timed out while establishing SSH session"),
            ));
        }

        match events.recv_timeout(remaining) {
            Ok(event) => {
                let is_connected =
                    matches!(event, SessionEvent::StatusChanged(SessionStatus::Connected));
                let is_closed = matches!(event, SessionEvent::Closed);
                buffered.push(event);

                if is_connected {
                    return Ok(buffered);
                }

                if is_closed {
                    return Err(extract_terminal_startup_error(&buffered));
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(extract_terminal_startup_error_with_fallback(
                    &buffered,
                    Some("Timed out while establishing SSH session"),
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(extract_terminal_startup_error_with_fallback(
                    &buffered, None,
                ));
            }
        }
    }
}

fn extract_terminal_startup_error(events: &[SessionEvent]) -> String {
    extract_terminal_startup_error_with_fallback(events, None)
}

fn extract_terminal_startup_error_with_fallback(
    events: &[SessionEvent],
    fallback: Option<&str>,
) -> String {
    let output_lines: Vec<&str> = events
        .iter()
        .filter_map(|event| match event {
            SessionEvent::Output(text) => Some(text.as_str()),
            _ => None,
        })
        .flat_map(|text| text.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    for line in output_lines.iter().rev() {
        if let Some(message) = line.strip_prefix("[known_hosts]") {
            let message = message.trim();
            if !message.is_empty()
                && (message.contains("rejected")
                    || message.contains("failed")
                    || message.contains("mismatch"))
            {
                return message.to_string();
            }
        }
    }

    for line in output_lines.iter().rev() {
        if let Some(message) = line.strip_prefix("[disconnect]") {
            let message = message.trim();
            if !message.is_empty() {
                return message.to_string();
            }
        }
    }

    for line in output_lines.iter().rev() {
        if let Some(message) = line.strip_prefix("[error]") {
            let message = message.trim();
            if !message.is_empty() {
                return message.to_string();
            }
        }
    }

    if let Some(line) = output_lines.last() {
        return (*line).to_string();
    }

    if let Some(fallback) = fallback {
        return fallback.to_string();
    }

    if events
        .iter()
        .any(|event| matches!(event, SessionEvent::StatusChanged(SessionStatus::Failed)))
    {
        return "SSH session failed before becoming ready".to_string();
    }

    "SSH session closed before becoming ready".to_string()
}

fn forward_terminal_event(
    channel: &Channel<TerminalStreamMessage>,
    controller: &SessionController,
    event: SessionEvent,
) -> bool {
    let should_stop = matches!(event, SessionEvent::Closed);
    if channel.send(map_terminal_event(event)).is_err() {
        let _ = controller.disconnect();
        return true;
    }
    should_stop
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
                if !quitting.load(Ordering::Relaxed) && background_on_close.load(Ordering::Relaxed)
                {
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
            read_remote_text,
            write_remote_text,
            start_sftp_transfer,
            cancel_sftp_transfer,
            inspect_local_path
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
    entry
        .set_password(secret)
        .map_err(|error| error.to_string())
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
    let home_dir =
        user_home_dir().ok_or_else(|| "Unable to determine user home directory".to_string())?;
    Ok(home_dir.join(".ssh").join("known_hosts"))
}

fn user_home_dir() -> Option<PathBuf> {
    env_var_path("HOME").or_else(|| {
        #[cfg(target_os = "windows")]
        {
            env_var_path("USERPROFILE").or_else(|| {
                let home_drive = env_var_path("HOMEDRIVE")?;
                let home_path = env_var_path("HOMEPATH")?;
                let mut combined = home_drive;
                combined.push(home_path);
                Some(combined)
            })
        }

        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    })
}

fn env_var_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
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

#[cfg(test)]
mod tests {
    use super::*;
    use rustdock_core::domain::{AuthMethod, SessionProfile, SessionSyncState};
    use rustdock_core::runtime::AppRuntime;
    use rustdock_core::ssh::{MockSessionService, SessionService};

    fn sample_session(id: &str) -> SessionProfile {
        SessionProfile {
            id: id.to_string(),
            name: "Test Session".to_string(),
            host: "example.com".to_string(),
            port: 22,
            username: "root".to_string(),
            auth_method: AuthMethod::Password,
            tags: Vec::new(),
            notes: String::new(),
            local_roots: Vec::new(),
            remote_roots: vec!["/".to_string()],
            created_at: 1,
            updated_at: 1,
            last_connected_at: None,
            sync_state: SessionSyncState::LocalOnly,
        }
    }

    #[test]
    fn startup_error_prefers_known_hosts_rejection_over_timeout_fallback() {
        let events = vec![
            SessionEvent::Output(
                "[known_hosts] rejected host key for example.com:22: key mismatch".to_string(),
            ),
            SessionEvent::StatusChanged(SessionStatus::Failed),
            SessionEvent::Closed,
        ];

        let message = extract_terminal_startup_error_with_fallback(
            &events,
            Some("Timed out while establishing SSH session"),
        );

        assert_eq!(
            message,
            "rejected host key for example.com:22: key mismatch"
        );
    }

    #[test]
    fn removing_old_controller_instance_keeps_newer_controller_registered() {
        let runtime = AppRuntime::new().expect("runtime");
        let service = MockSessionService::new(runtime.handle());
        let profile = sample_session("session-1");

        let old = service
            .connect(&profile, TerminalSize::default(), None)
            .expect("old controller")
            .controller();
        let new = service
            .connect(&profile, TerminalSize::default(), None)
            .expect("new controller")
            .controller();

        let registry = Arc::new(Mutex::new(HashMap::from([(
            profile.id.clone(),
            new.clone(),
        )])));

        remove_controller_if_instance_matches(&registry, &profile.id, old.instance_id());
        let stored = registry
            .lock()
            .expect("registry")
            .get(&profile.id)
            .cloned()
            .expect("new controller should remain");
        assert_eq!(stored.instance_id(), new.instance_id());

        remove_controller_if_instance_matches(&registry, &profile.id, new.instance_id());
        assert!(registry.lock().expect("registry").is_empty());

        let _ = old.disconnect();
        let _ = new.disconnect();
    }
}
