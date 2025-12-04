use dashmap::DashMap;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::layout::persistence::{load_layout, open_layout_database, save_layout};
use crate::layout::types::WindowLayout;

/// Layout manager that handles layout state caching and persistence
pub struct LayoutManager {
    /// In-memory cache of layouts by window ID
    layouts: DashMap<String, WindowLayout>,
    /// Database connection for persistence
    db: Arc<Mutex<Connection>>,
    /// Last save time per window (for debouncing)
    last_save: DashMap<String, Instant>,
    /// Debounce interval (300ms as per plan)
    debounce_interval: Duration,
}

impl LayoutManager {
    /// Create a new layout manager with a database connection
    pub fn new(db_path: &Path) -> Result<Self, String> {
        let conn = open_layout_database(db_path)
            .map_err(|e| format!("Failed to open layout database: {}", e))?;

        Ok(LayoutManager {
            layouts: DashMap::new(),
            db: Arc::new(Mutex::new(conn)),
            last_save: DashMap::new(),
            debounce_interval: Duration::from_millis(300),
        })
    }

    /// Get a layout by window ID (from cache or database)
    pub fn get_layout(&self, window_id: &str) -> Result<Option<WindowLayout>, String> {
        // Check cache first
        if let Some(layout) = self.layouts.get(window_id) {
            return Ok(Some(layout.clone()));
        }

        // Load from database
        let db = self
            .db
            .lock()
            .map_err(|e| format!("Database lock error: {}", e))?;
        match load_layout(&db, window_id) {
            Ok(Some(layout)) => {
                // Cache it
                self.layouts.insert(window_id.to_string(), layout.clone());
                Ok(Some(layout))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(format!("Failed to load layout: {}", e)),
        }
    }

    /// Save a layout (with debouncing)
    pub fn save_layout(&self, layout: WindowLayout) -> Result<(), String> {
        let window_id = layout.window_id.clone();

        // Update cache immediately
        self.layouts.insert(window_id.clone(), layout.clone());

        // Check if we should debounce
        let now = Instant::now();
        if let Some(last_save_time) = self.last_save.get(&window_id) {
            if now.duration_since(*last_save_time) < self.debounce_interval {
                // Too soon, skip save (will be saved on next call or flush)
                return Ok(());
            }
        }

        // Save to database
        self.save_layout_immediate(layout)?;
        self.last_save.insert(window_id, now);

        Ok(())
    }

    /// Save a layout immediately (bypasses debouncing)
    pub fn save_layout_immediate(&self, layout: WindowLayout) -> Result<(), String> {
        let db = self
            .db
            .lock()
            .map_err(|e| format!("Database lock error: {}", e))?;
        save_layout(&db, &layout).map_err(|e| format!("Failed to save layout: {}", e))?;
        Ok(())
    }

    /// Flush all pending saves (force immediate save)
    pub fn flush(&self) -> Result<(), String> {
        let layouts_to_save: Vec<WindowLayout> = self
            .layouts
            .iter()
            .map(|entry| entry.value().clone())
            .collect();

        for layout in layouts_to_save {
            self.save_layout_immediate(layout)?;
        }

        Ok(())
    }

    /// Update a specific region size in a layout
    pub fn update_region_size(
        &self,
        window_id: &str,
        region: &str,
        size: u32,
    ) -> Result<(), String> {
        let mut layout = self
            .get_layout(window_id)?
            .ok_or_else(|| format!("Layout not found for window: {}", window_id))?;

        match region {
            "left_sidebar" => layout.left_sidebar.width = size,
            "right_sidebar" => layout.right_sidebar.width = size,
            "bottom_panel" => layout.bottom_panel.height = size,
            "chat_sidebar" => layout.chat_sidebar.width = size,
            _ => return Err(format!("Unknown region: {}", region)),
        }

        self.save_layout(layout)
    }

    /// Toggle a region's visibility
    pub fn toggle_region(&self, window_id: &str, region: &str) -> Result<(), String> {
        let mut layout = self
            .get_layout(window_id)?
            .ok_or_else(|| format!("Layout not found for window: {}", window_id))?;

        match region {
            "left_sidebar" => layout.left_sidebar.visible = !layout.left_sidebar.visible,
            "right_sidebar" => layout.right_sidebar.visible = !layout.right_sidebar.visible,
            "bottom_panel" => layout.bottom_panel.visible = !layout.bottom_panel.visible,
            "chat_sidebar" => layout.chat_sidebar.visible = !layout.chat_sidebar.visible,
            _ => return Err(format!("Unknown region: {}", region)),
        }

        self.save_layout(layout)
    }

    /// Set chat mode for a window
    pub fn set_chat_mode(
        &self,
        window_id: &str,
        mode: crate::layout::types::ChatMode,
    ) -> Result<(), String> {
        let mut layout = self
            .get_layout(window_id)?
            .ok_or_else(|| format!("Layout not found for window: {}", window_id))?;

        layout.chat_sidebar.mode = mode;
        self.save_layout(layout)
    }

    /// Delete a layout (when window is closed)
    pub fn delete_layout(&self, window_id: &str) -> Result<(), String> {
        // Remove from cache
        self.layouts.remove(window_id);
        self.last_save.remove(window_id);

        // Remove from database
        let db = self
            .db
            .lock()
            .map_err(|e| format!("Database lock error: {}", e))?;
        crate::layout::persistence::delete_layout(&db, window_id)
            .map_err(|e| format!("Failed to delete layout: {}", e))?;

        Ok(())
    }

    /// Validate layout constraints (min/max sizes)
    pub fn validate_layout(&self, layout: &WindowLayout) -> Result<(), String> {
        // Minimum sizes
        const MIN_SIDEBAR_WIDTH: u32 = 100;
        const MAX_SIDEBAR_WIDTH: u32 = 800;
        const MIN_PANEL_HEIGHT: u32 = 50;
        const MAX_PANEL_HEIGHT: u32 = 600;
        const MIN_CHAT_WIDTH: u32 = 200;
        const MAX_CHAT_WIDTH: u32 = 1000;

        if layout.left_sidebar.width < MIN_SIDEBAR_WIDTH
            || layout.left_sidebar.width > MAX_SIDEBAR_WIDTH
        {
            return Err(format!(
                "Left sidebar width must be between {} and {}",
                MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH
            ));
        }

        if layout.right_sidebar.width < MIN_SIDEBAR_WIDTH
            || layout.right_sidebar.width > MAX_SIDEBAR_WIDTH
        {
            return Err(format!(
                "Right sidebar width must be between {} and {}",
                MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH
            ));
        }

        if layout.bottom_panel.height < MIN_PANEL_HEIGHT
            || layout.bottom_panel.height > MAX_PANEL_HEIGHT
        {
            return Err(format!(
                "Bottom panel height must be between {} and {}",
                MIN_PANEL_HEIGHT, MAX_PANEL_HEIGHT
            ));
        }

        if layout.chat_sidebar.width < MIN_CHAT_WIDTH || layout.chat_sidebar.width > MAX_CHAT_WIDTH
        {
            return Err(format!(
                "Chat sidebar width must be between {} and {}",
                MIN_CHAT_WIDTH, MAX_CHAT_WIDTH
            ));
        }

        Ok(())
    }
}
