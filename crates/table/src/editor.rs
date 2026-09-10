//! The floating editor: the card the grid and the Details panel open over the value being edited.
//!
//! It renders `gpui-base`'s `TextareaState` directly rather than through
//! `gpui_component::input::Textarea`. The component wrapper hard-codes its 10x8px medium inset and
//! offers no way to change it, so aligning the editor's first glyph with the cell's meant pushing
//! the textarea outside the card with negative offsets — which took its scrollbar out with it.
//! Owning the frame here lets the padding be set on the state itself, so text and scrollbar both
//! land where a cell-sized box expects them.
//!
//! Editing is still entirely gpui-base's: IME, selection, clipboard, undo, wrapping, scrolling and
//! shaping all live in `TextareaState`. What this module owns is the frame, its exact inset, the
//! text menu, the accessibility metadata and the spreadsheet growth rule.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_base::input::{InputBase, InputContextMenuCapabilities, TextareaState};
use gpui_component::ActiveTheme as _;
use gpui_component::native_menu::NativeMenu;

use crate::cell;

/// What gpui-base lays a multi-line editor's lines out at, and so what a wrapped height has to be
/// counted in. Set on the frame because the state inherits it from the ambient text style.
const LINE_HEIGHT: Rems = Rems(1.25);

/// The editor's own padding — the cell's, so the text sits exactly where it did unedited — plus the
/// 1px border on each side, plus the `RIGHT_MARGIN` a soft-wrapping textarea subtracts from its own
/// bounds before shaping. Leave that last term out and the box is 10px wider than the width the
/// text was measured at, so the final word wraps into a second line the box has no room for.
const PAD_X: f32 = cell::CELL_PAD_X * 2. + 2. + TEXTAREA_RIGHT_MARGIN;

/// `gpui_base::input::element::RIGHT_MARGIN`, which is private.
const TEXTAREA_RIGHT_MARGIN: f32 = 10.;
const PAD_Y: f32 = cell::CELL_PAD_Y * 2. + 2.;

/// Give `editor` the cell's inset and the platform text menu. Neither depends on the theme or the
/// value, so this runs once where the state is built instead of on every render. Colours are not
/// set at all: gpui-base resolves an unset `InputEditorStyle` against the live theme each frame,
/// and gpui-component keeps that theme in step with `cx.theme()`.
pub fn configure(editor: &Entity<TextareaState>, cx: &mut App) {
    editor.update(cx, |editor, _| {
        editor.set_editor_paddings(Edges {
            top: px(cell::CELL_PAD_Y),
            right: px(cell::CELL_PAD_X),
            bottom: px(cell::CELL_PAD_Y),
            left: px(cell::CELL_PAD_X),
        });
        editor.on_context_menu(Rc::new(text_menu));
    });
}

/// The native Cut/Copy/Paste/Select All menu. Row, column, plugin and suggestion actions stay on
/// the grid's own menu — this one is about the text under the pointer.
///
/// gpui-base hands over a presentation-independent menu model with no way to show itself; the menu
/// that can is gpui-component's, so the offered one goes unused, exactly as `Input` does with it.
fn text_menu(
    _: gpui_base::input::NativeMenu,
    can: InputContextMenuCapabilities,
    at: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let editable = can.is_editable();
    NativeMenu::new()
        .menu_with_disabled(
            "Cut",
            !(editable && can.is_copyable()),
            Box::new(gpui_base::input::Cut),
        )
        .menu_with_disabled("Copy", !can.is_copyable(), Box::new(gpui_base::input::Copy))
        .menu_with_disabled(
            "Paste",
            !(editable && cx.read_from_clipboard().is_some()),
            Box::new(gpui_base::input::Paste),
        )
        .separator()
        .menu("Select All", Box::new(gpui_base::input::SelectAll))
        .show(at, window, cx);
}

/// The app's one floating-editor box: a bordered, shadowed card sized Google-Sheets style to
/// `anchor` within `within`, holding nothing but the editor. The grid and the Details panel share
/// it so an edit looks and grows the same wherever it opens.
///
/// `label` names the edited field for a screen reader — the column and row for a cell, the column
/// for a header rename, the field name in Details.
///
/// The size comes back with it because a caller hanging anything below the box (the grid's
/// suggestion list) needs a height it can't compute itself. Place the result with
/// `deferred(float_at(anchor.origin, within, box))`.
pub fn editor_box(
    editor: &Entity<TextareaState>,
    label: SharedString,
    anchor: Bounds<Pixels>,
    within: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> (InputBase, Size<Pixels>) {
    let text_style = window.text_style();
    let font_size = text_style.font_size.to_pixels(window.rem_size());
    let line_height = window.rem_size() * LINE_HEIGHT.0;
    let value = editor.read(cx).value();
    let size = measured_size(
        Measurement {
            value: value.clone(),
            font: text_style.font(),
            font_size,
            line_height,
            anchor,
            within,
        },
        window,
    );

    // Materializing the value for the accessibility tree is only observable to a client that is
    // listening, so skip it when none is.
    let accessible_value = window.is_a11y_active().then(|| value.to_string());
    let set_value = editor.clone();

    let frame = InputBase::new(("qrate-editor", editor.entity_id()))
        .role(Role::MultilineTextInput)
        .accessibility_label(label)
        .when_some(accessible_value, |frame, value| frame.aria_value(value))
        .on_a11y_action(AccessibleAction::SetValue, move |data, window, cx| {
            let Some(accesskit::ActionData::Value(value)) = data else {
                return;
            };
            set_value.update(cx, |editor, cx| {
                editor.replace_all(value.to_string(), window, cx)
            });
        })
        .relative()
        .w(size.width)
        .h(size.height)
        .occlude()
        .bg(cx.theme().background)
        .border_1()
        .border_color(cx.theme().primary)
        .rounded(cx.theme().radius)
        .shadow_lg()
        .line_height(LINE_HEIGHT)
        .text_size(font_size)
        // The state lays itself out with `flex_1` and `h_full`, so it needs a flex parent that is
        // the card's inner box exactly — the padding it applies is its own, inside this.
        .child(
            div()
                .flex()
                .size_full()
                .child(div().relative().flex_1().child(editor.clone())),
        );
    (frame, size)
}

/// Everything the box's size is a function of. Also the cache key: while an edit is open the box
/// re-renders on every repaint of the table behind it, and remeasuring costs up to fourteen full
/// wraps of the value.
#[derive(Clone, PartialEq)]
struct Measurement {
    value: SharedString,
    font: Font,
    font_size: Pixels,
    line_height: Pixels,
    anchor: Bounds<Pixels>,
    within: Bounds<Pixels>,
}

thread_local! {
    /// One entry: there is one open edit at a time. A second editor rendering in the same frame
    /// just misses, which costs what not caching did.
    static LAST: RefCell<Option<(Measurement, Size<Pixels>)>> = const { RefCell::new(None) };
}

fn measured_size(at: Measurement, window: &mut Window) -> Size<Pixels> {
    if let Some(hit) = LAST.with_borrow(|last| {
        last.as_ref()
            .filter(|(key, _)| *key == at)
            .map(|(_, size)| *size)
    }) {
        return hit;
    }
    let size = measure(&at, window);
    LAST.with_borrow_mut(|last| *last = Some((at, size)));
    size
}

fn measure(at: &Measurement, window: &mut Window) -> Size<Pixels> {
    let hard_lines = at.value.split('\n').count();
    // gpui-base's textarea wraps with `LineWrapper`, whose per-character width accounting is
    // deliberately different from `shape_text`. Measuring with the latter can say a value fits
    // while the actual editor moves its final word onto another row. Ask the same wrapper the
    // editor uses for both its unwrapped width threshold and its final display-line count.
    let mut line_wrapper = window
        .text_system()
        .line_wrapper(at.font.clone(), at.font_size);
    let mut wrapped_lines = |wrap_w| {
        at.value
            .split('\n')
            .map(|line| {
                let fragments = [LineFragment::text(line)];
                1 + line_wrapper.wrap_line(&fragments, wrap_w).count()
            })
            .sum()
    };
    let available_content_w = (at.within.right() - at.anchor.origin.x - px(PAD_X)).max(px(1.));
    let cell_content_w = (at.anchor.size.width - px(PAD_X))
        .max(px(1.))
        .min(available_content_w);
    let natural_w = unwrapped_width(
        cell_content_w,
        available_content_w,
        hard_lines,
        &mut wrapped_lines,
    );
    editor_size(
        at.anchor,
        at.within,
        natural_w,
        at.line_height,
        &mut wrapped_lines,
    )
}

/// Google-Sheets box sizing. Text that fits gets a cell-sized box; text that overflows keeps the
/// row height and grows rightward to the table's edge; text that still overflows there grows
/// downward to its wrapped height. Never leaves the table rect, so no clamping is needed after.
/// `wrapped_lines` counts display lines at a given wrap width.
fn editor_size(
    cell: Bounds<Pixels>,
    table: Bounds<Pixels>,
    natural_w: Pixels,
    line_height: Pixels,
    mut wrapped_lines: impl FnMut(Pixels) -> usize,
) -> Size<Pixels> {
    let avail_w = table.right() - cell.origin.x;
    let avail_h = table.bottom() - cell.origin.y;
    let w = (natural_w + px(PAD_X)).clamp(cell.size.width.min(avail_w), avail_w);
    // Counted at the final width even when the text fits, because a value with hard newlines needs
    // more than one line at any width — assuming otherwise is what put a scrollbar in the box.
    let lines = wrapped_lines(w - px(PAD_X)).max(1);
    let h = (line_height * lines as f32 + px(PAD_Y))
        .max(cell.size.height)
        .min(avail_h);
    size(w, h)
}

/// Find the narrowest content width at which the textarea's own wrapper leaves every hard line
/// unwrapped. `minimum` is the source cell and `maximum` is the remaining table width. If even the
/// maximum wraps, return it and let `editor_size` grow downward instead.
fn unwrapped_width(
    minimum: Pixels,
    maximum: Pixels,
    hard_lines: usize,
    mut wrapped_lines: impl FnMut(Pixels) -> usize,
) -> Pixels {
    if wrapped_lines(minimum) <= hard_lines {
        return minimum;
    }
    if maximum <= minimum || wrapped_lines(maximum) > hard_lines {
        return maximum;
    }

    // At this point `low` wraps and `high` does not. Twelve bisections resolve a sub-pixel width
    // even across a full 4K table, and the returned upper bound is always one the editor accepted.
    let (mut low, mut high) = (minimum, maximum);
    for _ in 0..12 {
        let middle = low + (high - low) / 2.;
        if wrapped_lines(middle) > hard_lines {
            low = middle;
        } else {
            high = middle;
        }
    }
    high
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, Pixels, Size, point, px, size};

    fn cell_and_table() -> (Bounds<Pixels>, Bounds<Pixels>) {
        let cell = Bounds::new(point(px(300.), px(50.)), size(px(120.), px(32.)));
        let table = Bounds::new(point(px(0.), px(0.)), size(px(500.), px(150.)));
        (cell, table)
    }

    fn sized(natural_w: Pixels, lines: usize) -> Size<Pixels> {
        let (cell, table) = cell_and_table();
        super::editor_size(cell, table, natural_w, px(20.), |_| lines)
    }

    #[test]
    fn text_that_fits_gets_a_cell_sized_box() {
        assert_eq!(sized(px(40.), 1), cell_and_table().0.size);
    }

    #[test]
    fn overflowing_text_grows_right_at_the_row_height() {
        assert_eq!(sized(px(150.), 1), size(px(150. + super::PAD_X), px(32.)));
    }

    #[test]
    fn hard_newlines_grow_the_box_down_even_when_the_text_fits() {
        assert_eq!(sized(px(40.), 3), size(px(120.), px(60. + super::PAD_Y)));
    }

    #[test]
    fn text_past_the_table_edge_wraps_and_grows_down() {
        assert_eq!(sized(px(900.), 3), size(px(200.), px(60. + super::PAD_Y)));
    }

    #[test]
    fn height_never_passes_the_tables_bottom() {
        assert_eq!(sized(px(900.), 40).height, px(100.));
    }

    #[test]
    fn a_right_edge_cell_grows_down_without_crossing_the_table() {
        let cell = Bounds::new(point(px(380.), px(50.)), size(px(120.), px(32.)));
        let table = Bounds::new(point(px(0.), px(0.)), size(px(500.), px(150.)));
        let got = super::editor_size(cell, table, px(900.), px(20.), |_| 3);
        assert_eq!(got, size(px(120.), px(60. + super::PAD_Y)));
    }

    #[test]
    fn wrapper_threshold_keeps_the_final_word_on_the_first_line() {
        let threshold = px(137.);
        let got = super::unwrapped_width(px(80.), px(200.), 1, |width| match width < threshold {
            true => 2,
            false => 1,
        });
        assert!(got >= threshold);
        assert!(got - threshold < px(0.1));
    }

    /// The inset the whole geometry is written against: 8px each side plus the 1px border, plus
    /// the wrapper's own right margin; 4px top and bottom plus the border.
    #[test]
    fn the_text_inset_is_the_cells() {
        assert_eq!(super::PAD_X, 8. * 2. + 2. + 10.);
        assert_eq!(super::PAD_Y, 4. * 2. + 2.);
    }
}
