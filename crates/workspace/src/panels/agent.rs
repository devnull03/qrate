use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::dock::{DockPlacement, Panel, PanelControl, PanelEvent};
use gpui_component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};

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

/// How often the panel drains a running Pi session — roughly a frame, so output arrives as it is
/// written rather than in visible steps.
const LIVE_TICK: Duration = Duration::from_millis(33);
/// The tick with no session open. The cost of the slower rate is paid once, on the first frame of
/// a newly started session; the cost of *not* slowing down is paid from launch until quit.
const IDLE_TICK: Duration = Duration::from_millis(250);

fn terminal_dimensions(width: Pixels, height: Pixels, crop: Pixels) -> (usize, usize) {
    (
        ((f32::from(width) - 16.) / 8.).floor().max(2.) as usize,
        ((f32::from(height) - 8. - f32::from(crop)) / 16.)
            .floor()
            .max(2.) as usize,
    )
}

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
    terminal: agent_runtime::AgentTerminal,
    terminal_size: (usize, usize),
    opened_auth_urls: HashSet<String>,
    _terminal_task: Task<()>,
    /// Refreshes on any entry. One `observe_global` and no re-binding — the history is plain data
    /// that is never rebuilt.
    _sub: Subscription,
    /// Re-renders when the bottom dock opens or closes, so the padding tracks it.
    _crop_sub: Subscription,
}

impl AgentPanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let scroll = ScrollHandle::new();
        let terminal_task = cx.spawn(async move |this, cx| {
            // The panel is built with the window, not when it is first opened, so this loop runs
            // for the app's whole life. At 30 Hz unconditionally it woke the main thread thirty
            // times a second to ask a terminal that did not exist whether it had output. The fast
            // tick is what a live session needs; with no session there is nothing to drain.
            let mut idle = true;
            loop {
                let tick = if idle { IDLE_TICK } else { LIVE_TICK };
                cx.background_executor().timer(tick).await;
                let keep_going = this
                    .update(cx, |panel, cx| {
                        if panel.terminal.poll() {
                            cx.notify();
                        }
                        idle = !panel.terminal.is_running();
                        true
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        });
        Self {
            focus_handle: cx.focus_handle(),
            view: View::Terminal,
            scroll: scroll.clone(),
            terminal: Default::default(),
            terminal_size: (100, 32),
            opened_auth_urls: HashSet::new(),
            _terminal_task: terminal_task,
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
        let panel = cx.entity().downgrade();
        Some(
            h_flex()
                .gap_1()
                .items_center()
                .child(
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
                            if this.view == View::Terminal && !this.terminal.is_running() {
                                this.opened_auth_urls.clear();
                                this.terminal.start(true, cx);
                            }
                            cx.notify();
                        }))
                        .children(
                            View::ALL
                                .into_iter()
                                .map(|view| Tab::new().icon(view.icon()).label(view.label())),
                        ),
                )
                // Starting a session is the common action, so it is the click; stopping and
                // restarting an existing one are rare, so they are the right-click.
                .child(
                    Button::new("agent-new-session")
                        .icon(IconName::Plus)
                        .ghost()
                        .xsmall()
                        .tooltip("New Pi session")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.opened_auth_urls.clear();
                            this.view = View::Terminal;
                            this.terminal.start(false, cx);
                            cx.notify();
                        }))
                        .context_menu(move |menu, _window, _cx| {
                            let (stop, restart) = (panel.clone(), panel.clone());
                            menu.item(PopupMenuItem::new("Stop").on_click(move |_, _, cx| {
                                stop.update(cx, |this, cx| {
                                    this.terminal.stop();
                                    cx.notify();
                                })
                                .ok();
                            }))
                            .item(
                                PopupMenuItem::new("Restart").on_click(move |_, _, cx| {
                                    restart
                                        .update(cx, |this, cx| {
                                            this.opened_auth_urls.clear();
                                            this.terminal.start(true, cx);
                                            cx.notify();
                                        })
                                        .ok();
                                }),
                            )
                        }),
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.view == View::Terminal
            && !self.terminal.is_running()
            && matches!(
                self.terminal.status(),
                "Pi has not started." | "Open a qrate project before starting Pi."
            )
            && cx.has_global::<settings::project::CurrentProject>()
        {
            self.terminal.start(true, cx);
        }
        let (
            foreground,
            background,
            muted,
            danger,
            success,
            warning,
            info,
            accent,
            border,
            hover_bg,
        ) = {
            let theme = cx.theme();
            (
                theme.foreground,
                theme.background,
                theme.muted_foreground,
                theme.danger,
                theme.success,
                theme.warning,
                theme.info,
                theme.accent,
                theme.border,
                theme.secondary_hover,
            )
        };
        let crop = cx.try_global::<BottomDockCrop>().map_or(px(0.), |c| c.0);
        let (terminal_cols, terminal_lines) = self.terminal_size;
        self.terminal.resize(terminal_cols, terminal_lines);
        let agent_panel = cx.entity().downgrade();
        let terminal_screen = self.terminal.screen(agent_runtime::TerminalPalette {
            foreground,
            background,
            muted,
            red: danger,
            green: success,
            yellow: warning,
            blue: info,
            magenta: accent,
            cyan: info,
        });
        if let Some(url) = terminal_screen
            .auth_urls
            .iter()
            .find(|url| self.opened_auth_urls.insert((*url).clone()))
            .cloned()
        {
            window.defer(cx, move |_window, cx| cx.open_url(&url));
        }
        let history = cx.try_global::<AgentHistory>();
        let empty = history.is_none_or(|h| h.calls.is_empty());
        let all = history.map_or_else(String::new, |h| {
            h.calls
                .iter()
                .map(Logged::line)
                .collect::<Vec<_>>()
                .join("\n")
        });
        let terminal_content = if terminal_screen.text.is_empty() {
            StyledText::new(
                "Use /login openrouter once, then Pi will use OpenRouter's free router by default.",
            )
        } else {
            StyledText::new(terminal_screen.text).with_highlights(terminal_screen.highlights)
        };
        let terminal_backgrounds = terminal_screen.backgrounds;
        let terminal_status = self.terminal.status().to_owned();

        v_flex()
            .size_full()
            // Same as the Problems panel: the dock focuses this handle, so an element has to track
            // it or focus lands nowhere and window-wide shortcuts stop dispatching.
            .track_focus(&self.focus_handle)
            .id("agent-panel")
            .role(Role::Group)
            .aria_label("Agent")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if this.view == View::Terminal && this.terminal.input(event, cx) {
                    cx.stop_propagation();
                }
            }))
            .when(self.view == View::Terminal, |panel| {
                panel.child(
                    v_flex()
                        .size_full()
                        .min_h_0()
                        .child(
                            h_flex()
                                .flex_shrink_0()
                                .gap_1()
                                .items_center()
                                .px_2()
                                .py_1()
                                .border_b_1()
                                .border_color(border)
                                .child(
                                    div()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(muted)
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .child(terminal_status),
                                ),
                        )
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_h_0()
                                .overflow_hidden()
                                .bg(background)
                                .px_2()
                                .pt_1()
                                .pb(px(4.) + crop)
                                .text_sm()
                                .line_height(px(16.))
                                .font_family("monospace")
                                .on_scroll_wheel(cx.listener(
                                    |this, event: &ScrollWheelEvent, _window, cx| {
                                        let lines = match event.delta {
                                            ScrollDelta::Lines(delta) => delta.y,
                                            ScrollDelta::Pixels(delta) => {
                                                f32::from(delta.y) / 16.
                                            }
                                        };
                                        this.terminal.scroll(lines);
                                        cx.stop_propagation();
                                        cx.notify();
                                    },
                                ))
                                .child({
                                    canvas(
                                        move |bounds, _, cx| {
                                            let next = terminal_dimensions(
                                                bounds.size.width,
                                                bounds.size.height,
                                                crop,
                                            );
                                            let Some(panel) = agent_panel.upgrade() else {
                                                return;
                                            };
                                            panel.update(cx, |panel, cx| {
                                                if panel.terminal_size != next {
                                                    panel.terminal_size = next;
                                                    panel.terminal.resize(next.0, next.1);
                                                    cx.notify();
                                                }
                                            });
                                        },
                                        move |bounds, _, window, _| {
                                            for background in &terminal_backgrounds {
                                                let bounds = Bounds {
                                                    origin: point(
                                                        bounds.origin.x
                                                            + px(8.)
                                                            + px(
                                                                8. * background.start_column
                                                                    as f32,
                                                            ),
                                                        bounds.origin.y
                                                            + px(4.)
                                                            + px(16. * background.line as f32),
                                                    ),
                                                    size: size(
                                                        px(
                                                            8. * (background.end_column
                                                                - background.start_column)
                                                                as f32,
                                                        ),
                                                        px(16.),
                                                    ),
                                                };
                                                window.paint_quad(fill(bounds, background.color));
                                            }
                                        },
                                    )
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .size_full()
                                })
                                .child(terminal_content),
                        ),
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

    use gpui::{TestAppContext, px};

    use crate::panels::agent::{
        AgentCall, AgentHistory, AgentPanel, Entry, HISTORY_LIMIT, View, record,
        terminal_dimensions,
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

    /// Both views render, and the terminal opens first so Pi is immediately available.
    #[gpui::test]
    fn each_view_renders_and_the_terminal_is_the_default(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        cx.update(|cx| record(call("rows", Entry::Answered), cx));

        let (panel, cx) = cx.add_window_view(AgentPanel::new);
        panel.update(cx, |this, cx| {
            assert_eq!(this.view, View::Terminal);
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

    #[test]
    fn terminal_dimensions_follow_the_measured_panel() {
        assert_eq!(terminal_dimensions(px(816.), px(520.), px(0.)), (100, 32));
        assert_eq!(terminal_dimensions(px(416.), px(264.), px(0.)), (50, 16));
        assert_eq!(terminal_dimensions(px(8.), px(8.), px(30.)), (2, 2));
    }
}
