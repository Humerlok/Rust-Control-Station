//! Tauri commands for the MAVFTP screen. Thin wrappers over `FtpService`; file contents cross
//! the IPC boundary as base64 so large files are not serialized as JSON number arrays.
use base64::{engine::general_purpose::STANDARD, Engine};
use control_station_link::{DirEntry, FtpService};
use tauri::State;

#[tauri::command]
pub async fn ftp_list(path: String, ftp: State<'_, FtpService>) -> Result<Vec<DirEntry>, String> {
    ftp.list(&path).await.map_err(|e| e.to_string())
}

/// Downloads a file; the result is its bytes, base64 encoded.
#[tauri::command]
pub async fn ftp_download(path: String, ftp: State<'_, FtpService>) -> Result<String, String> {
    let bytes = ftp.read(&path).await.map_err(|e| e.to_string())?;
    Ok(STANDARD.encode(bytes))
}

/// Uploads base64 `data` to `path`, replacing it; `verify` compares the vehicle's CRC32.
#[tauri::command]
pub async fn ftp_upload(
    path: String,
    data: String,
    verify: bool,
    ftp: State<'_, FtpService>,
) -> Result<(), String> {
    let bytes = STANDARD
        .decode(data.as_bytes())
        .map_err(|e| format!("invalid file data: {e}"))?;
    ftp.write(&path, &bytes, verify)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ftp_remove(path: String, ftp: State<'_, FtpService>) -> Result<(), String> {
    ftp.remove(&path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ftp_mkdir(path: String, ftp: State<'_, FtpService>) -> Result<(), String> {
    ftp.mkdir(&path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ftp_rmdir(path: String, ftp: State<'_, FtpService>) -> Result<(), String> {
    ftp.rmdir(&path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ftp_rename(
    from: String,
    to: String,
    ftp: State<'_, FtpService>,
) -> Result<(), String> {
    ftp.rename(&from, &to).await.map_err(|e| e.to_string())
}

/// The vehicle-side CRC32 of a file, as lowercase hex.
#[tauri::command]
pub async fn ftp_crc(path: String, ftp: State<'_, FtpService>) -> Result<String, String> {
    let crc = ftp.crc32(&path).await.map_err(|e| e.to_string())?;
    Ok(format!("{crc:08x}"))
}

#[tauri::command]
pub fn ftp_cancel(ftp: State<'_, FtpService>) {
    ftp.cancel();
}
