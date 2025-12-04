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
        // Iterate over all windows and emit to each
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

    /// Emit an event to a specific window
    pub fn emit_to_window<T: Serialize + Clone>(
        &self,
        window_id: &str,
        event: &str,
        payload: T,
    ) -> Result<(), String> {
        let window = self
            .app_handle
            .get_webview_window(window_id)
            .ok_or_else(|| format!("Window not found: {}", window_id))?;

        window
            .emit(event, payload)
            .map_err(|e| format!("Failed to emit event to window {}: {}", window_id, e))
    }

    /// Broadcast a layout change event
    pub fn broadcast_layout_change<T: Serialize + Clone>(
        &self,
        window_id: &str,
        layout: &T,
    ) -> Result<(), String> {
        #[derive(Serialize, Clone)]
        struct LayoutChangePayload<T: Serialize + Clone> {
            window_id: String,
            layout: T,
        }

        let payload = LayoutChangePayload {
            window_id: window_id.to_string(),
            layout: layout.clone(),
        };

        self.emit_to_all("layout:changed", payload)
    }

    /// Broadcast a settings change event
    pub fn broadcast_settings_change<T: Serialize>(&self, settings: &T) -> Result<(), String> {
        self.emit_to_all("settings:updated", settings)
    }

    /// Broadcast a theme change event
    pub fn broadcast_theme_change(&self, theme: &str) -> Result<(), String> {
        #[derive(Serialize, Clone)]
        struct ThemeChangePayload {
            theme: String,
        }

        let payload = ThemeChangePayload {
            theme: theme.to_string(),
        };

        self.emit_to_all("theme:changed", payload)
    }
}
