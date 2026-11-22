//! The gpui view: draws a [`SurfaceState`] with gpui-kit (gpui-component) widgets.

use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, Styled, Subscription, WeakEntity, Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::checkbox::Checkbox;
use gpui_component::group_box::{GroupBox, GroupBoxVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::label::Label;
use gpui_component::radio::{Radio, RadioGroup};
use gpui_component::separator::Separator;
use gpui_component::{ActiveTheme, Disableable, h_flex, v_flex};
use serde_json::Value;

use crate::component::{Align, ButtonVariant, Justify, TextFieldVariant};
use crate::error::A2uiError;
use crate::protocol::{ActionMessage, AgentMessage, utc_timestamp};
use crate::surface::{Change, Node, NodeKind, SurfaceState};

/// What a [`Surface`] tells its host.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceEvent {
    /// The user clicked an enabled `Button`. Send `to_message()` to the agent; treat it as a
    /// request, never as authorization.
    Action(ActionMessage),
    /// Local input could not be applied (a limit was hit). Report it or log it.
    Error(A2uiError),
    /// The agent deleted this surface; drop the entity.
    Deleted,
}

struct BoundInput {
    state: Entity<InputState>,
    binding: Option<String>,
    _subscription: Subscription,
}

/// A rendered A2UI surface. It draws agent-authored content only: the host owns the window, the
/// frame that marks the content as coming from an agent, and any confirmation of what an action
/// does.
pub struct Surface {
    state: SurfaceState,
    tree: Option<Node>,
    inputs: HashMap<String, BoundInput>,
}

impl EventEmitter<SurfaceEvent> for Surface {}

impl Surface {
    pub fn new(state: SurfaceState) -> Self {
        let tree = state.tree().unwrap_or_else(|e| {
            log::error!("a2ui surface \"{}\" could not be resolved: {e}", state.id());
            None
        });
        Self {
            state,
            tree,
            inputs: HashMap::new(),
        }
    }

    pub fn state(&self) -> &SurfaceState {
        &self.state
    }

    /// Applies an agent message addressed to this surface. On error the surface is unchanged and
    /// the error's `to_message()` is the reply for the agent.
    pub fn apply(
        &mut self,
        message: &AgentMessage,
        cx: &mut Context<Self>,
    ) -> Result<Change, A2uiError> {
        let change = self.state.apply(message)?;
        if change == Change::Deleted {
            cx.emit(SurfaceEvent::Deleted);
        }
        self.refresh(cx);
        Ok(change)
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.tree = self.state.tree().unwrap_or_else(|e| {
            log::warn!(
                "a2ui surface \"{}\" could not be resolved: {e}",
                self.state.id()
            );
            cx.emit(SurfaceEvent::Error(e));
            None
        });
        cx.notify();
    }

    fn write(&mut self, path: &str, value: Value, cx: &mut Context<Self>) {
        match self.state.write(path, value) {
            Ok(()) => self.refresh(cx),
            Err(e) => {
                log::warn!(
                    "a2ui surface \"{}\" rejected input at {path}: {e}",
                    self.state.id()
                );
                cx.emit(SurfaceEvent::Error(e));
                cx.notify();
            }
        }
    }

    fn choose(
        &mut self,
        path: &str,
        value: &str,
        on: bool,
        multiple: bool,
        cx: &mut Context<Self>,
    ) {
        let mut selected: Vec<Value> = match multiple {
            true => crate::pointer::get(self.state.data_model(), path)
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            false => Vec::new(),
        };
        selected.retain(|v| v.as_str() != Some(value));
        if on {
            selected.push(Value::String(value.to_owned()));
        }
        self.write(path, Value::Array(selected), cx);
    }

    fn click(&mut self, component_id: &str, scope: Option<&str>, cx: &mut Context<Self>) {
        match self
            .state
            .action(component_id, scope, utc_timestamp(SystemTime::now()))
        {
            Ok(action) => cx.emit(SurfaceEvent::Action(action)),
            Err(e) => log::warn!("a2ui click on \"{component_id}\" sent no action: {e}"),
        }
    }

    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let fields: Vec<_> = self
            .tree
            .iter()
            .flat_map(Node::descendants)
            .filter_map(|node| match &node.kind {
                NodeKind::TextField {
                    value,
                    placeholder,
                    variant,
                    binding,
                    ..
                } => Some((
                    node.key.clone(),
                    value.clone(),
                    placeholder.clone(),
                    *variant == TextFieldVariant::Obscured,
                    binding.clone(),
                )),
                _ => None,
            })
            .collect();
        let live: HashSet<_> = fields.iter().map(|f| f.0.clone()).collect();
        self.inputs.retain(|key, _| live.contains(key));
        for (key, value, placeholder, masked, binding) in fields {
            if let Some(bound) = self.inputs.get_mut(&key) {
                bound.binding = binding;
                if bound.state.read(cx).value() != value {
                    bound
                        .state
                        .update(cx, |s, cx| s.set_value(value, window, cx));
                }
                continue;
            }
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .masked(masked)
                    .default_value(value)
            });
            let input_key = key.clone();
            let subscription = cx.subscribe_in(
                &state,
                window,
                move |this, input, event: &InputEvent, _, cx| {
                    let binding = this.inputs.get(&input_key).and_then(|b| b.binding.clone());
                    if let (InputEvent::Change, Some(path)) = (event, binding) {
                        let text = input.read(cx).value().to_string();
                        this.write(&path, Value::String(text), cx);
                    }
                },
            );
            self.inputs.insert(
                key,
                BoundInput {
                    state,
                    binding,
                    _subscription: subscription,
                },
            );
        }
    }
}

impl Render for Surface {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_inputs(window, cx);
        let surface = cx.entity().downgrade();
        let inputs: HashMap<_, _> = self
            .inputs
            .iter()
            .map(|(k, b)| (k.clone(), b.state.clone()))
            .collect();
        v_flex().size_full().children(
            self.tree
                .as_ref()
                .map(|root| render_node(root, &inputs, &surface, cx)),
        )
    }
}

fn render_node(
    node: &Node,
    inputs: &HashMap<String, Entity<InputState>>,
    surface: &WeakEntity<Surface>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let (danger, muted) = (theme.danger, theme.muted_foreground);
    let errors = |errors: &[String]| {
        v_flex().children(
            errors
                .iter()
                .map(|e| Label::new(e.clone()).text_xs().text_color(danger)),
        )
    };
    let flex = |vertical: bool, justify: Justify, align: Align, children: &[Node]| {
        match vertical {
            true => v_flex(),
            false => h_flex(),
        }
        .gap_2()
        .map(|d| match justify {
            Justify::Start | Justify::Stretch => d.justify_start(),
            Justify::Center => d.justify_center(),
            Justify::End => d.justify_end(),
            Justify::SpaceBetween => d.justify_between(),
            Justify::SpaceAround => d.justify_around(),
            Justify::SpaceEvenly => d.justify_evenly(),
        })
        .map(|d| match align {
            Align::Start => d.items_start(),
            Align::Center => d.items_center(),
            Align::End => d.items_end(),
            Align::Stretch => d.items_stretch(),
        })
        .children(children.iter().map(|c| render_node(c, inputs, surface, cx)))
    };
    let key = node.key.clone();
    let element = match &node.kind {
        NodeKind::Text { text, caption } => Label::new(text.clone())
            .when(*caption, |l| l.text_sm().text_color(muted))
            .into_any_element(),
        NodeKind::Row {
            children,
            justify,
            align,
        } => flex(false, *justify, *align, children).into_any_element(),
        NodeKind::Column {
            children,
            justify,
            align,
        } => flex(true, *justify, *align, children).into_any_element(),
        NodeKind::List {
            children,
            horizontal,
            align,
        } => flex(!horizontal, Justify::Start, *align, children).into_any_element(),
        NodeKind::Card { child } => GroupBox::new()
            .outline()
            .child(render_node(child, inputs, surface, cx))
            .into_any_element(),
        NodeKind::Divider { vertical: true } => Separator::vertical().into_any_element(),
        NodeKind::Divider { vertical: false } => Separator::horizontal().into_any_element(),
        NodeKind::TextField {
            label,
            binding,
            errors: failed,
            ..
        } => v_flex()
            .gap_1()
            .child(Label::new(label.clone()).text_sm())
            .children(
                inputs
                    .get(&node.key)
                    .map(|state| Input::new(state).disabled(binding.is_none())),
            )
            .child(errors(failed))
            .into_any_element(),
        NodeKind::CheckBox {
            label,
            checked,
            binding,
            errors: failed,
        } => {
            let (surface, binding_path) = (surface.clone(), binding.clone());
            v_flex()
                .gap_1()
                .child(
                    Checkbox::new(key)
                        .label(label.clone())
                        .checked(*checked)
                        .disabled(binding.is_none())
                        .on_click(move |checked, _, cx| {
                            let (Some(path), value) = (&binding_path, Value::Bool(*checked)) else {
                                return;
                            };
                            surface
                                .update(cx, |this, cx| this.write(path, value, cx))
                                .ok();
                        }),
                )
                .child(errors(failed))
                .into_any_element()
        }
        NodeKind::ChoicePicker {
            label,
            multiple,
            options,
            selected,
            binding,
            errors: failed,
        } => {
            let choose = |value: String, on: bool| {
                let (surface, path, multiple) = (surface.clone(), binding.clone(), *multiple);
                move |cx: &mut App| {
                    let Some(path) = &path else { return };
                    surface
                        .update(cx, |this, cx| this.choose(path, &value, on, multiple, cx))
                        .ok();
                }
            };
            let values: Vec<String> = options.iter().map(|(_, v)| v.clone()).collect();
            v_flex()
                .gap_1()
                .children(label.clone().map(|l| Label::new(l).text_sm()))
                .map(|d| match multiple {
                    true => d.children(options.iter().enumerate().map(|(i, (text, value))| {
                        let on = selected.contains(value);
                        let toggle = choose(value.clone(), !on);
                        Checkbox::new((SharedString::from(key.clone()), i))
                            .label(text.clone())
                            .checked(on)
                            .disabled(binding.is_none())
                            .on_click(move |_, _, cx| toggle(cx))
                    })),
                    false => {
                        let picks: Vec<_> =
                            values.iter().map(|v| choose(v.clone(), true)).collect();
                        d.child(
                            RadioGroup::vertical(key.clone())
                                .selected_index(values.iter().position(|v| selected.contains(v)))
                                .disabled(binding.is_none())
                                .children(options.iter().enumerate().map(|(i, (text, _))| {
                                    Radio::new((SharedString::from(key.clone()), i))
                                        .label(text.clone())
                                }))
                                .on_click(move |index, _, cx| {
                                    if let Some(pick) = picks.get(*index) {
                                        pick(cx);
                                    }
                                }),
                        )
                    }
                })
                .child(errors(failed))
                .into_any_element()
        }
        NodeKind::Button {
            child,
            variant,
            enabled,
        } => {
            let content = render_node(child, inputs, surface, cx);
            let (surface, id, scope) = (
                surface.clone(),
                node.component_id.clone(),
                node.scope.clone(),
            );
            let selector = format!("a2ui-{key}");
            Button::new(key)
                .map(|b| match variant {
                    ButtonVariant::Default => b,
                    ButtonVariant::Primary => b.primary(),
                    ButtonVariant::Borderless => b.ghost(),
                })
                .when_some(node.accessibility_label.clone(), |b, l| {
                    b.accessibility_label(l)
                })
                .disabled(!enabled)
                .debug_selector(move || selector)
                .child(content)
                .on_click(move |_, _, cx| {
                    surface
                        .update(cx, |this, cx| this.click(&id, scope.as_deref(), cx))
                        .ok();
                })
                .into_any_element()
        }
    };
    match node.weight {
        Some(weight) => div()
            .flex()
            .flex_col()
            .flex_basis(px(0.))
            .map(|mut d| {
                d.style().flex_grow = Some(weight);
                d
            })
            .child(element)
            .into_any_element(),
        None => element,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use serde_json::json;

    use super::{Surface, SurfaceEvent};
    use crate::protocol::{AgentMessage, BASIC_CATALOG_ID};
    use crate::surface::{Limits, SurfaceState};

    fn mount(
        cx: &mut TestAppContext,
        data: serde_json::Value,
    ) -> (
        Entity<Surface>,
        &mut VisualTestContext,
        Rc<RefCell<Vec<SurfaceEvent>>>,
    ) {
        cx.update(gpui_component::init);
        let AgentMessage::CreateSurface(create) = AgentMessage::from_value(json!({
            "version": "1.0",
            "createSurface": {
                "surfaceId": "form",
                "catalogId": BASIC_CATALOG_ID,
                "components": [
                    {"id": "root", "component": "Column", "children": ["email", "submit"]},
                    {"id": "email", "component": "TextField", "label": "Email", "value": {"@path": "/email"}},
                    {"id": "submit_label", "component": "Text", "text": "Send"},
                    {"id": "submit", "component": "Button", "child": "submit_label",
                     "action": {"event": {"name": "send", "context": {"email": {"@path": "/email"}}}},
                     "@checks": [{"condition": {"@call": "required", "args": {"value": {"@path": "/email"}}}}]}
                ],
                "dataModel": data
            }
        }))
        .unwrap() else {
            unreachable!()
        };
        let state = SurfaceState::create(create, Limits::default()).unwrap();
        let (surface, cx) = cx.add_window_view(|_, _| Surface::new(state));
        let events = Rc::new(RefCell::new(Vec::new()));
        cx.update({
            let events = events.clone();
            let surface = surface.clone();
            move |_, cx| {
                cx.subscribe(&surface, move |_, event: &SurfaceEvent, _| {
                    events.borrow_mut().push(event.clone())
                })
                .detach()
            }
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        (surface, cx, events)
    }

    fn click_submit(cx: &mut VisualTestContext) {
        let bounds = cx.debug_bounds("a2ui-submit").expect("the button is drawn");
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    #[gpui::test]
    fn a_click_emits_exactly_one_action(cx: &mut TestAppContext) {
        let (_, cx, events) = mount(cx, json!({"email": "a@b.c"}));
        click_submit(cx);
        let events = events.borrow();
        assert_eq!(events.len(), 1, "{events:?}");
        let SurfaceEvent::Action(action) = &events[0] else {
            panic!("expected an action, got {:?}", events[0])
        };
        assert_eq!(action.name, "send");
        assert_eq!(action.source_component_id, "submit");
        assert_eq!(action.context["email"], json!("a@b.c"));
    }

    #[gpui::test]
    fn rendering_and_failing_checks_emit_nothing(cx: &mut TestAppContext) {
        let (_, cx, events) = mount(cx, json!({}));
        click_submit(cx);
        assert!(events.borrow().is_empty(), "{:?}", events.borrow());
    }

    #[gpui::test]
    fn typing_writes_the_bound_path_and_agent_updates_reach_the_field(cx: &mut TestAppContext) {
        let (surface, cx, events) = mount(cx, json!({}));
        let input = surface.read_with(cx, |s, _| s.inputs["email"].state.clone());
        cx.update(|window, cx| input.update(cx, |s, cx| s.insert("ada@example.org", window, cx)));
        surface.read_with(cx, |s, _| {
            assert_eq!(s.state().data_model(), &json!({"email": "ada@example.org"}))
        });
        assert!(events.borrow().is_empty(), "typing never sends an action");

        let update = AgentMessage::from_value(json!({
            "version": "1.0",
            "updateDataModel": {"surfaceId": "form", "path": "/email", "value": "grace@example.org"}
        }))
        .unwrap();
        surface.update(cx, |s, cx| s.apply(&update, cx)).unwrap();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            input.read_with(cx, |s, _| s.value().to_string()),
            "grace@example.org"
        );
        click_submit(cx);
        let SurfaceEvent::Action(action) = &events.borrow()[0] else {
            panic!("expected an action")
        };
        assert_eq!(action.context["email"], json!("grace@example.org"));
    }

    #[gpui::test]
    fn delete_surface_is_announced(cx: &mut TestAppContext) {
        let (surface, cx, events) = mount(cx, json!({}));
        let delete = AgentMessage::from_value(
            json!({"version": "1.0", "deleteSurface": {"surfaceId": "form"}}),
        )
        .unwrap();
        surface.update(cx, |s, cx| s.apply(&delete, cx)).unwrap();
        assert_eq!(events.borrow().as_slice(), &[SurfaceEvent::Deleted]);
    }
}
