use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};
use crate::layout::types::WindowLayout;
use crate::layout::manager::LayoutManager;
use crate::window::registry::WindowRegistry;
use crate::window::ipc::WindowIPC;

/// Window manager that handles window lifecycle and layout coordination
pub struct WindowManager {
    registry: WindowRegistry,
    layout_manager: Arc<Mutex<LayoutManager>>,
    ipc: WindowIPC,
    app_handle: AppHandle,
}

impl WindowManager {
    /// Create a new window manager
    pub fn new(
        app_handle: AppHandle,
        layout_manager: Arc<Mutex<LayoutManager>>,
    ) -> Self {
        WindowManager {
            registry: WindowRegistry::new(),
            layout_manager,
            ipc: WindowIPC::new(app_handle.clone()),
            app_handle,
        }
    }

    /// Create a new main window with a layout
    pub fn create_main_window(
        &self,
        window_label: &str,
        workspace_path: Option<String>,
        initial_layout: Option<WindowLayout>,
    ) -> Result<String, String> {
        // Generate window ID if not provided in layout
        // Use timestamp-based ID for uniqueness
        let window_id = if let Some(ref layout) = initial_layout {
            layout.window_id.clone()
        } else {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis();
            format!("main_{}", timestamp)
        };

        // Create or load layout
        let layout = if let Some(layout) = initial_layout {
            layout
        } else {
            let mut layout = WindowLayout::new(window_id.clone());
            layout.workspace_path = workspace_path.clone();
            layout
        };

        // Validate layout
        let layout_mgr = self.layout_manager.lock().unwrap();
        layout_mgr
            .validate_layout(&layout)
            .map_err(|e| format!("Invalid layout: {}", e))?;
        drop(layout_mgr);

        // Save layout
        let layout_mgr = self.layout_manager.lock().unwrap();
        layout_mgr
            .save_layout_immediate(layout.clone())
            .map_err(|e| format!("Failed to save layout: {}", e))?;

        // Register window
        let window_info = crate::window::registry::WindowInfo {
            window_id: window_id.clone(),
            window_label: window_label.to_string(),
            workspace_path: workspace_path.clone(),
            is_main: true,
            is_chat: false,
        };
        self.registry.register(window_info);

        // Emit window created event
        #[derive(serde::Serialize, Clone)]
        struct WindowCreatedPayload {
            window_id: String,
            window_label: String,
        }

        self.ipc
            .emit_to_all(
                "window:created",
                WindowCreatedPayload {
                    window_id: window_id.clone(),
                    window_label: window_label.to_string(),
                },
            )
            .map_err(|e| format!("Failed to emit window:created event: {}", e))?;

        Ok(window_id)
    }

    /// Create a detached chat window
    pub fn create_chat_window(
        &self,
        source_window_id: &str,
    ) -> Result<String, String> {
        // Generate unique chat window ID using timestamp
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let window_id = format!("chat_{}", timestamp);
        let window_label = window_id.clone();

        // Create chat-specific layout
        let layout = WindowLayout::new_chat_window(window_id.clone(), source_window_id.to_string());

        // Save layout
        let layout_mgr = self.layout_manager.lock().unwrap();
        layout_mgr
            .save_layout_immediate(layout.clone())
            .map_err(|e| format!("Failed to save layout: {}", e))?;
        drop(layout_mgr);

        // Create the actual Tauri window
        let chat_window = tauri::WebviewWindowBuilder::new(
            &self.app_handle,
            &window_label,
            tauri::WebviewUrl::App("/chat".into()),
        )
        .title("qRate - Chat")
        .inner_size(layout.window_bounds.width, layout.window_bounds.height)
        .min_inner_size(300.0, 400.0)
        .decorations(false)
        .visible(true)
        .build()
        .map_err(|e| format!("Failed to create chat window: {}", e))?;

        // Set window position if specified
        if layout.window_bounds.x > 0.0 && layout.window_bounds.y > 0.0 {
            let _ = chat_window.set_position(tauri::Position::Logical(tauri::LogicalPosition {
                x: layout.window_bounds.x,
                y: layout.window_bounds.y,
            }));
        }

        // Register window
        let window_info = crate::window::registry::WindowInfo {
            window_id: window_id.clone(),
            window_label: window_label.clone(),
            workspace_path: None,
            is_main: false,
            is_chat: true,
        };
        self.registry.register(window_info);

        // Emit chat detached event
        #[derive(serde::Serialize, Clone)]
        struct ChatDetachedPayload {
            source_window_id: String,
            chat_window_id: String,
        }

        self.ipc
            .emit_to_all(
                "chat:detached",
                ChatDetachedPayload {
                    source_window_id: source_window_id.to_string(),
                    chat_window_id: window_id.clone(),
                },
            )
            .map_err(|e| format!("Failed to emit chat:detached event: {}", e))?;

        Ok(window_id)
    }

    /// Close a window and clean up its layout
    pub fn close_window(&self, window_id: &str) -> Result<(), String> {
        // Get window info
        let window_info = self
            .registry
            .get(window_id)
            .ok_or_else(|| format!("Window not found: {}", window_id))?;

        // If it's a chat window, emit reattach event
        if window_info.is_chat {
            // Get the layout to find the source window ID
            let layout_mgr = self.layout_manager.lock().unwrap();
            if let Ok(Some(layout)) = layout_mgr.get_layout(window_id) {
                if let Some(source_window_id) = layout.chat_sidebar.detached_window_id {
                    #[derive(serde::Serialize, Clone)]
                    struct ChatReattachedPayload {
                        source_window_id: String,
                    }

                    self.ipc
                        .emit_to_all(
                            "chat:reattached",
                            ChatReattachedPayload {
                                source_window_id,
                            },
                        )
                        .ok(); // Don't fail if event emission fails
                }
            }
        }

        // Delete layout
        let layout_mgr = self.layout_manager.lock().unwrap();
        layout_mgr
            .delete_layout(window_id)
            .map_err(|e| format!("Failed to delete layout: {}", e))?;

        // Unregister window
        self.registry.unregister(window_id);

        // Emit window closed event
        #[derive(serde::Serialize, Clone)]
        struct WindowClosedPayload {
            window_id: String,
        }

        self.ipc
            .emit_to_all(
                "window:closed",
                WindowClosedPayload {
                    window_id: window_id.to_string(),
                },
            )
            .ok(); // Don't fail if event emission fails

        Ok(())
    }

    /// Focus a window
    pub fn focus_window(&self, window_id: &str) -> Result<(), String> {
        let window = self
            .app_handle
            .get_webview_window(window_id)
            .ok_or_else(|| format!("Window not found: {}", window_id))?;

        window
            .set_focus()
            .map_err(|e| format!("Failed to focus window: {}", e))?;

        // Emit window focused event
        #[derive(serde::Serialize, Clone)]
        struct WindowFocusedPayload {
            window_id: String,
        }

        self.ipc
            .emit_to_all(
                "window:focused",
                WindowFocusedPayload {
                    window_id: window_id.to_string(),
                },
            )
            .ok(); // Don't fail if event emission fails

        Ok(())
    }

    /// Get window registry reference
    pub fn registry(&self) -> &WindowRegistry {
        &self.registry
    }

    /// Get layout manager reference
    pub fn layout_manager(&self) -> &Arc<Mutex<LayoutManager>> {
        &self.layout_manager
    }

    /// Get IPC helper reference
    pub fn ipc(&self) -> &WindowIPC {
        &self.ipc
    }
}

