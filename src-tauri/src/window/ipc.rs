use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// IPC helper for cross-window communication
pub struct WindowIPC {
    app_handle: AppHandle,
}

impl WindowIPC {
    pub fn new(app_handle: AppHandle) -> Self {
        WindowIPC { app_handle }
    }

    /// Emit an event to all windows
    pub fn emit_to_all<T: Serialize + Clone>(&self, event: &str, payload: T) -> Result<(), String> {
        for window in self.app_handle.webview_windows().values() {
            if let Err(e) = window.emit(event, payload.clone()) {
                eprintln!(
                    "Failed to emit {} to window {}: {}",
                    event,
                    window.label(),
                    e
                );
            }
        }
        Ok(())
    }
}
