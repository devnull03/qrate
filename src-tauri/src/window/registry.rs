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

    /// Register a new window
    pub fn register(&self, info: WindowInfo) {
        self.windows.insert(info.window_id.clone(), info);
    }

    /// Unregister a window
    pub fn unregister(&self, window_id: &str) -> Option<WindowInfo> {
        self.windows.remove(window_id).map(|(_, info)| info)
    }

    /// Get window info by ID
    pub fn get(&self, window_id: &str) -> Option<WindowInfo> {
        self.windows.get(window_id).map(|entry| entry.value().clone())
    }

    /// Get all registered windows
    pub fn get_all(&self) -> Vec<WindowInfo> {
        self.windows.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Get all main windows
    pub fn get_main_windows(&self) -> Vec<WindowInfo> {
        self.windows
            .iter()
            .filter(|entry| entry.value().is_main)
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get all chat windows
    pub fn get_chat_windows(&self) -> Vec<WindowInfo> {
        self.windows
            .iter()
            .filter(|entry| entry.value().is_chat)
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Check if a window is registered
    pub fn contains(&self, window_id: &str) -> bool {
        self.windows.contains_key(window_id)
    }

    /// Get count of registered windows
    pub fn count(&self) -> usize {
        self.windows.len()
    }
}

impl Default for WindowRegistry {
    fn default() -> Self {
        Self::new()
    }
}

