use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    description_list::DescriptionList,
    dock::{Panel, PanelControl, PanelEvent},
    scroll::ScrollableElement,
    table::TableState,
};
use table::{QrateTableDelegate, Selection, TableChanged, TableStateHandle};

/// Left dock: the selected row's photo (if the files folder resolved one) plus its fields as a
/// label/value list, per the main-workspace design.
pub struct DetailsPanel {
    focus_handle: FocusHandle,
    /// Live table state, read for the selected row.
    state: Option<WeakEntity<TableState<QrateTableDelegate>>>,
    /// Re-binds `state` whenever `TablePanel` publishes a new table (project reload, dock
    /// layout restore rebuilding panels) — without this, a panel constructed before the table
    /// would point at a dead entity forever.
    _handle_sub: Subscription,
    /// Re-renders on any table change (selection, edits, column moves).
    _table_sub: Option<Subscription>,
}

impl DetailsPanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _handle_sub = cx.observe_global::<TableStateHandle>(|this: &mut Self, cx| {
            this.bind(cx);
            cx.notify();
        });
        let mut this = Self {
            focus_handle: cx.focus_handle(),
            state: None,
            _handle_sub,
            _table_sub: None,
        };
        this.bind(cx);
        this
    }

    fn bind(&mut self, cx: &mut Context<Self>) {
        self.state = cx.try_global::<TableStateHandle>().map(|h| h.0.clone());
        self._table_sub =
            self.state.as_ref().and_then(|w| w.upgrade()).map(|entity| {
                cx.subscribe(&entity, |_this, _st, _ev: &TableChanged, cx| cx.notify())
            });
    }
}

impl Focusable for DetailsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for DetailsPanel {}

impl Panel for DetailsPanel {
    fn panel_name(&self) -> &'static str {
        "DetailsPanel"
    }

    fn title(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Details")
    }

    // The library always renders the ⋯ menu button; these just empty it of Close + Zoom.
    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }
}

/// The image frame: the selected row's photo if one resolved, else a muted placeholder icon —
/// same rounded/bordered box either way. When a photo is shown, two ghost icon buttons overlay
/// the top-right: open in the OS default viewer, reveal in the file manager (plain shell-outs,
/// no panel state touched). `gpui`'s image cache is keyed by resource path, so switching back
/// to a previously-viewed row's image is served from cache rather than re-decoded from disk.
///
/// Returns `AnyElement` (erased), not `impl IntoElement` — this is called from several `Render`
/// impls (the real panel plus test probes), and gpui's chained builder calls produce deeply
/// nested generic types; propagating that concrete type into every caller overflowed rustc's
/// stack during type-checking instead of just hitting a slow compile.
fn render_image_frame(image_path: Option<PathBuf>, cx: &App) -> AnyElement {
    // Also `img()`'s missing-file fallback; captures the color because the `'static` fallback
    // closure can't borrow `cx`.
    let placeholder = {
        let color = cx.theme().muted_foreground;
        move || {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(color)
                .child(Icon::new(IconName::File).size_6())
                .into_any_element()
        }
    };

    let action = |id: &'static str, icon: IconName, tip: &'static str, path: PathBuf| {
        Button::new(id)
            .icon(icon)
            .ghost()
            .small()
            .tooltip(tip)
            .on_click(move |_, _, _| {
                let result = match id {
                    "open-image" => settings::os_open::open_in_default_app(&path),
                    _ => settings::os_open::reveal_in_folder(&path),
                };
                if let Err(err) = result {
                    eprintln!("image action {id} failed: {err}");
                }
            })
    };

    div()
        .relative()
        .w_full()
        .h(px(180.))
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .overflow_hidden()
        .bg(cx.theme().muted)
        .map(|frame| match image_path {
            Some(path) => frame
                .child(
                    img(path.clone())
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                        .with_fallback(placeholder),
                )
                .child(
                    div()
                        .absolute()
                        .top_1()
                        .right_1()
                        .flex()
                        .gap_1()
                        .bg(cx.theme().background.opacity(0.7))
                        .child(action(
                            "open-image",
                            IconName::ExternalLink,
                            "Open in default app",
                            path.clone(),
                        ))
                        .child(action(
                            "reveal-image",
                            IconName::FolderOpen,
                            "Reveal in folder",
                            path,
                        )),
                ),
            None => frame.child(placeholder()),
        })
        .into_any_element()
}

impl Render for DetailsPanel {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selection = self.state.as_ref().and_then(|w| w.upgrade()).and_then(|s| {
            let state = s.read(cx);
            let row = match state.delegate().selection()? {
                Selection::Cell { row, .. } | Selection::Row(row) => row,
                // A whole-column selection has no single row to detail.
                Selection::Column(_) => return None,
            };
            let delegate = state.delegate();
            let image = delegate.row_image(row).map(Path::to_path_buf);
            Some((delegate.row_fields(row), image))
        });

        let content = match selection {
            Some((fields, image_path)) if !fields.is_empty() => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(render_image_frame(image_path, cx))
                .child(
                    fields.into_iter().fold(
                        DescriptionList::horizontal()
                            .columns(1)
                            .bordered(false)
                            .label_width(px(110.)),
                        |list, (k, v)| list.item(k, v, 1),
                    ),
                )
                .into_any_element(),
            _ => div()
                .text_color(cx.theme().muted_foreground)
                .child("No selection")
                .into_any_element(),
        };

        // Scroll vertically when a long field list overflows the dock height, rather than
        // clipping it (`overflow_hidden`).
        div()
            .size_full()
            .overflow_y_scrollbar()
            .p_3()
            .child(content)
    }
}

#[cfg(test)]
mod tests {
    // No `use super::*` here: it would chain-glob `gpui::*`, whose `test` proc-macro shadows
    // the built-in `#[test]` that `#[gpui::test]`'s expansion emits — making the macro expand
    // into itself until rustc's recursion limit (and then its stack) blows.
    use std::path::{Path, PathBuf};

    use gpui::{Context, IntoElement, Render, TestAppContext, Window};

    use super::{DetailsPanel, render_image_frame};

    /// Wraps `render_image_frame` in a root `Render` view so a test can actually draw it —
    /// `Img`'s real load/fallback logic runs during layout/paint, not at element construction,
    /// so building the element tree alone (without a window draw) wouldn't exercise it.
    struct ImageFrameProbe(Option<PathBuf>);

    impl Render for ImageFrameProbe {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            render_image_frame(self.0.clone(), cx)
        }
    }

    fn sample_photo(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../sample/photos")
            .join(name)
            .canonicalize()
            .expect("sample/photos present in repo")
    }

    #[gpui::test]
    fn renders_a_resolved_image_without_panicking(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let path = sample_photo("1.jpg");
        cx.add_window_view(|_, _| ImageFrameProbe(Some(path)));
    }

    #[gpui::test]
    fn falls_back_when_the_resolved_path_is_missing_on_disk(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let path = PathBuf::from("/nonexistent/qrate-test-image.jpg");
        cx.add_window_view(|_, _| ImageFrameProbe(Some(path)));
    }

    #[gpui::test]
    fn renders_the_no_image_placeholder_without_a_path(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        cx.add_window_view(|_, _| ImageFrameProbe(None));
    }

    #[gpui::test]
    fn details_panel_shows_no_selection_without_a_table(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        // No `TableStateHandle` global set — `DetailsPanel::bind` finds nothing, so `render`
        // takes the "No selection" branch. Mirrors dev-launch-with-no-project.
        cx.add_window_view(DetailsPanel::new);
    }
}
