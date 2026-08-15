use std::collections::VecDeque;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::dock::{DockPlacement, Panel, PanelControl, PanelEvent};
use gpui_component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{ActiveTheme as _, IconName, Sizable as _, h_flex, v_flex};

use crate::BottomDockCrop;
use crate::panel_registry::PanelMeta;

/// Where Agent starts out and what it puts in the status bar.
pub static AGENT_META: PanelMeta = PanelMeta {
    name: "AgentPanel",
    icon: IconName::Star,
    label: "Agent",
    default_placement: DockPlacement::Right,
    badge: false,
};

/// The two halves of the Agent panel: what an agent already did, and where one is run.
///
/// They are tabs rather than two panels because they are one subject — the terminal is where the
/// agent is started and the log is what it then did, and a reader flips between them constantly.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum View {
    Log,
    Terminal,
}

impl View {
    /// Tab order, left to right.
    const ALL: [View; 2] = [View::Log, View::Terminal];

    /// `Menu` for the log on the same reasoning the Table view uses it — rows of things.
    fn icon(self) -> IconName {
        match self {
            View::Log => IconName::Menu,
            View::Terminal => IconName::SquareTerminal,
        }
    }

    fn label(self) -> &'static str {
        match self {
            View::Log => "Log",
            View::Terminal => "Terminal",
        }
    }
}

/// What kind of entry this is, which is also how loudly it reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    /// The bridge answered.
    Answered,
    /// The bridge refused before answering — a wrong token, an unparseable request, a rejected
    /// one. The reason this list is worth reading.
    Refused,
    /// A connect or disconnect. The transport infers both: the protocol has no session, so
    /// "connected" is the first authenticated call from a name and "disconnected" is silence.
    Lifecycle,
}

/// What the agent bridge reports about one thing that happened on it.
///
/// Plain strings rather than the contract's types: this crate hosts panels and has no business
/// knowing `ai::agent`, and the transport is already matching on the request to answer it.
pub struct AgentCall {
    /// Who said they were calling — the `X-Agent` header. A label the caller chose, never proof:
    /// anything holding the token can claim any name. See the README.
    pub agent: SharedString,
    /// The wire method, or `connected` / `disconnected` for a [`Entry::Lifecycle`] entry.
    pub label: SharedString,
    /// What was asked for, phrased short — a row count, the query, how many findings.
    pub detail: SharedString,
    /// What came back, or why it was refused.
    pub outcome: SharedString,
    pub entry: Entry,
    pub took: Duration,
}

/// One entry as the panel holds it: what happened, and when in the session.
struct Logged {
    at: Duration,
    call: AgentCall,
}

impl Logged {
    /// One entry as a line of text, which is what the copy buttons hand over. Tab-separated so it
    /// pastes into a sheet as columns and into a chat as a readable line.
    fn line(&self) -> String {
        let call = &self.call;
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            elapsed(self.at),
            call.agent,
            call.label,
            call.detail,
            call.outcome,
            took(call)
        )
    }
}

/// Everything that happened on the agent bridge this run, oldest first.
///
/// Chronological because this reads as a log: a call and the answer that followed it belong in the
/// order they happened, and the panel follows the tail rather than making the reader chase it.
///
/// In memory and capped. A session's agent traffic is a debugging aid — persisting it would put a
/// log of somebody else's questions inside the archivist's project file.
pub struct AgentHistory {
    /// Start of the first entry, which every `at` is measured from.
    // ponytail: elapsed rather than a wall clock — nothing in the tree formats local time, and
    // ordering plus elapsed is what reading a live list needs. Give it a real timestamp if it ever
    // has to line up with qrate.log.
    started: Instant,
    calls: VecDeque<Logged>,
}

impl Global for AgentHistory {}

impl Default for AgentHistory {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            calls: VecDeque::new(),
        }
    }
}

/// Oldest entries fall off the end. Long enough to hold a whole review, short enough that an agent
/// in a loop cannot grow the list without bound.
const HISTORY_LIMIT: usize = 200;

/// File one thing that happened. Called by the transport, which is the only thing that sees them.
pub fn record(call: AgentCall, cx: &mut App) {
    let history = cx.default_global::<AgentHistory>();
    let at = history.started.elapsed();
    history.calls.push_back(Logged { at, call });
    while history.calls.len() > HISTORY_LIMIT {
        history.calls.pop_front();
    }
}

/// `+2:07` — minutes and seconds since the first entry of the session.
fn elapsed(at: Duration) -> SharedString {
    format!("+{}:{:02}", at.as_secs() / 60, at.as_secs() % 60).into()
}

/// Sub-millisecond calls are the common case for a read off the live delegate, and `0ms` reads as
/// a measurement that failed rather than one that was fast. A lifecycle entry timed nothing.
fn took(call: &AgentCall) -> SharedString {
    match (call.entry, call.took.as_millis()) {
        (Entry::Lifecycle, _) => SharedString::default(),
        (_, 0) => "<1ms".into(),
        (_, ms) => format!("{ms}ms").into(),
    }
}

/// Right dock: what the external agent has asked qrate for, and what it got back.
pub struct AgentPanel {
    focus_handle: FocusHandle,
    view: View,
    scroll: ScrollHandle,
    /// Refreshes on any entry. One `observe_global` and no re-binding — the history is plain data
    /// that is never rebuilt.
    _sub: Subscription,
    /// Re-renders when the bottom dock opens or closes, so the padding tracks it.
    _crop_sub: Subscription,
}

impl AgentPanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let scroll = ScrollHandle::new();
        Self {
            focus_handle: cx.focus_handle(),
            view: View::Log,
            scroll: scroll.clone(),
            // ponytail: follows the tail unconditionally. Only worth remembering whether the
            // reader had scrolled away if watching a live agent while reading back proves annoying.
            _sub: cx.observe_global::<AgentHistory>(move |_, cx| {
                scroll.scroll_to_bottom();
                cx.notify();
            }),
            _crop_sub: cx.observe_global::<BottomDockCrop>(|_this: &mut Self, cx| cx.notify()),
        }
    }
}

impl Focusable for AgentPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PanelEvent> for AgentPanel {}

impl Panel for AgentPanel {
    fn panel_name(&self) -> &'static str {
        "AgentPanel"
    }

    fn title(&mut self, _w: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        SharedString::from("Agent")
    }

    /// The view switcher, as the same segmented control the centre dock switches Table/Gallery
    /// with — one switcher idiom in the app, not two.
    ///
    /// In the suffix rather than the title so it sits to the *right* of the panel name:
    /// `TabPanel` lays the row out as `[title (flex_1)] [title_suffix] [⋯]`. `small` because that
    /// row is 30px and a default segmented tab is 32px, which the title cell would clip.
    fn title_suffix(
        &mut self,
        _w: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        Some(
            TabBar::new("agent-view")
                .segmented()
                .small()
                .selected_index(
                    View::ALL
                        .iter()
                        .position(|view| *view == self.view)
                        .unwrap_or(0),
                )
                .on_click(cx.listener(|this, ix: &usize, _w, cx| {
                    this.view = View::ALL[*ix];
                    cx.notify();
                }))
                .children(
                    View::ALL
                        .into_iter()
                        .map(|view| Tab::new().icon(view.icon()).label(view.label())),
                ),
        )
    }

    // The library always renders the ⋯ menu button; these just empty it of Close + Zoom.
    fn closable(&self, _cx: &App) -> bool {
        false
    }

    fn zoomable(&self, _cx: &App) -> Option<PanelControl> {
        None
    }
}

impl Render for AgentPanel {
    fn render(&mut self, _w: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, danger, info, border, hover_bg) = (
            theme.muted_foreground,
            theme.danger,
            theme.info,
            theme.border,
            theme.secondary_hover,
        );
        let crop = cx.try_global::<BottomDockCrop>().map_or(px(0.), |c| c.0);
        let history = cx.try_global::<AgentHistory>();
        let empty = history.is_none_or(|h| h.calls.is_empty());
        let all = history.map_or_else(String::new, |h| {
            h.calls
                .iter()
                .map(Logged::line)
                .collect::<Vec<_>>()
                .join("\n")
        });

        v_flex()
            .size_full()
            .when(self.view == View::Terminal, |panel| {
                panel.child(
                    div()
                        .p_3()
                        .text_sm()
                        .text_color(muted)
                        .child("Nothing runs here yet. This is where qrate will start an agent and hand you its terminal."),
                )
            })
            .when(self.view == View::Log && empty, |panel| {
                panel.child(
                    div()
                        .p_3()
                        .text_sm()
                        .text_color(muted)
                        // Names where the switch is: an agent that never connected and a bridge
                        // that was switched off look identical from here otherwise.
                        .child("No agent activity yet. Agents may read this app unless you switch that off under Settings ▸ Agent."),
                )
            })
            .children(history.filter(|_| !empty && self.view == View::Log).map(|history| {
                v_flex()
                    .id("agent-log")
                    .size_full()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .vertical_scrollbar(&self.scroll)
                    // The rows stop short of the scrollbar rather than running under it, and the
                    // last entry clears the closed bottom dock's strip (29px closed, 0 open) —
                    // otherwise the newest line, which is the one autoscroll just brought into
                    // view, is the one hidden behind it.
                    .pr_2()
                    .pb(px(8.) + crop)
                    .children(
                        history.calls.iter().enumerate().map(|(ix, logged)| {
                            let call = &logged.call;
                            let (line, all) = (logged.line(), all.clone());
                            v_flex()
                                .id(ix)
                                .w_full()
                                .gap_0p5()
                                .px_2()
                                .py_1()
                                .border_b_1()
                                .border_color(border)
                                .hover(|row| row.bg(hover_bg))
                                // A bug report about an agent is the line it produced, and
                                // retyping one out of a screenshot is how the row index quietly
                                // becomes wrong.
                                .context_menu(move |menu, _window, _cx| {
                                    let (line, all) = (line.clone(), all.clone());
                                    menu.item(PopupMenuItem::new("Copy").on_click(
                                        move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                line.clone(),
                                            ));
                                        },
                                    ))
                                    .item(PopupMenuItem::new("Copy all").on_click(
                                        move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                all.clone(),
                                            ));
                                        },
                                    ))
                                })
                                .child(
                                    h_flex()
                                        .w_full()
                                        .gap_2()
                                        .items_center()
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .text_xs()
                                                .text_color(muted)
                                                .child(elapsed(logged.at)),
                                        )
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .text_xs()
                                                .text_color(info)
                                                .child(call.agent.clone()),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_sm()
                                                .overflow_hidden()
                                                .text_ellipsis()
                                                .child(call.label.clone()),
                                        )
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .text_xs()
                                                .text_color(muted)
                                                .child(took(call)),
                                        ),
                                )
                                .when(!call.detail.is_empty(), |row| {
                                    row.child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .child(call.detail.clone()),
                                    )
                                })
                                .child(
                                    div()
                                        .text_xs()
                                        // A refusal is the whole reason to read this list: it is
                                        // how a wrong token or a misbehaving agent shows up.
                                        .text_color(match call.entry {
                                            Entry::Refused => danger,
                                            Entry::Lifecycle => info,
                                            Entry::Answered => muted,
                                        })
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .child(call.outcome.clone()),
                                )
                        }),
                    )
            }))
    }
}

#[cfg(test)]
mod tests {
    // Never `use super::*` here — the parent's `use gpui::*` would make gpui's `test` macro shadow
    // the built-in one and expand into itself.
    use std::time::Duration;

    use gpui::TestAppContext;

    use crate::panels::agent::{
        AgentCall, AgentHistory, AgentPanel, Entry, HISTORY_LIMIT, View, record,
    };

    fn call(label: &str, entry: Entry) -> AgentCall {
        AgentCall {
            agent: "claude-code".into(),
            label: label.into(),
            detail: "3 row(s)".into(),
            outcome: match entry {
                Entry::Refused => "forbidden".into(),
                Entry::Lifecycle => "first call from this agent".into(),
                Entry::Answered => "3 rows".into(),
            },
            entry,
            took: Duration::from_millis(2),
        }
    }

    /// The panel must render before anything has called, and after — the empty state is what an
    /// archivist sees on every launch that never starts the bridge. All three entry kinds render,
    /// because each takes its own colour branch.
    #[gpui::test]
    fn renders_empty_and_every_entry_kind(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        cx.add_window_view(AgentPanel::new);

        cx.update(|cx| {
            record(call("connected", Entry::Lifecycle), cx);
            record(call("rows", Entry::Answered), cx);
            record(call("stage_findings", Entry::Refused), cx);
            record(call("disconnected", Entry::Lifecycle), cx);
        });
        cx.add_window_view(AgentPanel::new);
    }

    /// Both views render, and the log opens first — an archivist who never starts an agent should
    /// land on the record of what one already did, not on an empty terminal.
    #[gpui::test]
    fn each_view_renders_and_the_log_is_the_default(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        cx.update(|cx| record(call("rows", Entry::Answered), cx));

        let (panel, cx) = cx.add_window_view(AgentPanel::new);
        panel.update(cx, |this, cx| {
            assert_eq!(this.view, View::Log);
            for view in View::ALL {
                this.view = view;
                cx.notify();
            }
        });
    }

    /// Chronological, and an agent in a loop cannot grow the list without bound. The cap has to
    /// drop the *oldest* — trimming the tail of a log that the panel follows would throw away the
    /// entries somebody is actually watching.
    #[gpui::test]
    fn history_is_chronological_and_drops_the_oldest(cx: &mut TestAppContext) {
        cx.update(|cx| {
            record(call("first", Entry::Answered), cx);
            for _ in 0..HISTORY_LIMIT {
                record(call("rows", Entry::Answered), cx);
            }
            record(call("newest", Entry::Answered), cx);

            let history = cx.global::<AgentHistory>();
            assert_eq!(history.calls.len(), HISTORY_LIMIT);
            assert_eq!(
                history.calls.back().unwrap().call.label,
                "newest",
                "the newest entry is at the bottom"
            );
            assert!(
                history.calls.iter().all(|l| l.call.label != "first"),
                "the oldest entry fell off the front"
            );
        });
    }

    /// What lands on the clipboard is the whole entry, in the panel's own column order — a line
    /// pasted into a bug report has to carry who called and what came back, not just the method.
    #[gpui::test]
    fn an_entry_copies_as_one_tab_separated_line(cx: &mut TestAppContext) {
        cx.update(|cx| {
            record(call("rows", Entry::Answered), cx);
            let line = cx.global::<AgentHistory>().calls.back().unwrap().line();
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(
                fields,
                ["+0:00", "claude-code", "rows", "3 row(s)", "3 rows", "2ms"]
            );
        });
    }
}
