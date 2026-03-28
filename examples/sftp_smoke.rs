use rustdock_core::domain::{now_timestamp, AuthMethod, SessionProfile, SessionSyncState};
use rustdock_core::sftp::{
    create_remote_dir, delete_remote_path, download_remote_file, list_remote_dir, rename_remote_path,
    upload_local_file,
};
use std::env;
use std::fs;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = required_env("SSH_SMOKE_HOST")?;
    let username = required_env("SSH_SMOKE_USER")?;
    let password = required_env("SSH_SMOKE_PASSWORD")?;
    let port = env::var("SSH_SMOKE_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(22);

    let profile = SessionProfile {
        id: "sftp-smoke".to_string(),
        name: "SFTP Smoke".to_string(),
        host,
        port,
        username,
        auth_method: AuthMethod::Password,
        tags: vec!["smoke".to_string()],
        notes: "Transient SFTP smoke test profile".to_string(),
        local_roots: Vec::new(),
        remote_roots: Vec::new(),
        created_at: now_timestamp(),
        updated_at: now_timestamp(),
        last_connected_at: None,
        sync_state: SessionSyncState::LocalOnly,
    };

    let listing = list_remote_dir(profile.clone(), "/tmp".to_string(), Some(password.clone())).await?;
    println!("listed {} entries from {}", listing.entries.len(), listing.directory);

    let marker = format!("RUSTDOCK_SFTP_{}", now_timestamp());
    let timestamp = now_timestamp();
    let local_source = format!("/tmp/rustdock-sftp-source-{timestamp}.txt");
    let local_download = format!("/tmp/rustdock-sftp-download-{timestamp}.txt");
    let remote_directory = format!("/tmp/rustdock-sftp-dir-{timestamp}");
    let remote_target = format!("{remote_directory}/upload.txt");
    let remote_renamed = format!("{remote_directory}/upload-renamed.txt");

    fs::write(&local_source, &marker)?;

    create_remote_dir(
        profile.clone(),
        remote_directory.clone(),
        Some(password.clone()),
    )
    .await?;
    println!("created directory {}", remote_directory);

    let upload = upload_local_file(
        profile.clone(),
        local_source.clone(),
        remote_target.clone(),
        Some(password.clone()),
    )
    .await?;
    println!("uploaded {} bytes to {}", upload.bytes, upload.path);

    let renamed = rename_remote_path(
        profile.clone(),
        remote_target.clone(),
        remote_renamed.clone(),
        Some(password.clone()),
    )
    .await?;
    println!("renamed remote file to {}", renamed.path);

    let download = download_remote_file(
        profile.clone(),
        remote_renamed.clone(),
        local_download.clone(),
        Some(password.clone()),
    )
    .await?;
    println!("downloaded {} bytes to {}", download.bytes, download.path);

    let downloaded = fs::read_to_string(&local_download)?;
    if downloaded.trim() != marker {
        return Err(format!(
            "downloaded content mismatch: expected {marker}, got {}",
            downloaded.trim()
        )
        .into());
    }

    delete_remote_path(
        profile.clone(),
        remote_renamed.clone(),
        false,
        Some(password.clone()),
    )
    .await?;
    delete_remote_path(
        profile,
        remote_directory.clone(),
        true,
        Some(password),
    )
    .await?;

    let _ = fs::remove_file(&local_source);
    let _ = fs::remove_file(&local_download);

    println!("sftp smoke test passed");
    Ok(())
}

fn required_env(name: &str) -> Result<String, Box<dyn std::error::Error>> {
    env::var(name)
        .map_err(|_| format!("missing required environment variable {name}").into())
}
