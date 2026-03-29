use crate::domain::SessionProfile;
use crate::ssh::{connect_authenticated, TrustOnFirstUseHandler};
use anyhow::{anyhow, Context, Result};
use russh_sftp::client::SftpSession;
use serde::Serialize;
use std::path::Path;
use tokio::fs::File as TokioFile;
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
}

const MAX_TEXT_FILE_BYTES: u64 = 1024 * 1024;

pub async fn list_remote_dir(
    profile: SessionProfile,
    path: String,
    secret: Option<String>,
) -> Result<RemoteDirectoryListing> {
    let (sftp, _driver) = open_sftp(profile, secret.as_deref()).await?;
    let directory = sftp.canonicalize(&path).await.unwrap_or(path);
    let mut entries = Vec::new();

    let read_dir = sftp
        .read_dir(directory.clone())
        .await
        .with_context(|| format!("failed to list remote directory {directory}"))?;

    for entry in read_dir {
        let name = entry.file_name();
        let kind = format!("{:?}", entry.file_type());
        let is_dir = kind == "Directory";
        entries.push(RemoteDirEntry {
            path: join_remote_path(&directory, &name),
            name,
            kind,
            is_dir,
            size: None,
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
    download_remote_file_with_progress(profile, remote_path, local_path, secret, None, |_| {})
        .await
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
    let (sftp, _driver) = open_sftp(profile, secret.as_deref()).await?;
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
    let (sftp, _driver) = open_sftp(profile, secret.as_deref()).await?;
    let total = tokio::fs::metadata(&local_path).await.ok().map(|metadata| metadata.len());
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

pub async fn create_remote_dir(
    profile: SessionProfile,
    remote_path: String,
    secret: Option<String>,
) -> Result<MutationResult> {
    let (sftp, _driver) = open_sftp(profile, secret.as_deref()).await?;
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
    let (sftp, _driver) = open_sftp(profile, secret.as_deref()).await?;
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

    let (sftp, _driver) = open_sftp(profile, secret.as_deref()).await?;
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
    let (sftp, _driver) = open_sftp(profile, secret.as_deref()).await?;
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
    let (sftp, _driver) = open_sftp(profile, secret.as_deref()).await?;
    if is_dir {
        sftp.remove_dir(&remote_path)
            .await
            .with_context(|| format!("failed to remove remote directory {remote_path}"))?;
    } else {
        sftp.remove_file(&remote_path)
            .await
            .with_context(|| format!("failed to remove remote file {remote_path}"))?;
    }

    Ok(MutationResult { path: remote_path })
}

async fn open_sftp(
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
    on_progress(TransferProgress { transferred, total });

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
        on_progress(TransferProgress { transferred, total });
    }

    Ok(transferred)
}
