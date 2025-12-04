use std::sync::{Arc, Mutex};
use crate::layout::manager::LayoutManager;
use crate::window::manager::WindowManager;

/// State container for layout and window managers
pub struct LayoutState {
    pub layout_manager: Arc<Mutex<LayoutManager>>,
    pub window_manager: Arc<Mutex<WindowManager>>,
}

impl LayoutState {
    pub fn new(
        layout_manager: Arc<Mutex<LayoutManager>>,
        window_manager: WindowManager,
    ) -> Self {
        LayoutState {
            layout_manager,
            window_manager: Arc::new(Mutex::new(window_manager)),
        }
    }
}

