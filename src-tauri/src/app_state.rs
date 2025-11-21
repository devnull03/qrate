use dashmap::DashMap;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

/// Application state that manages multiple database connections
/// Each connection is identified by its file path
pub struct AppState {
    pub connections: DashMap<String, Arc<Mutex<Connection>>>,
}

impl AppState {
    pub fn new() -> Self {
        AppState {
            connections: DashMap::new(),
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

    /// Check if a connection exists
    pub fn has_connection(&self, path: &str) -> bool {
        self.connections.contains_key(path)
    }

    /// Get all open file paths
    pub fn get_open_files(&self) -> Vec<String> {
        self.connections
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Close all connections
    pub fn close_all(&self) {
        self.connections.clear();
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
