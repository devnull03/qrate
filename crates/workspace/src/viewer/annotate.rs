//! Region notes on the viewer: the row's regions drawn over its file, the card a hovered one opens,
//! and annotate mode, which marks new ones and writes their notes. Positions come from [`regions`],
//! measured against the content box the page is drawn in.

use std::time::Duration;

use diagnostics::{DATASET_MAIN, Diagnostics, Location, NoteId, NoteKind, Region};
use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    v_flex,
};
use settings::history::Origin;
use settings::project::RowId;

use crate::viewer::regions::{self, Grip, Rect};
use crate::viewer::{Scope, Viewer};

/// Annotate mode, the tool in hand, and whether regions are hidden. App-wide, since stepping to the
/// next row builds a new viewer and should not put the pen down.
#[derive(Default)]
pub(crate) struct Annotating {
    pub on: bool,
    pub tool: Tool,
    pub hidden: bool,
}

impl Global for Annotating {}

/// The note whose Notes card the pointer is over, lit in whichever viewer shows it.
#[derive(Default)]
pub(crate) struct Lit(pub Option<NoteId>);

impl Global for Lit {}

/// The region a Notes card was clicked for, which the viewer showing its row turns to and zooms on.
#[derive(Default)]
pub(crate) struct Reveal(pub Option<NoteId>);

impl Global for Reveal {}

/// Show region `id` in a viewer: the pop-out's when the click came from there, else the open
/// overlay if it shows the row, else a new one over the workspace.
pub(crate) fn reveal(id: NoteId, in_pop_out: bool, window: &mut Window, cx: &mut App) {
    let Some(row_id) = Diagnostics::note(id, cx).and_then(|note| note.location.row_id) else {
        return;
    };
    let shown = crate::viewer::viewer_in(Scope::Workspace, cx)
        .or_else(|| crate::viewer::viewer_in(Scope::Centre, cx))
        .is_some_and(|viewer| viewer.read(cx).row == Some(row_id));
    if !in_pop_out && !shown {
        let file = cx
            .try_global::<table::TableStateHandle>()
            .and_then(|handle| handle.0.upgrade())
            .and_then(|table| {
                let delegate = table.read(cx).delegate();
                crate::viewer::shown_file(delegate, delegate.row_of(row_id)?, cx)
            });
        let Some(file) = file else {
            return;
        };
        crate::viewer::open_viewer(file, Some(row_id), Scope::Workspace, window, cx);
    }
    cx.set_global(Reveal(Some(id)));
}

/// One of the row's region notes, numbered in the order they were filed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Mark {
    pub id: NoteId,
    pub number: usize,
    pub region: Region,
}

pub(crate) fn marks(row: RowId, cx: &App) -> Vec<Mark> {
    let mut notes: Vec<(NoteId, Region)> = Diagnostics::all(cx)
        .iter()
        .filter(|d| d.location.dataset == DATASET_MAIN && d.location.row_id == Some(row))
        .filter_map(|d| {
            let note = d.note.as_ref()?;
            Some((note.id, note.region?))
        })
        .collect();
    notes.sort_by_key(|(id, _)| *id);
    notes
        .into_iter()
        .enumerate()
        .map(|(ix, (id, region))| Mark {
            id,
            number: ix + 1,
            region,
        })
        .collect()
}

/// The tool annotate mode marks with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tool {
    Select,
    #[default]
    Rect,
    Pin,
}

/// A new region being dragged out, corner to corner, in fractions of the page as shown.
pub(super) struct Draw {
    pub(super) from: (f32, f32),
    pub(super) to: (f32, f32),
}

/// A selected region being moved or resized, by one of its [`Grip`]s.
pub(super) struct Grab {
    id: NoteId,
    grip: Grip,
    rect: Rect,
    from: (f32, f32),
    to: (f32, f32),
}

impl Grab {
    fn rect(&self) -> Rect {
        regions::drag(
            self.rect,
            self.grip,
            (self.to.0 - self.from.0, self.to.1 - self.from.1),
        )
    }
}

/// The region selected in the viewer, which the Notes panel brings into view.
#[derive(Default)]
pub(crate) struct Picked(pub Option<NoteId>);

impl Global for Picked {}

/// The card a new region's note is written in, beside the region until it is saved or dropped.
pub(super) struct Composer {
    region: Region,
    text: Entity<TextareaState>,
    kind: Option<NoteKind>,
    /// The name field, shown the first time someone with no name set saves.
    sign: Option<Entity<InputState>>,
}

fn state(cx: &App) -> (bool, Tool, bool) {
    cx.try_global::<Annotating>()
        .map_or((false, Tool::default(), false), |a| {
            (a.on, a.tool, a.hidden)
        })
}

/// Whether the composer still has to ask who is signing.
fn unasked(cx: &App) -> bool {
    settings::history::author(cx).is_none()
        && !cx.try_global::<settings::AppSettings>().is_some_and(|s| {
            s.values
                .get(settings::NOTE_AUTHOR_ASKED_KEY)
                .is_some_and(|v| v.bool())
        })
}

fn hidden(cx: &App) -> bool {
    state(cx).2
}

fn area(b: &Bounds<Pixels>) -> f32 {
    f32::from(b.size.width) * f32::from(b.size.height)
}

impl Viewer {
    pub(super) fn row_marks(&self, cx: &App) -> Vec<Mark> {
        self.row.map_or_else(Vec::new, |row| marks(row, cx))
    }

    /// Whether the page's shape is known, so regions land where they belong.
    fn ready(&self) -> bool {
        self.pixels.is_some()
    }

    fn aspect(&self) -> f32 {
        self.fit()
            .map_or(0.75, |(image, _)| image.width / image.height)
    }

    /// The page in content-box coordinates, which is where every region is drawn.
    pub(super) fn page_box(&self) -> Bounds<Pixels> {
        let area = Bounds {
            origin: Point::default(),
            size: self.frame.get().size,
        };
        regions::page(area, self.zoom, self.offset, self.aspect())
    }

    pub(super) fn local(&self, position: Point<Pixels>) -> Point<Pixels> {
        position - self.frame.get().origin
    }

    /// This page's marks and where each sits on screen, larger first so smaller ones paint over.
    fn placed(&self, marks: &[Mark]) -> Vec<(Mark, Rect, Bounds<Pixels>)> {
        let page = self.page_box();
        let mut placed: Vec<_> = marks
            .iter()
            .filter(|m| m.region.page as usize == self.page)
            .map(|m| {
                let rect = match self.grab.as_ref().filter(|grab| grab.id == m.id) {
                    Some(grab) => grab.rect(),
                    None => regions::shown(&m.region, self.shown),
                };
                (*m, rect, regions::on_screen(page, rect))
            })
            .collect();
        placed.sort_by(|a, b| area(&b.2).total_cmp(&area(&a.2)));
        placed
    }

    /// Track the region under the pointer, for its fill and its card.
    pub(super) fn hover(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let hit = match self.ready() && !hidden(cx) {
            true => {
                let placed = self.placed(&self.row_marks(cx));
                let boxes: Vec<_> = placed.iter().map(|(m, _, b)| (m.id, *b)).collect();
                regions::hit(&boxes, self.local(position))
            }
            false => None,
        };
        if hit != self.hovered {
            self.hovered = hit;
            cx.notify();
        }
    }

    /// Show `text` at the foot of the stage for a moment.
    pub(super) fn hint(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let gone = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(2500))
                .await;
            this.update(cx, |this, cx| {
                this.hint = None;
                cx.notify();
            })
            .ok();
        });
        self.hint = Some((text.into(), gone));
        cx.notify();
    }

    /// Whether this viewer can mark: annotate mode is on and the file belongs to a row.
    pub(super) fn annotating(&self, cx: &App) -> bool {
        self.row.is_some() && !self.missing && state(cx).0
    }

    pub(super) fn toggle_annotate(&mut self, cx: &mut Context<Self>) {
        if self.row.is_none() || self.missing {
            return;
        }
        let annotating = cx.default_global::<Annotating>();
        annotating.on = !annotating.on;
        self.draw = None;
        cx.notify();
    }

    pub(super) fn exit_annotate(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.annotating(cx) {
            return false;
        }
        cx.default_global::<Annotating>().on = false;
        self.composer = None;
        self.deleting = None;
        self.draw = None;
        self.grab = None;
        self.stepped = None;
        self.revealing = None;
        self.drag_from = None;
        self.select(None, cx);
        window.focus(&self.focus_handle, cx);
        true
    }

    /// Start a gesture under the pointer. `false` leaves the press to panning.
    pub(super) fn press(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        if self.composer.is_some() {
            return true;
        }
        let (on, tool, hidden) = state(cx);
        if !on || self.row.is_none() || !self.ready() {
            return false;
        }
        let at = regions::at(self.page_box(), self.local(position));
        if tool == Tool::Select {
            let local = self.local(position);
            let placed = match hidden {
                true => Vec::new(),
                false => self.placed(&self.row_marks(cx)),
            };
            let handle = self
                .selected
                .and_then(|id| placed.iter().find(|(m, ..)| m.id == id))
                .and_then(|(m, rect, b)| Some((m.id, Grip::under(*b, local)?, *rect)));
            let boxes: Vec<_> = placed.iter().map(|(m, _, b)| (m.id, *b)).collect();
            let grabbed = handle.or_else(|| {
                let id = regions::hit(&boxes, local)?;
                let (_, rect, _) = placed.iter().find(|(m, ..)| m.id == id)?;
                Some((id, Grip::Move, *rect))
            });
            self.select(grabbed.map(|(id, ..)| id), cx);
            self.grab = grabbed.map(|(id, grip, rect)| Grab {
                id,
                grip,
                rect,
                from: at,
                to: at,
            });
            return grabbed.is_some();
        }
        self.draw = Some(Draw { from: at, to: at });
        cx.notify();
        true
    }

    /// Follow the pointer with the gesture under way. `false` when there is none.
    pub(super) fn drag_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let at = regions::at(self.page_box(), self.local(position));
        if let Some(grab) = self.grab.as_mut() {
            grab.to = at;
            cx.notify();
            return true;
        }
        match self.draw.as_mut() {
            Some(draw) => {
                draw.to = at;
                cx.notify();
                true
            }
            None => false,
        }
    }

    /// End the gesture: a drag marks a box, a click a pin, and either opens the composer.
    pub(super) fn release(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(grab) = self.grab.take() {
            let moved = grab.rect();
            let of = self
                .row_marks(cx)
                .into_iter()
                .find(|m| m.id == grab.id)
                .and_then(|m| m.region.of);
            if moved != grab.rect {
                let region = regions::upright(moved, self.shown, self.page as u32, of);
                Diagnostics::move_note(grab.id, region, cx);
            }
            cx.notify();
            return;
        }
        let Some(Draw { from, to }) = self.draw.take() else {
            return;
        };
        let page = self.page_box();
        let far = (to.0 - from.0).abs() * f32::from(page.size.width) > 4.
            || (to.1 - from.1).abs() * f32::from(page.size.height) > 4.;
        let rect = match (state(cx).1, far) {
            (Tool::Rect, true) => regions::span(from, to),
            _ => [to.0, to.1, to.0, to.1],
        };
        let region = regions::upright(rect, self.shown, self.page as u32, self.header);
        let text = cx.new(|cx| TextareaState::new(window, cx).placeholder("Note"));
        cx.subscribe_in(&text, window, |this, _, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter {
                secondary: true, ..
            } = event
            {
                this.save(false, window, cx);
            }
        })
        .detach();
        text.update(cx, |text, cx| text.focus(window, cx));
        self.composer = Some(Composer {
            region,
            text,
            kind: None,
            sign: None,
        });
        cx.notify();
    }

    /// File the composed note. The first time someone with no name saves, ask once instead.
    pub(super) fn save(&mut self, skip: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = self.composer.as_mut() else {
            return;
        };
        let message = composer.text.read(cx).value();
        if message.trim().is_empty() {
            return;
        }
        if !skip && composer.sign.is_none() && unasked(cx) {
            let sign = cx.new(|cx| InputState::new(window, cx).placeholder("Your name"));
            cx.subscribe_in(&sign, window, |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.save(false, window, cx);
                }
            })
            .detach();
            sign.update(cx, |sign, cx| sign.focus(window, cx));
            composer.sign = Some(sign);
            cx.notify();
            return;
        }
        if let Some(sign) = composer.sign.as_ref() {
            let name = sign.read(cx).value().trim().to_string();
            if !skip && !name.is_empty() {
                settings::AppSettings::set_text(settings::NOTE_AUTHOR_KEY, name.into(), cx);
            }
            settings::AppSettings::set_bool(settings::NOTE_AUTHOR_ASKED_KEY, true, cx);
        }
        let Some(composer) = self.composer.take() else {
            return;
        };
        if let Some(location) = self.location(cx) {
            Diagnostics::file_note(
                location,
                Some(composer.region),
                composer.kind,
                message.trim().to_string().into(),
                Origin::Drawn,
                cx,
            );
        }
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    pub(super) fn select(&mut self, id: Option<NoteId>, cx: &mut Context<Self>) {
        self.selected = id;
        cx.set_global(Picked(id));
        cx.notify();
    }

    /// Ask before deleting the selected region.
    pub(super) fn ask_delete(&mut self, cx: &mut Context<Self>) {
        if self.annotating(cx) && state(cx).1 == Tool::Select {
            self.deleting = self.selected;
            cx.notify();
        }
    }

    pub(super) fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.deleting.take() {
            Diagnostics::edit_note(id, SharedString::default(), None, Origin::Drawn, cx);
            self.select(None, cx);
        }
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// Put away whatever annotate mode has in hand, innermost first: the composer, a confirmation,
    /// a drag, then the selection. `false` if there was nothing.
    pub(super) fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let had = self.composer.take().is_some()
            || self.deleting.take().is_some()
            || self.draw.take().is_some()
            || self.grab.take().is_some()
            || self.stepped.take().is_some();
        if !had && self.selected.is_some() {
            self.select(None, cx);
            return true;
        }
        if had {
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
        had
    }

    /// Where a new note on this row is filed: the row itself, at its current position.
    pub(super) fn location(&self, cx: &App) -> Option<Location> {
        let row_id = self.row?;
        let row = cx
            .try_global::<table::TableStateHandle>()
            .and_then(|handle| handle.0.upgrade())
            .and_then(|table| table.read(cx).delegate().row_of(row_id));
        Some(Location {
            dataset: DATASET_MAIN.into(),
            row,
            row_id: Some(row_id),
            column: None,
        })
    }

    /// What the foot of the stage says while annotate mode waits for a first region on this page.
    pub(super) fn standing_hint(&self, marks: &[Mark], cx: &App) -> Option<SharedString> {
        let (_, tool, hidden) = state(cx);
        let on_page: Vec<_> = marks
            .iter()
            .filter(|m| m.region.page as usize == self.page)
            .collect();
        if let Some(at) = self
            .stepped
            .and_then(|id| on_page.iter().position(|m| m.id == id))
            .filter(|_| !hidden)
        {
            return Some(
                format!(
                    "{} of {} on this page · [ previous · ] next",
                    at + 1,
                    on_page.len()
                )
                .into(),
            );
        }
        (self.annotating(cx) && on_page.is_empty() && !hidden && self.composer.is_none())
            .then_some(match tool {
                Tool::Rect => "Drag to mark a region, click to drop a pin",
                Tool::Pin => "Click to drop a pin",
                Tool::Select => return None,
            })
            .map(SharedString::from)
    }

    /// Turn to the region the Notes panel asked for, zooming once the page is laid out.
    pub(super) fn follow_reveal(&mut self, cx: &mut Context<Self>) {
        let Some(id) = cx.try_global::<Reveal>().and_then(|reveal| reveal.0) else {
            return;
        };
        let Some(mark) = self.row_marks(cx).into_iter().find(|m| m.id == id) else {
            return;
        };
        if mark.region.page as usize != self.page {
            self.show_page(mark.region.page as usize);
        }
        self.stepped = Some(id);
        self.revealing = Some(mark.region);
        cx.notify();
    }

    /// Zoom to the previous or next region on this page, stopping at either end as pages do.
    pub(super) fn step_mark(&mut self, delta: isize, cx: &mut Context<Self>) {
        let mut on_page = self.row_marks(cx);
        on_page.retain(|m| m.region.page as usize == self.page);
        let last = on_page.len() as isize - 1;
        if last < 0 || hidden(cx) {
            return;
        }
        let at = self
            .stepped
            .and_then(|id| on_page.iter().position(|m| m.id == id));
        let next = match at {
            Some(at) => (at as isize + delta).clamp(0, last),
            None if delta < 0 => last,
            None => 0,
        };
        let mark = on_page[next as usize];
        self.stepped = Some(mark.id);
        self.zoom_to(&mark.region);
        cx.notify();
    }

    /// Centre `region` and zoom until it spans half the frame, the way W fits the width.
    pub(super) fn zoom_to(&mut self, region: &Region) {
        let Some((image, fit)) = self.fit() else {
            return;
        };
        let [l, t, r, b] = regions::shown(region, self.shown);
        let (width, height) = (image.width * fit, image.height * fit);
        let frame = self.frame.get().size;
        let zoom = (f32::from(frame.width) * 0.5 / (width * (r - l)))
            .min(f32::from(frame.height) * 0.5 / (height * (b - t)))
            .clamp(1., 8.);
        self.zoom = zoom;
        self.offset = point(
            px(width * zoom * (0.5 - (l + r) / 2.)),
            px(height * zoom * (0.5 - (t + b) / 2.)),
        );
        self.clamp_pan();
    }

    pub(super) fn toggle_hidden(&mut self, cx: &mut Context<Self>) {
        let now = !hidden(cx);
        cx.default_global::<Annotating>().hidden = now;
        let count = self.row_marks(cx).len();
        if now && count > 0 {
            let noun = if count == 1 {
                "annotation"
            } else {
                "annotations"
            };
            self.hint(format!("{count} {noun} hidden. Press N to show them."), cx);
        }
        cx.notify();
    }
}

/// Annotate and show/hide, first in the viewer's toolbar.
pub(super) fn buttons(this: &Viewer, cx: &mut Context<Viewer>) -> AnyElement {
    let (on, _, hidden) = state(cx);
    h_flex()
        .gap_1()
        .child(
            Button::new("toggle-annotate")
                .icon(Icon::empty().path("icons/square-pen.svg"))
                .ghost()
                .small()
                .selected(on && this.row.is_some() && !this.missing)
                .disabled(this.row.is_none() || this.missing)
                .tooltip("Annotate (A)")
                .on_click(cx.listener(|this, _, _, cx| this.toggle_annotate(cx))),
        )
        .child(
            Button::new("toggle-annotations")
                .icon(match hidden {
                    true => IconName::EyeOff,
                    false => IconName::Eye,
                })
                .ghost()
                .small()
                .disabled(this.row.is_none())
                .tooltip(match hidden {
                    true => "Show annotations (N)",
                    false => "Hide annotations (N)",
                })
                .on_click(cx.listener(|this, _, _, cx| this.toggle_hidden(cx))),
        )
        .into_any_element()
}

/// The tool pill on the stage's left edge, while annotating.
pub(super) fn tools(this: &Viewer, cx: &mut Context<Viewer>) -> Option<AnyElement> {
    let (_, current, _) = state(cx);
    this.annotating(cx).then(|| {
        v_flex()
            .gap(px(2.))
            .p_1()
            .rounded(cx.theme().radius)
            .bg(cx.theme().background.opacity(0.8))
            .occlude()
            .children(
                [
                    (Tool::Select, "icons/mouse-pointer-2.svg", "Select"),
                    (Tool::Rect, "icons/rectangle.svg", "Rectangle"),
                    (Tool::Pin, "icons/map-pin.svg", "Pin"),
                ]
                .map(|(tool, icon, tip)| {
                    Button::new(tip)
                        .icon(Icon::empty().path(icon))
                        .ghost()
                        .small()
                        .selected(tool == current)
                        .tooltip(tip)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.default_global::<Annotating>().tool = tool;
                            this.draw = None;
                            cx.notify();
                        }))
                }),
            )
            .into_any_element()
    })
}

/// Everything drawn over the page: each region, its badge, and the card of the one hovered.
/// Laid over the content box, in its coordinates.
pub(super) fn layer(this: &Viewer, marks: &[Mark], cx: &mut Context<Viewer>) -> AnyElement {
    let frame = this.frame.get().size;
    let shown = !hidden(cx) && this.ready() && frame.width > px(0.);
    let placed = match shown {
        true => this.placed(marks),
        false => Vec::new(),
    };
    let lit = cx.try_global::<Lit>().and_then(|lit| lit.0);
    let focus = this.hovered.or(this.stepped).or(lit).or(this.selected);
    let focus_box = focus.and_then(|id| placed.iter().find(|(m, ..)| m.id == id).map(|p| p.2));
    let crowded = focus_box.is_some_and(|f| {
        placed
            .iter()
            .any(|(m, _, b)| Some(m.id) != focus && f.intersects(b))
    });
    let dim = |id: NoteId| crowded && Some(id) != focus;
    let (white, dark) = (white(), rgb(0x161616));

    let mut boxes: Vec<_> = placed
        .iter()
        .filter(|(_, rect, _)| rect[0] != rect[2] || rect[1] != rect[3])
        .map(|(m, _, b)| (m.id, m.number, *b))
        .collect();
    boxes.sort_by_key(|(_, number, _)| *number);
    let badges = regions::badges(&boxes);

    let card = focus
        .filter(|id| Some(*id) == this.hovered.or(this.stepped))
        .and_then(|id| {
            Some((
                Diagnostics::note(id, cx)?,
                placed.iter().find(|p| p.0.id == id)?,
            ))
        })
        .map(|(note, (_, _, b))| {
            let pin = b.size.width == px(0.);
            let below = match pin {
                true => b.origin.y + px(14.),
                false => b.origin.y + b.size.height + px(8.),
            };
            let top = match below + px(110.) > frame.height - px(12.) {
                true => b.origin.y - px(118.),
                false => below,
            };
            let left = b
                .origin
                .x
                .min(frame.width - px(diagnostics::note_card::WIDTH + 12.))
                .max(px(12.));
            div()
                .absolute()
                .left(left)
                .top(top)
                .child(diagnostics::note_card::note_card(
                    note,
                    None,
                    Vec::new(),
                    cx,
                ))
        });

    let page = this.page_box();
    let tool = state(cx).1;
    let theme = cx.theme().clone();
    let drawing = this
        .draw
        .as_ref()
        .filter(|_| tool == Tool::Rect)
        .map(|draw| regions::on_screen(page, regions::span(draw.from, draw.to)))
        .filter(|b| b.size.width > px(0.) || b.size.height > px(0.));
    let composing = this.composer.as_ref().map(|composer| {
        let rect = regions::shown(&composer.region, this.shown);
        (rect, regions::on_screen(page, rect))
    });
    let composer =
        this.composer
            .as_ref()
            .zip(composing)
            .map(|(composer, (rect, b))| {
                let height = if composer.sign.is_some() { 262. } else { 180. };
                let right = b.origin.x + b.size.width + px(12.);
                let left = match right + px(280.) > frame.width - px(12.) {
                    true => b.origin.x - px(292.),
                    false => right,
                }
                .max(px(12.));
                let top = b.origin.y.min(frame.height - px(height + 12.)).max(px(12.));
                let row = this.location(cx).and_then(|l| l.row);
                let strip = [
                    Some(row.map_or("Note".to_string(), |row| format!("Note on row {}", row + 1))),
                    this.paged().then(|| format!("page {}", this.page + 1)),
                    Some(match rect[0] == rect[2] && rect[1] == rect[3] {
                        true => "pin".to_string(),
                        false => "region".to_string(),
                    }),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
                let save_key = match cfg!(target_os = "macos") {
                    true => "⌘Enter to save",
                    false => "Ctrl+Enter to save",
                };
                div()
                    .absolute()
                    .left(left)
                    .top(top)
                    .w(px(280.))
                    .occlude()
                    .on_action(cx.listener(
                        |this, _: &gpui_component::input::Escape, window, cx| {
                            if !this.exit_annotate(window, cx) {
                                this.cancel(window, cx);
                            }
                        },
                    ))
                    .child(
                        v_flex()
                            .bg(theme.popover)
                            .text_color(theme.popover_foreground)
                            .border_1()
                            .border_color(theme.primary)
                            .rounded(px(6.))
                            .shadow_md()
                            .overflow_hidden()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .child(
                                div()
                                    .px_2()
                                    .py(px(3.))
                                    .bg(theme.muted)
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(strip),
                            )
                            .child(
                                div().px_1().py_1().child(
                                    Textarea::new(&composer.text).appearance(false).h(px(54.)),
                                ),
                            )
                            .child(h_flex().gap_1().px_2().pb_2().children(NoteKind::ALL.map(
                                |kind| {
                                    let on = composer.kind == Some(kind);
                                    div()
                                        .id(kind.key())
                                        .h(px(20.))
                                        .px_2()
                                        .flex()
                                        .items_center()
                                        .rounded(px(10.))
                                        .border_1()
                                        .text_xs()
                                        .cursor_pointer()
                                        .map(|chip| match on {
                                            true => chip
                                                .bg(theme.primary.opacity(0.14))
                                                .text_color(theme.primary)
                                                .border_color(theme.primary.opacity(0.5)),
                                            false => chip
                                                .text_color(theme.muted_foreground)
                                                .border_color(theme.border),
                                        })
                                        .child(kind.label())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(composer) = this.composer.as_mut() {
                                                composer.kind =
                                                    (composer.kind != Some(kind)).then_some(kind);
                                            }
                                            cx.notify();
                                        }))
                                },
                            )))
                            .children(composer.sign.as_ref().map(|sign| {
                                v_flex()
                                    .gap_1p5()
                                    .p_2()
                                    .border_t_1()
                                    .border_color(theme.border)
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .child("Sign your notes as…"),
                                    )
                                    .child(Input::new(sign).small())
                                    .child(
                                        div()
                                            .id("skip-signing")
                                            .text_xs()
                                            .text_color(theme.link)
                                            .cursor_pointer()
                                            .child("Skip, leave unsigned")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.save(true, window, cx)
                                            })),
                                    )
                            }))
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1p5()
                                    .px_2()
                                    .py_1p5()
                                    .border_t_1()
                                    .border_color(theme.border)
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .whitespace_nowrap()
                                            .child(save_key),
                                    )
                                    .child(
                                        Button::new("composer-cancel")
                                            .label("Cancel")
                                            .small()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.cancel(window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("composer-save")
                                            .label("Save")
                                            .primary()
                                            .small()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.save(false, window, cx)
                                            })),
                                    ),
                            ),
                    )
            });

    let deleting = this
        .deleting
        .and_then(|id| {
            Some((
                Diagnostics::note(id, cx)?,
                placed.iter().find(|p| p.0.id == id)?,
            ))
        })
        .map(|(note, (mark, _, b))| {
            let right = b.origin.x + b.size.width + px(12.);
            let left = match right + px(252.) > frame.width - px(12.) {
                true => b.origin.x - px(264.),
                false => right,
            }
            .max(px(12.));
            let top = b.origin.y.min(frame.height - px(112.)).max(px(12.));
            let author = note
                .note
                .as_ref()
                .and_then(|n| n.filed.as_ref()?.author.clone());
            let question = match author {
                Some(author) => format!("Delete annotation #{} by {author}?", mark.number),
                None => format!("Delete annotation #{}?", mark.number),
            };
            div()
                .absolute()
                .left(left)
                .top(top)
                .w(px(252.))
                .occlude()
                .child(
                    v_flex()
                        .gap_1p5()
                        .px_2p5()
                        .pt_2p5()
                        .pb_2()
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .border_1()
                        .border_color(theme.border)
                        .rounded(px(6.))
                        .shadow_md()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .child(question)
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("History keeps a copy you can restore."),
                        )
                        .child(
                            h_flex()
                                .justify_end()
                                .gap_1p5()
                                .mt_0p5()
                                .child(
                                    Button::new("delete-cancel")
                                        .label("Cancel")
                                        .small()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.cancel(window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("delete-confirm")
                                        .label("Delete")
                                        .danger()
                                        .small()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.delete(window, cx)
                                        })),
                                ),
                        ),
                )
        });

    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .children(drawing.map(|b| {
            let shade = black().opacity(0.3);
            let (right, bottom) = (b.origin.x + b.size.width, b.origin.y + b.size.height);
            let page_right = page.origin.x + page.size.width;
            let page_bottom = page.origin.y + page.size.height;
            let strip = |left: Pixels, top: Pixels, width: Pixels, height: Pixels| {
                div()
                    .absolute()
                    .left(left)
                    .top(top)
                    .w(width.max(px(0.)))
                    .h(height.max(px(0.)))
                    .bg(shade)
            };
            div()
                .absolute()
                .size_full()
                .child(strip(
                    page.origin.x,
                    page.origin.y,
                    page.size.width,
                    b.origin.y - page.origin.y,
                ))
                .child(strip(
                    page.origin.x,
                    bottom,
                    page.size.width,
                    page_bottom - bottom,
                ))
                .child(strip(
                    page.origin.x,
                    b.origin.y,
                    b.origin.x - page.origin.x,
                    b.size.height,
                ))
                .child(strip(right, b.origin.y, page_right - right, b.size.height))
                .child(
                    div()
                        .absolute()
                        .left(b.origin.x)
                        .top(b.origin.y)
                        .w(b.size.width)
                        .h(b.size.height)
                        .border(px(1.5))
                        .border_dashed()
                        .border_color(white),
                )
        }))
        .children(composing.map(|(rect, b)| {
            match rect[0] == rect[2] && rect[1] == rect[3] {
                true => div()
                    .absolute()
                    .left(b.origin.x - px(9.))
                    .top(b.origin.y - px(9.))
                    .size(px(18.))
                    .rounded_full()
                    .bg(theme.primary)
                    .border(px(1.5))
                    .border_color(white),
                false => div()
                    .absolute()
                    .left(b.origin.x)
                    .top(b.origin.y)
                    .w(b.size.width)
                    .h(b.size.height)
                    .border(px(1.5))
                    .border_color(black().opacity(0.7))
                    .child(
                        div()
                            .size_full()
                            .border(px(1.5))
                            .border_color(white.opacity(0.92)),
                    ),
            }
        }))
        .children(placed.iter().map(|(m, rect, b)| {
            let pin = rect[0] == rect[2] && rect[1] == rect[3];
            let opacity = match (dim(m.id), this.missing) {
                (true, _) => 0.4,
                (false, true) => 0.5,
                (false, false) => 1.,
            };
            match pin {
                true => div()
                    .absolute()
                    .left(b.origin.x - px(9.))
                    .top(b.origin.y - px(9.))
                    .size(px(18.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(white)
                    .bg(match this.selected == Some(m.id) {
                        true => theme.primary,
                        false => dark.into(),
                    })
                    .border(px(1.5))
                    .border_color(white)
                    .shadow_sm()
                    .opacity(opacity)
                    .child(m.number.to_string()),
                false => div()
                    .absolute()
                    .left(b.origin.x)
                    .top(b.origin.y)
                    .w(b.size.width)
                    .h(b.size.height)
                    .rounded(px(1.))
                    .border(px(1.5))
                    .border_color(match this.missing {
                        true => transparent_black(),
                        false => black().opacity(0.7),
                    })
                    .opacity(opacity)
                    .child(
                        div()
                            .size_full()
                            .border(px(1.5))
                            .when(this.missing, |inner| inner.border_dashed())
                            .border_color(white.opacity(0.92))
                            .when(focus == Some(m.id), |inner| inner.bg(white.opacity(0.18))),
                    )
                    .when(this.selected == Some(m.id), |region| {
                        region.children(Grip::HANDLES.map(|grip| {
                            let (u, v) = grip.anchor();
                            div()
                                .absolute()
                                .left(b.size.width * u - px(regions::HANDLE / 2. + 1.5))
                                .top(b.size.height * v - px(regions::HANDLE / 2. + 1.5))
                                .size(px(regions::HANDLE))
                                .bg(white)
                                .border(px(1.5))
                                .border_color(rgb(0x111111))
                        }))
                    }),
            }
        }))
        .children(badges.into_iter().map(|badge| {
            let faded = badge.members.iter().all(|id| dim(*id));
            let picked = badge.members.iter().any(|id| this.selected == Some(*id));
            div()
                .absolute()
                .left(badge.at.x - px(1.5))
                .top(badge.at.y - px(1.5))
                .h(px(regions::BADGE))
                .min_w(px(regions::BADGE))
                .px(px(4.))
                .rounded_br(px(4.))
                .flex()
                .items_center()
                .justify_center()
                .whitespace_nowrap()
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(white)
                .bg(match picked {
                    true => theme.primary,
                    false => black().opacity(0.9),
                })
                .opacity(if faded { 0.4 } else { 1. })
                .child(badge.label)
        }))
        .children(card.filter(|_| {
            this.draw.is_none()
                && this.grab.is_none()
                && this.composer.is_none()
                && this.deleting.is_none()
        }))
        .children(composer)
        .children(deleting)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use diagnostics::{DATASET_MAIN, Diagnostics, Location, Region};
    use gpui::TestAppContext;
    use settings::history::Origin;

    use crate::viewer::Scope;
    use crate::viewer::annotate::{Annotating, Tool, marks, unasked};

    /// A row's regions are numbered in the order they were filed, and nothing else on the row —
    /// its plain notes, another row's regions — takes a number.
    #[gpui::test]
    fn a_rows_regions_are_numbered_in_filing_order(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let row = |id| Location {
                dataset: DATASET_MAIN.into(),
                row: Some(id as usize - 1),
                row_id: Some(id),
                column: None,
            };
            let region = |x| Region {
                page: 0,
                x,
                y: 0,
                w: 1000,
                h: 1000,
                of: None,
            };
            let file = |at, region, text: &str, cx: &mut gpui::App| {
                Diagnostics::file_note(row(at), region, None, text.into(), Origin::Drawn, cx)
            };
            let first = file(1, Some(region(100)), "stamp", cx);
            file(1, None, "faded", cx);
            file(2, Some(region(200)), "elsewhere", cx);
            let second = file(1, Some(region(300)), "signature", cx);

            let numbered: Vec<_> = marks(1, cx)
                .iter()
                .map(|m| (Some(m.id), m.number))
                .collect();
            assert_eq!(numbered, vec![(first, 1), (second, 2)]);
        });
    }

    /// A box dragged out in annotate mode opens the composer; the first save with no name set asks
    /// for one instead, and skipping files the note unsigned and never asks again.
    #[gpui::test]
    fn a_dragged_box_is_filed_after_asking_once_for_a_name(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            gpui_component::init(cx);
            cx.set_global(settings::AppSettings::default());
            cx.set_global(settings::SettingsPersistence::default());
            cx.default_global::<Annotating>().on = true;
            let viewer = crate::viewer::build(
                "/nonexistent/qrate-annotate.png".into(),
                Some(7),
                Scope::Workspace,
                window,
                cx,
            );
            viewer.update(cx, |viewer, cx| {
                viewer.frame.set(gpui::Bounds {
                    origin: gpui::point(gpui::px(0.), gpui::px(0.)),
                    size: gpui::size(gpui::px(400.), gpui::px(300.)),
                });
                viewer.pixels = Some((400, 300));
                assert!(viewer.press(gpui::point(gpui::px(100.), gpui::px(75.)), cx));
                viewer.drag_to(gpui::point(gpui::px(200.), gpui::px(150.)), cx);
                viewer.release(window, cx);
                let text = viewer
                    .composer
                    .as_ref()
                    .expect("the composer opens")
                    .text
                    .clone();
                text.update(cx, |text, cx| text.set_value("customs stamp", window, cx));

                viewer.save(false, window, cx);
                assert!(
                    viewer.composer.as_ref().is_some_and(|c| c.sign.is_some()),
                    "the first save asks for a name"
                );
                assert!(marks(7, cx).is_empty(), "and files nothing yet");

                viewer.save(true, window, cx);
                let filed = marks(7, cx);
                assert_eq!(filed.len(), 1);
                let region = filed[0].region;
                assert_eq!(
                    (region.x, region.y, region.w, region.h),
                    (2500, 2500, 2500, 2500)
                );
                assert!(!unasked(cx), "skipping counts as asked");
            });
        });
    }

    /// The Select tool picks a region up by its middle and moves it; Delete asks first, and only
    /// the confirmation removes it.
    #[gpui::test]
    fn a_selected_region_moves_and_deletes_on_confirmation(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            gpui_component::init(cx);
            let state = cx.default_global::<Annotating>();
            state.on = true;
            state.tool = Tool::Select;
            let location = Location {
                dataset: DATASET_MAIN.into(),
                row: Some(0),
                row_id: Some(3),
                column: None,
            };
            let stamp = Region {
                page: 0,
                x: 2500,
                y: 2500,
                w: 2500,
                h: 2500,
                of: None,
            };
            let id = Diagnostics::file_note(
                location,
                Some(stamp),
                None,
                "stamp".into(),
                Origin::Drawn,
                cx,
            )
            .unwrap();
            let viewer = crate::viewer::build(
                {
                    let path = std::env::temp_dir().join("qrate-select.png");
                    std::fs::write(&path, b"").unwrap();
                    path
                },
                Some(3),
                Scope::Workspace,
                window,
                cx,
            );
            viewer.update(cx, |viewer, cx| {
                viewer.frame.set(gpui::Bounds {
                    origin: gpui::point(gpui::px(0.), gpui::px(0.)),
                    size: gpui::size(gpui::px(400.), gpui::px(300.)),
                });
                viewer.pixels = Some((400, 300));
                assert!(viewer.press(gpui::point(gpui::px(150.), gpui::px(110.)), cx));
                assert_eq!(viewer.selected, Some(id));
                viewer.drag_to(gpui::point(gpui::px(190.), gpui::px(110.)), cx);
                viewer.release(window, cx);
                let moved = marks(3, cx)[0].region;
                assert_eq!((moved.x, moved.y, moved.w), (3500, 2500, 2500));

                viewer.ask_delete(cx);
                assert_eq!(viewer.deleting, Some(id));
                assert_eq!(marks(3, cx).len(), 1, "asking deletes nothing");
                viewer.delete(window, cx);
                assert!(marks(3, cx).is_empty());
                assert_eq!(viewer.selected, None);
            });
        });
    }

    /// `]` zooms to the first region on the page and centres it; `[` from there stays put at the
    /// first, as a page turn stops at the first page.
    #[gpui::test]
    fn stepping_zooms_to_each_region_on_the_page(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            gpui_component::init(cx);
            let location = Location {
                dataset: DATASET_MAIN.into(),
                row: Some(0),
                row_id: Some(4),
                column: None,
            };
            let corner = Region {
                page: 0,
                x: 0,
                y: 0,
                w: 2500,
                h: 2500,
                of: None,
            };
            let first = Diagnostics::file_note(
                location.clone(),
                Some(corner),
                None,
                "a".into(),
                Origin::Drawn,
                cx,
            );
            Diagnostics::file_note(
                location,
                Some(Region { x: 5000, ..corner }),
                None,
                "b".into(),
                Origin::Drawn,
                cx,
            );
            let viewer = crate::viewer::build(
                {
                    let path = std::env::temp_dir().join("qrate-step.png");
                    std::fs::write(&path, b"").unwrap();
                    path
                },
                Some(4),
                Scope::Workspace,
                window,
                cx,
            );
            viewer.update(cx, |viewer, cx| {
                viewer.frame.set(gpui::Bounds {
                    origin: gpui::point(gpui::px(0.), gpui::px(0.)),
                    size: gpui::size(gpui::px(400.), gpui::px(300.)),
                });
                viewer.pixels = Some((400, 300));
                viewer.step_mark(1, cx);
                assert_eq!(viewer.stepped, first);
                assert_eq!(viewer.zoom, 2.);
                assert_eq!(
                    (viewer.offset.x, viewer.offset.y),
                    (gpui::px(200.), gpui::px(150.))
                );
                viewer.step_mark(-1, cx);
                assert_eq!(viewer.stepped, first);
                viewer.step_mark(1, cx);
                assert_ne!(viewer.stepped, first);
                assert!(
                    viewer
                        .standing_hint(&marks(4, cx), cx)
                        .is_some_and(|h| h.starts_with("2 of 2"))
                );
            });
        });
    }

    /// A row whose file is gone still shows its regions, on a page of the size they were marked
    /// on, but cannot take new ones.
    #[gpui::test]
    fn a_missing_file_keeps_its_regions_and_turns_annotating_off(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        cx.update(|window, cx| {
            gpui_component::init(cx);
            cx.default_global::<Annotating>().on = true;
            let location = Location {
                dataset: DATASET_MAIN.into(),
                row: Some(0),
                row_id: Some(5),
                column: None,
            };
            let region = Region {
                page: 0,
                x: 0,
                y: 0,
                w: 2500,
                h: 2500,
                of: Some((4000, 3000)),
            };
            Diagnostics::file_note(location, Some(region), None, "a".into(), Origin::Drawn, cx);
            let viewer = crate::viewer::build(
                "/nonexistent/qrate-gone.tif".into(),
                Some(5),
                Scope::Workspace,
                window,
                cx,
            );
            viewer.update(cx, |viewer, cx| {
                assert!(viewer.missing);
                assert_eq!(viewer.pixels, Some((4000, 3000)));
                assert!(!viewer.annotating(cx));
                assert!(!viewer.has_controls());
            });
        });
    }

    /// Clicking a Notes card for a row the overlay already shows turns it to the region's page and
    /// zooms there once the page has a size.
    #[gpui::test]
    fn revealing_a_region_turns_to_its_page_and_zooms(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let (id, region) = cx.update(|window, cx| {
            gpui_component::init(cx);
            let location = Location {
                dataset: DATASET_MAIN.into(),
                row: Some(0),
                row_id: Some(6),
                column: None,
            };
            let region = Region {
                page: 2,
                x: 2500,
                y: 2500,
                w: 2500,
                h: 2500,
                of: None,
            };
            let id =
                Diagnostics::file_note(location, Some(region), None, "a".into(), Origin::Drawn, cx)
                    .unwrap();
            let path = std::env::temp_dir().join("qrate-reveal.png");
            std::fs::write(&path, b"").unwrap();
            crate::viewer::open_viewer(path, Some(6), Scope::Workspace, window, cx);
            crate::viewer::reveal(id, false, window, cx);
            (id, region)
        });
        cx.update(|window, cx| {
            let viewer = crate::viewer::viewer_in(Scope::Workspace, cx).unwrap();
            viewer.update(cx, |viewer, _| {
                assert_eq!(viewer.page, 2);
                assert_eq!(viewer.stepped, Some(id));
                assert_eq!(viewer.revealing, Some(region));
                viewer.frame.set(gpui::Bounds {
                    origin: gpui::point(gpui::px(0.), gpui::px(0.)),
                    size: gpui::size(gpui::px(400.), gpui::px(300.)),
                });
                viewer.pixels = Some((400, 300));
                viewer.zoom_to(&region);
                assert_eq!(viewer.zoom, 2.);
            });
            crate::viewer::close_viewer(window, cx);
        });
    }
}
