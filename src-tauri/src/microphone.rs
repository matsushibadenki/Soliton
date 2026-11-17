use base64::{engine::general_purpose::STANDARD, Engine};
use tauri::AppHandle;

#[cfg(target_os = "macos")]
extern "C" {
    fn soliton_mic_permission() -> i32;
    fn soliton_mic_start() -> bool;
    fn soliton_mic_stop(keep: bool, out: *mut u8, capacity: usize) -> usize;
}

async fn on_main<T: Send + 'static>(
    app: AppHandle,
    operation: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(operation());
    })
    .map_err(|e| e.to_string())?;
    rx.await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn microphone_permission(app: AppHandle) -> Result<i32, String> {
    on_main(app, || unsafe { soliton_mic_permission() }).await
}

#[tauri::command]
pub async fn microphone_start(app: AppHandle) -> Result<(), String> {
    if on_main(app, || unsafe { soliton_mic_start() }).await? {
        Ok(())
    } else {
        Err("Unable to start the microphone".into())
    }
}

#[tauri::command]
pub async fn microphone_stop(app: AppHandle, keep: bool) -> Result<Vec<String>, String> {
    let wav = on_main(app, move || {
        let mut wav = if keep { vec![0; 960_000 + 4096] } else { Vec::new() };
        let length = unsafe { soliton_mic_stop(keep, wav.as_mut_ptr(), wav.len()) };
        wav.truncate(length);
        wav
    })
    .await?;
    if !keep {
        return Ok(Vec::new());
    }
    let pcm = pcm_data(&wav).ok_or("Missing microphone audio")?;
    Ok(pcm.chunks(32_000).map(|chunk| STANDARD.encode(chunk)).collect())
}

fn pcm_data(wav: &[u8]) -> Option<&[u8]> {
    if wav.get(..4)? != b"RIFF" || wav.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut position = 12;
    while let Some(header) = wav.get(position..position + 8) {
        let size = u32::from_le_bytes(header[4..8].try_into().ok()?) as usize;
        position += 8;
        let chunk = wav.get(position..position + size)?;
        if &header[..4] == b"data" {
            return (size > 0 && size % 2 == 0).then_some(chunk);
        }
        position += size + size % 2;
    }
    None
}
