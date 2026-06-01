use gpui::{prelude::FluentBuilder, *};
use gpui_component::{ActiveTheme, label::Label};

/// Approximate visual line height for text_sm. Used for Y-position → line index mapping.
const LINE_HEIGHT: f32 = 22.0;

fn line_color(line: &str, cx: &App) -> Hsla {
    let t = cx.theme();
    if line.starts_with("[ERROR]") {
        t.red
    } else if line.starts_with("[STDERR]") || line.starts_with("[WARN]") {
        t.yellow
    } else if line.starts_with("[PROMPT]") {
        t.blue
    } else if line.starts_with("[AUTO-ACCEPT]") {
        t.green
    } else if line.starts_with("[INFO]")
        || (line.starts_with("---") && line.ends_with("---"))
    {
        t.muted_foreground
    } else {
        t.foreground
    }
}

pub struct LogViewer {
    pub lines: Vec<String>,
    pub scroll_handle: ScrollHandle,
    selected_range: Option<(usize, usize)>,
    drag_anchor: Option<usize>,
    drag_active: bool,
    /// Y coordinate (window space) of the container top, inferred on first mouse-down.
    /// Used to convert subsequent mouse positions to line indices.
    container_top: Option<Pixels>,
    focus_handle: FocusHandle,
}

impl LogViewer {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            lines: Vec::new(),
            scroll_handle: ScrollHandle::new(),
            selected_range: None,
            drag_anchor: None,
            drag_active: false,
            container_top: None,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn append(&mut self, line: &str, cx: &mut Context<Self>) {
        self.lines.push(line.to_string());
        self.scroll_handle.scroll_to_bottom();
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.lines.clear();
        self.selected_range = None;
        self.drag_anchor = None;
        self.drag_active = false;
        self.container_top = None;
        cx.notify();
    }

    pub fn all_text(&self) -> String {
        self.lines.join("\n")
    }

    fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selected_range?;
        let end = end.min(self.lines.len().saturating_sub(1));
        if start > end {
            return None;
        }
        Some(self.lines[start..=end].join("\n"))
    }

    fn line_idx_from_event_y(&self, event_y: Pixels) -> usize {
        let top = self.container_top.unwrap_or(px(0.));
        // scroll offset is negative when scrolled down
        let offset_y = self.scroll_handle.offset().y;
        let content_y = (event_y - top - offset_y).to_f64() as f32;
        let idx = (content_y / LINE_HEIGHT).floor().max(0.) as usize;
        idx.min(self.lines.len().saturating_sub(1))
    }
}

impl Render for LogViewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_range = self.selected_range;

        // Collect display data before building the tree so closures don't borrow self.
        let lines_data: Vec<(String, Hsla)> = self
            .lines
            .iter()
            .map(|l| (l.clone(), line_color(l, cx)))
            .collect();

        let selection_bg = cx.theme().selection;
        let bg_color = cx.theme().input;
        let border_color = cx.theme().border;
        let focus_handle = self.focus_handle.clone();
        let scroll_handle = self.scroll_handle.clone();
        let weak = cx.weak_entity();

        div()
            .id("log-viewer")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&scroll_handle)
            .track_focus(&focus_handle)
            .key_context("LogViewer")
            .bg(bg_color)
            .border_1()
            .border_color(border_color)
            .rounded_lg()
            .p_2()
            // Keyboard: Ctrl+C copy, Ctrl+A select-all, Escape clear
            .on_key_down({
                let weak = weak.clone();
                move |event: &KeyDownEvent, _window, cx| {
                    let Some(entity) = weak.upgrade() else { return };
                    entity.update(cx, |this, cx| {
                        if event.keystroke.modifiers.control {
                            match event.keystroke.key.to_lowercase().as_str() {
                                "c" => {
                                    let text = this
                                        .selected_text()
                                        .unwrap_or_else(|| this.all_text());
                                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                                }
                                "a" => {
                                    if !this.lines.is_empty() {
                                        this.selected_range =
                                            Some((0, this.lines.len() - 1));
                                        cx.notify();
                                    }
                                }
                                _ => {}
                            }
                        } else if event.keystroke.key == "escape" {
                            this.selected_range = None;
                            cx.notify();
                        }
                    });
                }
            })
            // Mouse move: update drag selection, or clear drag if button released outside
            .on_mouse_move({
                let weak = weak.clone();
                move |event: &MouseMoveEvent, _window, cx| {
                    let Some(entity) = weak.upgrade() else { return };
                    entity.update(cx, |this, cx| {
                        if !this.drag_active {
                            return;
                        }
                        if event.pressed_button != Some(MouseButton::Left) {
                            this.drag_active = false;
                            cx.notify();
                            return;
                        }
                        if let Some(anchor) = this.drag_anchor {
                            if this.container_top.is_some() {
                                let idx = this.line_idx_from_event_y(event.position.y);
                                let (s, e) = if anchor <= idx {
                                    (anchor, idx)
                                } else {
                                    (idx, anchor)
                                };
                                if this.selected_range != Some((s, e)) {
                                    this.selected_range = Some((s, e));
                                    cx.notify();
                                }
                            }
                        }
                    });
                }
            })
            // Mouse up: end drag
            .on_mouse_up(MouseButton::Left, {
                let weak = weak.clone();
                move |_event, _window, cx| {
                    let Some(entity) = weak.upgrade() else { return };
                    entity.update(cx, |this, _cx| {
                        this.drag_active = false;
                    });
                }
            })
            // Per-line rows
            .children(lines_data.into_iter().enumerate().map(|(ix, (line, color))| {
                let is_selected = selected_range
                    .map(|(s, e)| ix >= s && ix <= e)
                    .unwrap_or(false);
                let weak = weak.clone();
                let focus_handle = focus_handle.clone();

                div()
                    .id(("log-line", ix as u64))
                    .w_full()
                    .px_1()
                    .when(is_selected, |s: gpui::Stateful<Div>| s.bg(selection_bg))
                    .on_mouse_down(MouseButton::Left, {
                        let weak = weak.clone();
                        move |event: &MouseDownEvent, window, cx| {
                            focus_handle.focus(window);
                            let Some(entity) = weak.upgrade() else { return };
                            entity.update(cx, |this, cx| {
                                // Infer the container's Y origin from this line's known index.
                                // scroll offset is negative when scrolled down.
                                let offset_y = this.scroll_handle.offset().y;
                                this.container_top = Some(
                                    event.position.y
                                        - px(ix as f32 * LINE_HEIGHT)
                                        - offset_y,
                                );
                                this.drag_anchor = Some(ix);
                                this.drag_active = true;
                                this.selected_range = Some((ix, ix));
                                cx.notify();
                            });
                        }
                    })
                    .child(Label::new(line).text_sm().text_color(color))
            }))
    }
}

impl Focusable for LogViewer {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
