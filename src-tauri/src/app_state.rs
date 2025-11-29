use dashmap::DashMap;
use rusqlite::Connection;
use std::sync::{Arc, Mutex, RwLock};

/// Tracks the currently active file and its state
#[derive(Debug, Clone)]
pub struct CurrentFileState {
    pub path: String,
    pub offset: u32,
    pub limit: u32,
}

/// Application state that manages multiple database connections
/// Each connection is identified by its file path
pub struct AppState {
    pub connections: DashMap<String, Arc<Mutex<Connection>>>,
    /// The currently active file (shared across all windows)
    pub current_file: RwLock<Option<CurrentFileState>>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            connections: DashMap::new(),
            current_file: RwLock::new(None),
        }
    }

    /// Open or get an existing connection
    pub fn get_connection(&self, path: &str) -> Option<Arc<Mutex<Connection>>> {
        self.connections.get(path).map(|entry| entry.clone())
    }

    /// Add a new connection
    pub fn add_connection(&self, path: String, conn: Connection) {
        self.connections.insert(path, Arc::new(Mutex::new(conn)));
    }

    /// Remove a connection
    pub fn remove_connection(&self, path: &str) -> Option<Arc<Mutex<Connection>>> {
        self.connections.remove(path).map(|(_, conn)| conn)
    }

    /// Set the current active file
    pub fn set_current_file(&self, path: String, offset: u32, limit: u32) {
        *self.current_file.write().unwrap() = Some(CurrentFileState {
            path,
            offset,
            limit,
        });
    }

    /// Get the current active file state
    pub fn get_current_file(&self) -> Option<CurrentFileState> {
        self.current_file.read().unwrap().clone()
    }

    /// Clear the current file (e.g., when closing)
    pub fn clear_current_file(&self) {
        *self.current_file.write().unwrap() = None;
    }

    /// Update viewport position for current file
    pub fn update_viewport(&self, offset: u32, limit: u32) {
        if let Some(ref mut state) = *self.current_file.write().unwrap() {
            state.offset = offset;
            state.limit = limit;
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
