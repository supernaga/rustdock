use crate::domain::SessionProfile;
use crate::ssh::{connect_authenticated, TrustOnFirstUseHandler};
use anyhow::{anyhow, Context, Result};
use russh_sftp::client::SftpSession;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tokio::fs::{self, File as TokioFile};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::watch;

#[derive(Clone, Debug, Serialize)]
pub struct RemoteDirEntry {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RemoteDirectoryListing {
    pub directory: String,
    pub entries: Vec<RemoteDirEntry>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TransferResult {
    pub path: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct MutationResult {
    pub path: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct RemoteTextFile {
    pub path: String,
    pub content: String,
    pub bytes: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct TransferProgress {
    pub transferred: u64,
    pub total: Option<u64>,
    pub current_path: Option<String>,
    pub completed_files: Option<usize>,
    pub total_files: Option<usize>,
}

const MAX_TEXT_FILE_BYTES: u64 = 1024 * 1024;

struct RemoteDirectoryPlan {
    directories: Vec<PathBuf>,
    files: Vec<RemoteFileTransfer>,
    total_bytes: u64,
}

struct RemoteFileTransfer {
    remote_path: String,
    local_path: PathBuf,
}

struct LocalDirectoryPlan {
    directories: Vec<String>,
    files: Vec<LocalFileTransfer>,
    total_bytes: u64,
}

struct LocalFileTransfer {
    local_path: PathBuf,
    remote_path: String,
}

pub async fn list_remote_dir(
    profile: SessionProfile,
    path: String,
    secret: Option<String>,
) -> Result<RemoteDirectoryListing> {
    let (sftp, _driver) = open_sftp_raw(profile, secret.as_deref()).await?;
    let directory = sftp.canonicalize(&path).await.unwrap_or(path);
    let mut entries = Vec::new();

    let read_dir = sftp
        .read_dir(directory.clone())
        .await
        .with_context(|| format!("failed to list remote directory {directory}"))?;

    for entry in read_dir {
        let name = entry.file_name();
        let file_type = entry.file_type();
        let kind = format!("{:?}", file_type);
        let is_dir = file_type.is_dir();
        let size = if is_dir {
            None
        } else {
            Some(entry.metadata().len())
        };
        entries.push(RemoteDirEntry {
            path: join_remote_path(&directory, &name),
            name,
            kind,
            is_dir,
            size,
        });
    }

    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    Ok(RemoteDirectoryListing { directory, entries })
}

pub async fn download_remote_file(
    profile: SessionProfile,
    remote_path: String,
    local_path: String,
    secret: Option<String>,
) -> Result<TransferResult> {
    download_remote_file_with_progress(profile, remote_path, local_path, secret, None, |_| {}).await
}

pub async fn download_remote_file_with_progress<F>(
    profile: SessionProfile,
    remote_path: String,
    local_path: String,
    secret: Option<String>,
    cancel_rx: Option<watch::Receiver<bool>>,
    mut on_progress: F,
) -> Result<TransferResult>
where
    F: FnMut(TransferProgress) + Send,
{
    let (sftp, _driver) = open_sftp_raw(profile, secret.as_deref()).await?;
    let total = sftp
        .metadata(&remote_path)
        .await
        .ok()
        .map(|metadata| metadata.len());
    let mut remote_file = sftp
        .open(&remote_path)
        .await
        .with_context(|| format!("failed to open remote file {remote_path}"))?;

    let local_path_ref = Path::new(&local_path);
    if let Some(parent) = local_path_ref.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create local directory {}", parent.display()))?;
    }

    let mut local_file = TokioFile::create(&local_path)
        .await
        .with_context(|| format!("failed to create local file {local_path}"))?;

    let bytes = copy_with_progress(
        &mut remote_file,
        &mut local_file,
        total,
        cancel_rx,
        &mut on_progress,
    )
    .await
    .with_context(|| format!("failed to download remote file {remote_path}"))?;

    Ok(TransferResult {
        path: local_path,
        bytes,
    })
}

pub async fn upload_local_file(
    profile: SessionProfile,
    local_path: String,
    remote_path: String,
    secret: Option<String>,
) -> Result<TransferResult> {
    upload_local_file_with_progress(profile, local_path, remote_path, secret, None, |_| {}).await
}

pub async fn upload_local_file_with_progress<F>(
    profile: SessionProfile,
    local_path: String,
    remote_path: String,
    secret: Option<String>,
    cancel_rx: Option<watch::Receiver<bool>>,
    mut on_progress: F,
) -> Result<TransferResult>
where
    F: FnMut(TransferProgress) + Send,
{
    let (sftp, _driver) = open_sftp_raw(profile, secret.as_deref()).await?;
    let total = tokio::fs::metadata(&local_path)
        .await
        .ok()
        .map(|metadata| metadata.len());
    let mut local_file = TokioFile::open(&local_path)
        .await
        .with_context(|| format!("failed to open local file {local_path}"))?;
    let mut remote_file = sftp
        .create(&remote_path)
        .await
        .with_context(|| format!("failed to create remote file {remote_path}"))?;

    let bytes = copy_with_progress(
        &mut local_file,
        &mut remote_file,
        total,
        cancel_rx,
        &mut on_progress,
    )
    .await
    .with_context(|| format!("failed to upload local file {local_path}"))?;

    Ok(TransferResult {
        path: remote_path,
        bytes,
    })
}

pub async fn download_remote_directory_with_progress<F>(
    profile: SessionProfile,
    remote_path: String,
    local_path: String,
    secret: Option<String>,
    cancel_rx: Option<watch::Receiver<bool>>,
    mut on_progress: F,
) -> Result<TransferResult>
where
    F: FnMut(TransferProgress) + Send,
{
    let (sftp, _driver) = open_sftp_raw(profile, secret.as_deref()).await?;
    let directory = sftp.canonicalize(&remote_path).await.unwrap_or(remote_path);
    let plan = build_remote_directory_plan(&sftp, &directory, PathBuf::from(&local_path)).await?;
    let total_files = plan.files.len();

    on_progress(TransferProgress {
        transferred: 0,
        total: Some(plan.total_bytes),
        current_path: None,
        completed_files: Some(0),
        total_files: Some(total_files),
    });

    for directory_path in &plan.directories {
        ensure_not_cancelled(cancel_rx.as_ref())?;
        fs::create_dir_all(directory_path).await.with_context(|| {
            format!(
                "failed to create local directory {}",
                directory_path.display()
            )
        })?;
    }

    let mut transferred = 0_u64;
    let mut completed_files = 0_usize;
    for file in plan.files {
        ensure_not_cancelled(cancel_rx.as_ref())?;
        if let Some(parent) = file.local_path.parent() {
            fs::create_dir_all(parent).await.with_context(|| {
                format!("failed to create local directory {}", parent.display())
            })?;
        }

        let mut remote_file = sftp
            .open(&file.remote_path)
            .await
            .with_context(|| format!("failed to open remote file {}", file.remote_path))?;
        let mut local_file = TokioFile::create(&file.local_path).await.with_context(|| {
            format!("failed to create local file {}", file.local_path.display())
        })?;

        on_progress(TransferProgress {
            transferred,
            total: Some(plan.total_bytes),
            current_path: Some(file.remote_path.clone()),
            completed_files: Some(completed_files),
            total_files: Some(total_files),
        });

        copy_with_aggregate_progress(
            &mut remote_file,
            &mut local_file,
            plan.total_bytes,
            cancel_rx.clone(),
            &mut transferred,
            &file.remote_path,
            completed_files,
            total_files,
            &mut on_progress,
        )
        .await
        .with_context(|| format!("failed to download remote file {}", file.remote_path))?;
        completed_files += 1;
    }

    on_progress(TransferProgress {
        transferred,
        total: Some(plan.total_bytes),
        current_path: None,
        completed_files: Some(completed_files),
        total_files: Some(total_files),
    });

    Ok(TransferResult {
        path: local_path,
        bytes: transferred,
    })
}

pub async fn upload_local_directory_with_progress<F>(
    profile: SessionProfile,
    local_path: String,
    remote_path: String,
    secret: Option<String>,
    cancel_rx: Option<watch::Receiver<bool>>,
    mut on_progress: F,
) -> Result<TransferResult>
where
    F: FnMut(TransferProgress) + Send,
{
    let plan = build_local_directory_plan(PathBuf::from(&local_path), remote_path.clone()).await?;
    let (sftp, _driver) = open_sftp_raw(profile, secret.as_deref()).await?;
    let total_files = plan.files.len();

    on_progress(TransferProgress {
        transferred: 0,
        total: Some(plan.total_bytes),
        current_path: None,
        completed_files: Some(0),
        total_files: Some(total_files),
    });

    for directory in &plan.directories {
        ensure_not_cancelled(cancel_rx.as_ref())?;
        ensure_remote_dir_all(&sftp, directory).await?;
    }

    let mut transferred = 0_u64;
    let mut completed_files = 0_usize;
    for file in plan.files {
        ensure_not_cancelled(cancel_rx.as_ref())?;
        let mut local_file = TokioFile::open(&file.local_path)
            .await
            .with_context(|| format!("failed to open local file {}", file.local_path.display()))?;
        let mut remote_file = sftp
            .create(&file.remote_path)
            .await
            .with_context(|| format!("failed to create remote file {}", file.remote_path))?;

        on_progress(TransferProgress {
            transferred,
            total: Some(plan.total_bytes),
            current_path: Some(file.remote_path.clone()),
            completed_files: Some(completed_files),
            total_files: Some(total_files),
        });

        copy_with_aggregate_progress(
            &mut local_file,
            &mut remote_file,
            plan.total_bytes,
            cancel_rx.clone(),
            &mut transferred,
            &file.remote_path,
            completed_files,
            total_files,
            &mut on_progress,
        )
        .await
        .with_context(|| format!("failed to upload local file {}", file.local_path.display()))?;
        completed_files += 1;
    }

    on_progress(TransferProgress {
        transferred,
        total: Some(plan.total_bytes),
        current_path: None,
        completed_files: Some(completed_files),
        total_files: Some(total_files),
    });

    Ok(TransferResult {
        path: remote_path,
        bytes: transferred,
    })
}

pub async fn create_remote_dir(
    profile: SessionProfile,
    remote_path: String,
    secret: Option<String>,
) -> Result<MutationResult> {
    let (sftp, _driver) = open_sftp_raw(profile, secret.as_deref()).await?;
    sftp.create_dir(&remote_path)
        .await
        .with_context(|| format!("failed to create remote directory {remote_path}"))?;

    Ok(MutationResult { path: remote_path })
}

pub async fn read_remote_text_file(
    profile: SessionProfile,
    remote_path: String,
    secret: Option<String>,
) -> Result<RemoteTextFile> {
    let (sftp, _driver) = open_sftp_raw(profile, secret.as_deref()).await?;
    let metadata = sftp
        .metadata(&remote_path)
        .await
        .with_context(|| format!("failed to stat remote file {remote_path}"))?;
    let size = metadata.len();
    if size > MAX_TEXT_FILE_BYTES {
        return Err(anyhow!(
            "remote file {remote_path} is too large for the inline editor ({} bytes > {} bytes)",
            size,
            MAX_TEXT_FILE_BYTES
        ));
    }

    let mut remote_file = sftp
        .open(&remote_path)
        .await
        .with_context(|| format!("failed to open remote file {remote_path}"))?;
    let mut bytes = Vec::with_capacity(size as usize);
    remote_file
        .read_to_end(&mut bytes)
        .await
        .with_context(|| format!("failed to read remote file {remote_path}"))?;
    let content = String::from_utf8(bytes)
        .with_context(|| format!("remote file {remote_path} is not valid UTF-8 text"))?;

    Ok(RemoteTextFile {
        path: remote_path,
        bytes: content.len(),
        content,
    })
}

pub async fn write_remote_text_file(
    profile: SessionProfile,
    remote_path: String,
    content: String,
    secret: Option<String>,
) -> Result<MutationResult> {
    if content.len() as u64 > MAX_TEXT_FILE_BYTES {
        return Err(anyhow!(
            "edited content exceeds the inline editor limit ({} bytes > {} bytes)",
            content.len(),
            MAX_TEXT_FILE_BYTES
        ));
    }

    let (sftp, _driver) = open_sftp_raw(profile, secret.as_deref()).await?;
    let mut remote_file = sftp
        .create(&remote_path)
        .await
        .with_context(|| format!("failed to open remote file {remote_path} for writing"))?;

    remote_file
        .write_all(content.as_bytes())
        .await
        .with_context(|| format!("failed to write remote file {remote_path}"))?;
    remote_file
        .flush()
        .await
        .with_context(|| format!("failed to flush remote file {remote_path}"))?;

    Ok(MutationResult { path: remote_path })
}

pub async fn rename_remote_path(
    profile: SessionProfile,
    source_path: String,
    target_path: String,
    secret: Option<String>,
) -> Result<MutationResult> {
    let (sftp, _driver) = open_sftp_raw(profile, secret.as_deref()).await?;
    sftp.rename(&source_path, &target_path)
        .await
        .with_context(|| format!("failed to rename remote path {source_path} to {target_path}"))?;

    Ok(MutationResult { path: target_path })
}

pub async fn delete_remote_path(
    profile: SessionProfile,
    remote_path: String,
    is_dir: bool,
    secret: Option<String>,
) -> Result<MutationResult> {
    let (sftp, _driver) = open_sftp_raw(profile, secret.as_deref()).await?;
    if is_dir {
        delete_remote_directory_recursive(&sftp, &remote_path).await?;
    } else {
        sftp.remove_file(&remote_path)
            .await
            .with_context(|| format!("failed to remove remote file {remote_path}"))?;
    }

    Ok(MutationResult { path: remote_path })
}

async fn delete_remote_directory_recursive(sftp: &SftpSession, remote_root: &str) -> Result<()> {
    let metadata = sftp
        .metadata(remote_root)
        .await
        .with_context(|| format!("failed to stat remote directory {remote_root}"))?;
    if !metadata.is_dir() {
        return Err(anyhow!("remote path {remote_root} is not a directory"));
    }

    let mut stack = vec![(remote_root.to_string(), false)];

    while let Some((remote_dir, visited)) = stack.pop() {
        if visited {
            sftp.remove_dir(&remote_dir)
                .await
                .with_context(|| format!("failed to remove remote directory {remote_dir}"))?;
            continue;
        }

        stack.push((remote_dir.clone(), true));

        let entries = sftp
            .read_dir(remote_dir.clone())
            .await
            .with_context(|| format!("failed to list remote directory {remote_dir}"))?;
        let mut child_directories = Vec::new();
        let mut child_files = Vec::new();

        for entry in entries {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }

            let child_path = join_remote_path(&remote_dir, &name);
            if entry.file_type().is_dir() {
                child_directories.push(child_path);
            } else {
                child_files.push(child_path);
            }
        }

        child_files.sort();
        for child_file in child_files {
            sftp.remove_file(&child_file)
                .await
                .with_context(|| format!("failed to remove remote file {child_file}"))?;
        }

        child_directories.sort();
        for child_directory in child_directories.into_iter().rev() {
            stack.push((child_directory, false));
        }
    }

    Ok(())
}

async fn open_sftp_raw(
    profile: SessionProfile,
    secret: Option<&str>,
) -> Result<(SftpSession, tokio::task::JoinHandle<()>)> {
    let handler = TrustOnFirstUseHandler::new(profile.host.clone(), profile.port, None);
    let session = connect_authenticated(profile, secret, handler).await?;
    let channel = session
        .channel_open_session()
        .await
        .context("failed to open SSH channel for SFTP")?;

    channel
        .request_subsystem(true, "sftp")
        .await
        .context("failed to start SFTP subsystem")?;

    let stream = channel.into_stream();
    let sftp = SftpSession::new(stream)
        .await
        .context("failed to initialize SFTP session")?;

    let driver = tokio::spawn(async move {
        let _ = session.await;
    });

    Ok((sftp, driver))
}

fn join_remote_path(directory: &str, name: &str) -> String {
    if directory == "/" {
        format!("/{name}")
    } else if directory.ends_with('/') {
        format!("{directory}{name}")
    } else {
        format!("{directory}/{name}")
    }
}

async fn build_remote_directory_plan(
    sftp: &SftpSession,
    remote_root: &str,
    local_root: PathBuf,
) -> Result<RemoteDirectoryPlan> {
    let metadata = sftp
        .metadata(remote_root)
        .await
        .with_context(|| format!("failed to stat remote directory {remote_root}"))?;
    if !metadata.is_dir() {
        return Err(anyhow!("remote path {remote_root} is not a directory"));
    }

    let mut directories = vec![local_root.clone()];
    let mut files = Vec::new();
    let mut total_bytes = 0_u64;
    let mut stack = vec![(remote_root.to_string(), local_root)];

    while let Some((remote_dir, local_dir)) = stack.pop() {
        let entries = sftp
            .read_dir(remote_dir.clone())
            .await
            .with_context(|| format!("failed to list remote directory {remote_dir}"))?;
        let mut child_directories = Vec::new();

        for entry in entries {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }

            let remote_path = join_remote_path(&remote_dir, &name);
            let local_path = local_dir.join(&name);
            if entry.file_type().is_dir() {
                child_directories.push((remote_path, local_path));
            } else {
                total_bytes += entry.metadata().len();
                files.push(RemoteFileTransfer {
                    remote_path,
                    local_path,
                });
            }
        }

        child_directories.sort_by(|left, right| left.0.cmp(&right.0));
        for (remote_path, local_path) in child_directories.into_iter().rev() {
            directories.push(local_path.clone());
            stack.push((remote_path, local_path));
        }
    }

    files.sort_by(|left, right| left.remote_path.cmp(&right.remote_path));

    Ok(RemoteDirectoryPlan {
        directories,
        files,
        total_bytes,
    })
}

async fn build_local_directory_plan(
    local_root: PathBuf,
    remote_root: String,
) -> Result<LocalDirectoryPlan> {
    let metadata = fs::metadata(&local_root)
        .await
        .with_context(|| format!("failed to stat local directory {}", local_root.display()))?;
    if !metadata.is_dir() {
        return Err(anyhow!(
            "local path {} is not a directory",
            local_root.display()
        ));
    }

    let mut directories = vec![remote_root.clone()];
    let mut files = Vec::new();
    let mut total_bytes = 0_u64;
    let mut stack = vec![(local_root, remote_root)];

    while let Some((local_dir, remote_dir)) = stack.pop() {
        let mut entries = fs::read_dir(&local_dir)
            .await
            .with_context(|| format!("failed to read local directory {}", local_dir.display()))?;
        let mut child_directories = Vec::new();

        while let Some(entry) = entries
            .next_entry()
            .await
            .with_context(|| format!("failed to read local directory {}", local_dir.display()))?
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "." || name == ".." {
                continue;
            }

            let file_type = entry.file_type().await.with_context(|| {
                format!("failed to inspect local path {}", entry.path().display())
            })?;
            let local_path = entry.path();
            let remote_path = join_remote_path(&remote_dir, &name);

            if file_type.is_dir() {
                directories.push(remote_path.clone());
                child_directories.push((local_path, remote_path));
            } else if file_type.is_file() {
                total_bytes += entry
                    .metadata()
                    .await
                    .with_context(|| {
                        format!("failed to inspect local file {}", local_path.display())
                    })?
                    .len();
                files.push(LocalFileTransfer {
                    local_path,
                    remote_path,
                });
            } else {
                return Err(anyhow!(
                    "unsupported local entry type at {}",
                    local_path.display()
                ));
            }
        }

        child_directories.sort_by(|left, right| left.0.cmp(&right.0));
        for child in child_directories.into_iter().rev() {
            stack.push(child);
        }
    }

    files.sort_by(|left, right| left.remote_path.cmp(&right.remote_path));

    Ok(LocalDirectoryPlan {
        directories,
        files,
        total_bytes,
    })
}

async fn ensure_remote_dir_all(sftp: &SftpSession, remote_path: &str) -> Result<()> {
    let normalized = remote_path.trim();
    if normalized.is_empty() || normalized == "/" {
        return Ok(());
    }

    let is_absolute = normalized.starts_with('/');
    let mut current = if is_absolute {
        "/".to_string()
    } else {
        String::new()
    };

    for segment in normalized.split('/').filter(|segment| !segment.is_empty()) {
        current = if current.is_empty() {
            segment.to_string()
        } else if current == "/" {
            format!("/{segment}")
        } else {
            format!("{current}/{segment}")
        };

        if let Err(create_error) = sftp.create_dir(current.clone()).await {
            let create_error_message = create_error.to_string();
            let metadata = sftp.metadata(&current).await.with_context(|| {
                format!("failed to create remote directory {current}: {create_error_message}")
            })?;
            if !metadata.is_dir() {
                return Err(anyhow!(
                    "remote path {current} exists but is not a directory"
                ));
            }
        }
    }

    Ok(())
}

fn ensure_not_cancelled(cancel_rx: Option<&watch::Receiver<bool>>) -> Result<()> {
    if cancel_rx.is_some_and(|receiver| *receiver.borrow()) {
        return Err(anyhow!("transfer cancelled"));
    }

    Ok(())
}

async fn copy_with_progress<R, W, F>(
    reader: &mut R,
    writer: &mut W,
    total: Option<u64>,
    mut cancel_rx: Option<watch::Receiver<bool>>,
    on_progress: &mut F,
) -> Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    F: FnMut(TransferProgress),
{
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut transferred = 0_u64;
    on_progress(TransferProgress {
        transferred,
        total,
        current_path: None,
        completed_files: None,
        total_files: None,
    });

    loop {
        if let Some(receiver) = cancel_rx.as_mut() {
            if *receiver.borrow() {
                return Err(anyhow::anyhow!("transfer cancelled"));
            }
        }

        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            writer.flush().await?;
            break;
        }

        writer.write_all(&buffer[..read]).await?;
        transferred += read as u64;
        on_progress(TransferProgress {
            transferred,
            total,
            current_path: None,
            completed_files: None,
            total_files: None,
        });
    }

    Ok(transferred)
}

async fn copy_with_aggregate_progress<R, W, F>(
    reader: &mut R,
    writer: &mut W,
    total: u64,
    cancel_rx: Option<watch::Receiver<bool>>,
    transferred: &mut u64,
    current_path: &str,
    completed_files: usize,
    total_files: usize,
    on_progress: &mut F,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    F: FnMut(TransferProgress),
{
    let mut buffer = vec![0_u8; 64 * 1024];

    loop {
        ensure_not_cancelled(cancel_rx.as_ref())?;

        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            writer.flush().await?;
            break;
        }

        writer.write_all(&buffer[..read]).await?;
        *transferred += read as u64;
        on_progress(TransferProgress {
            transferred: *transferred,
            total: Some(total),
            current_path: Some(current_path.to_string()),
            completed_files: Some(completed_files),
            total_files: Some(total_files),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_remote_path_keeps_root_paths_clean() {
        assert_eq!(join_remote_path("/", "tmp"), "/tmp");
        assert_eq!(join_remote_path("/var", "log"), "/var/log");
        assert_eq!(join_remote_path("/srv/", "app"), "/srv/app");
    }

    #[tokio::test]
    async fn build_local_directory_plan_collects_nested_files() {
        let unique = format!(
            "rustdock-local-plan-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        fs::create_dir_all(root.join("nested"))
            .await
            .expect("create nested directory");
        fs::write(root.join("root.txt"), b"root")
            .await
            .expect("write root file");
        fs::write(root.join("nested").join("child.txt"), b"child")
            .await
            .expect("write child file");

        let plan = build_local_directory_plan(root.clone(), "/remote/root".to_string())
            .await
            .expect("build local plan");

        assert_eq!(plan.total_bytes, 9);
        assert_eq!(
            plan.directories,
            vec![
                "/remote/root".to_string(),
                "/remote/root/nested".to_string()
            ]
        );
        assert_eq!(plan.files.len(), 2);
        let remote_paths = plan
            .files
            .iter()
            .map(|file| file.remote_path.as_str())
            .collect::<Vec<_>>();
        assert!(remote_paths.contains(&"/remote/root/root.txt"));
        assert!(remote_paths.contains(&"/remote/root/nested/child.txt"));

        let _ = fs::remove_dir_all(root).await;
    }
}
