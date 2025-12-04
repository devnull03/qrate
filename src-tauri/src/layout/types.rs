use serde::{Deserialize, Serialize};

/// Chat sidebar display mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ChatMode {
    Docked,   // Right sidebar
    Panel,    // Bottom panel
    Detached, // Separate window
}

/// Sidebar state (left or right)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidebarState {
    pub visible: bool,
    pub width: u32,
    pub active_view_id: String,
}

impl Default for SidebarState {
    fn default() -> Self {
        SidebarState {
            visible: false,
            width: 250,
            active_view_id: String::new(),
        }
    }
}

/// Bottom panel state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelState {
    pub visible: bool,
    pub height: u32,
    pub active_tab: String,
}

impl Default for PanelState {
    fn default() -> Self {
        PanelState {
            visible: false,
            height: 300,
            active_tab: String::new(),
        }
    }
}

/// Chat sidebar state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSidebarState {
    pub visible: bool,
    pub width: u32,
    pub mode: ChatMode,
    pub detached_window_id: Option<String>,
}

impl Default for ChatSidebarState {
    fn default() -> Self {
        ChatSidebarState {
            visible: true,
            width: 360,
            mode: ChatMode::Docked,
            detached_window_id: None,
        }
    }
}

/// Editor area state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorAreaState {
    pub active_view: String,
}

impl Default for EditorAreaState {
    fn default() -> Self {
        EditorAreaState {
            active_view: "spreadsheet".to_string(),
        }
    }
}

/// Window bounds (position and size)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub maximized: bool,
}

impl Default for WindowBounds {
    fn default() -> Self {
        WindowBounds {
            x: 100.0,
            y: 100.0,
            width: 1200.0,
            height: 800.0,
            maximized: false,
        }
    }
}

/// Complete window layout state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowLayout {
    pub version: u32,
    pub window_id: String,
    pub workspace_path: Option<String>,
    pub left_sidebar: SidebarState,
    pub right_sidebar: SidebarState,
    pub bottom_panel: PanelState,
    pub chat_sidebar: ChatSidebarState,
    pub editor_area: EditorAreaState,
    pub window_bounds: WindowBounds,
}

impl Default for WindowLayout {
    fn default() -> Self {
        WindowLayout {
            version: 1,
            window_id: String::new(),
            workspace_path: None,
            left_sidebar: SidebarState::default(),
            right_sidebar: SidebarState::default(),
            bottom_panel: PanelState::default(),
            chat_sidebar: ChatSidebarState::default(),
            editor_area: EditorAreaState::default(),
            window_bounds: WindowBounds::default(),
        }
    }
}

impl WindowLayout {
    /// Create a new layout with a window ID
    pub fn new(window_id: String) -> Self {
        Self {
            window_id,
            ..Default::default()
        }
    }

    /// Create a layout for a detached chat window
    pub fn new_chat_window(window_id: String, source_window_id: String) -> Self {
        let mut layout = WindowLayout::new(window_id);
        layout.chat_sidebar.mode = ChatMode::Detached;
        layout.chat_sidebar.visible = true;
        layout.chat_sidebar.detached_window_id = Some(source_window_id);
        layout.left_sidebar.visible = false;
        layout.right_sidebar.visible = false;
        layout.bottom_panel.visible = false;
        layout.window_bounds.width = 500.0;
        layout.window_bounds.height = 700.0;
        layout
    }
}
