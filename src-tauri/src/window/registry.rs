use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// Information about an active window
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub window_id: String,
    pub window_label: String,
    pub workspace_path: Option<String>,
    pub is_main: bool,
    pub is_chat: bool,
}

/// Registry of active windows
pub struct WindowRegistry {
    windows: DashMap<String, WindowInfo>,
}

impl WindowRegistry {
    pub fn new() -> Self {
        WindowRegistry {
            windows: DashMap::new(),
        }
    }

    pub fn register(&self, info: WindowInfo) {
        self.windows.insert(info.window_id.clone(), info);
    }

    pub fn unregister(&self, window_id: &str) -> Option<WindowInfo> {
        self.windows.remove(window_id).map(|(_, info)| info)
    }

    pub fn get(&self, window_id: &str) -> Option<WindowInfo> {
        self.windows
            .get(window_id)
            .map(|entry| entry.value().clone())
    }

    pub fn get_all(&self) -> Vec<WindowInfo> {
        self.windows
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }
}

impl Default for WindowRegistry {
    fn default() -> Self {
        Self::new()
    }
}
