//! One surface's state — its components and data model — and the resolved tree it renders.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use crate::checks::{self, Dynamic, Scope};
use crate::component::{
    Align, ButtonVariant, ChildList, Component, ComponentKind, Justify, TextFieldVariant,
};
use crate::error::{A2uiError, ErrorCode};
use crate::pointer;
use crate::protocol::{ActionMessage, AgentMessage, BASIC_CATALOG_ID, CreateSurface};

/// Bounds on what an agent can make a surface hold. Exceeding one rejects the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    pub max_components: usize,
    /// Nesting depth from `root`, counting `root` as 1.
    pub max_depth: usize,
    /// Characters in any one string, in components or the data model.
    pub max_string_len: usize,
    /// Size of the data model serialized as JSON.
    pub max_data_model_bytes: usize,
    /// Nodes in the resolved tree, after `List` templates are expanded.
    pub max_rendered_nodes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_components: 500,
            max_depth: 16,
            max_string_len: 8_192,
            max_data_model_bytes: 256 * 1024,
            max_rendered_nodes: 2_000,
        }
    }
}

/// What an accepted message did to the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Updated,
    Deleted,
}

/// A component resolved against the data model, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub component_id: String,
    /// Unique per rendered instance: the component id plus its template scope.
    pub key: String,
    /// The `List` item pointer relative bindings resolve against, if inside a template.
    pub scope: Option<String>,
    pub weight: Option<f32>,
    pub accessibility_label: Option<String>,
    pub kind: NodeKind,
}

impl Node {
    pub fn children(&self) -> Vec<&Node> {
        match &self.kind {
            NodeKind::Row { children, .. }
            | NodeKind::Column { children, .. }
            | NodeKind::List { children, .. } => children.iter().collect(),
            NodeKind::Card { child } | NodeKind::Button { child, .. } => vec![child],
            _ => Vec::new(),
        }
    }

    /// This node and everything under it, breadth first.
    pub fn descendants(&self) -> Vec<&Node> {
        let mut out = vec![self];
        let mut i = 0;
        while i < out.len() {
            let next = out[i].children();
            out.extend(next);
            i += 1;
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    Text {
        text: String,
        caption: bool,
    },
    Row {
        children: Vec<Node>,
        justify: Justify,
        align: Align,
    },
    Column {
        children: Vec<Node>,
        justify: Justify,
        align: Align,
    },
    List {
        children: Vec<Node>,
        horizontal: bool,
        align: Align,
    },
    Card {
        child: Box<Node>,
    },
    Divider {
        vertical: bool,
    },
    /// `binding` is the absolute pointer user input writes to; `None` makes the field read-only.
    TextField {
        label: String,
        value: String,
        placeholder: String,
        variant: TextFieldVariant,
        binding: Option<String>,
        errors: Vec<String>,
    },
    CheckBox {
        label: String,
        checked: bool,
        binding: Option<String>,
        errors: Vec<String>,
    },
    ChoicePicker {
        label: Option<String>,
        multiple: bool,
        /// `(label, value)` pairs.
        options: Vec<(String, String)>,
        selected: Vec<String>,
        binding: Option<String>,
        errors: Vec<String>,
    },
    Button {
        child: Box<Node>,
        variant: ButtonVariant,
        /// False while any of the button's `@checks` fails.
        enabled: bool,
    },
}

#[derive(Debug, Clone)]
pub struct SurfaceState {
    id: String,
    send_data_model: bool,
    components: HashMap<String, Component>,
    model: Value,
    limits: Limits,
}

impl SurfaceState {
    /// Builds a surface from `createSurface`, applying its data model before its components.
    pub fn create(message: CreateSurface, limits: Limits) -> Result<Self, A2uiError> {
        let id = message.surface_id;
        let fail = |e: A2uiError| e.with_surface(id.clone());
        if message.catalog_id.as_deref() != Some(BASIC_CATALOG_ID) {
            return Err(fail(A2uiError::validation(
                "/catalogId",
                format!("This renderer resolves components only against {BASIC_CATALOG_ID}."),
            )));
        }
        let mut state = Self {
            id: id.clone(),
            send_data_model: message.send_data_model,
            components: HashMap::new(),
            model: Value::Object(Map::new()),
            limits,
        };
        if let Some(model) = message.data_model {
            let model = Value::Object(model);
            check_strings(&model, state.limits.max_string_len, "/dataModel").map_err(fail)?;
            state.model = model;
        }
        state.merge(&message.components).map_err(fail)?;
        state.validate().map_err(fail)?;
        Ok(state)
    }

    /// Applies an update for this surface. On error nothing changes.
    pub fn apply(&mut self, message: &AgentMessage) -> Result<Change, A2uiError> {
        let fail = |e: A2uiError| e.with_surface(self.id.clone());
        if message.surface_id() != self.id {
            return Err(fail(A2uiError::new(
                ErrorCode::IntegrityError,
                "/surfaceId",
                format!(
                    "Message is for surface \"{}\", not this one.",
                    message.surface_id()
                ),
            )));
        }
        let mut next = self.clone();
        match message {
            AgentMessage::CreateSurface(_) => {
                return Err(fail(A2uiError::new(
                    ErrorCode::IntegrityError,
                    "/surfaceId",
                    "A surface with this id already exists; delete it first.",
                )));
            }
            AgentMessage::DeleteSurface(_) => {
                self.components.clear();
                self.model = Value::Object(Map::new());
                return Ok(Change::Deleted);
            }
            AgentMessage::UpdateComponents(update) => {
                next.merge(&update.components).map_err(fail)?
            }
            AgentMessage::UpdateDataModel(update) => {
                let path = update.path.as_deref().unwrap_or("/");
                check_strings(&update.value, self.limits.max_string_len, "/value").map_err(fail)?;
                pointer::set(&mut next.model, path, update.value.clone())
                    .map_err(|e| fail(A2uiError::validation("/path", e)))?;
            }
        }
        next.validate().map_err(fail)?;
        *self = next;
        Ok(Change::Updated)
    }

    /// Writes user input into the data model at an absolute pointer.
    pub fn write(&mut self, path: &str, value: Value) -> Result<(), A2uiError> {
        let fail = |e: A2uiError| e.with_surface(self.id.clone());
        check_strings(&value, self.limits.max_string_len, path).map_err(fail)?;
        let mut next = self.clone();
        pointer::set(&mut next.model, path, value)
            .map_err(|e| fail(A2uiError::validation(path, e)))?;
        next.validate().map_err(fail)?;
        *self = next;
        Ok(())
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn data_model(&self) -> &Value {
        &self.model
    }

    /// Whether the agent asked for the data model alongside every action (`sendDataModel`).
    pub fn send_data_model(&self) -> bool {
        self.send_data_model
    }

    pub fn component(&self, id: &str) -> Option<&Component> {
        self.components.get(id)
    }

    /// The resolved tree under `root`, or `None` while no `root` has arrived.
    pub fn tree(&self) -> Result<Option<Node>, A2uiError> {
        if !self.components.contains_key("root") {
            return Ok(None);
        }
        let mut budget = self.limits.max_rendered_nodes;
        self.build("root", None, &mut budget).map(Some)
    }

    /// The `action` message for a click on `component_id`, resolving its context in `scope`.
    /// Fails if the component is not a `Button` or one of its checks fails.
    pub fn action(
        &self,
        component_id: &str,
        scope: Option<&str>,
        timestamp: String,
    ) -> Result<ActionMessage, A2uiError> {
        let fail = |message: &str| A2uiError::validation("", message).with_surface(self.id.clone());
        let component = self
            .components
            .get(component_id)
            .ok_or_else(|| fail("No such component."))?;
        let ComponentKind::Button { action, .. } = &component.kind else {
            return Err(fail("Only a Button dispatches actions."));
        };
        let scope = Scope {
            model: &self.model,
            base: scope,
        };
        if !checks::failures(&component.checks, scope).is_empty() {
            return Err(fail("The button's checks do not pass."));
        }
        Ok(ActionMessage {
            name: action.name.clone(),
            surface_id: self.id.clone(),
            source_component_id: component_id.to_owned(),
            timestamp,
            user_message: action.user_message.as_ref().map(|m| m.eval_text(scope)),
            context: action
                .context
                .iter()
                .map(|(key, value)| (key.clone(), value.eval(scope)))
                .collect(),
        })
    }

    fn merge(&mut self, components: &[Value]) -> Result<(), A2uiError> {
        let mut seen = HashSet::new();
        for (i, raw) in components.iter().enumerate() {
            let path = format!("/components/{i}");
            check_strings(raw, self.limits.max_string_len, &path)?;
            let component = Component::parse(raw, &path)?;
            if !seen.insert(component.id.clone()) {
                return Err(A2uiError::validation(
                    format!("{path}/id"),
                    format!(
                        "Component id \"{}\" appears twice in one message.",
                        component.id
                    ),
                ));
            }
            self.components.insert(component.id.clone(), component);
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), A2uiError> {
        if self.components.len() > self.limits.max_components {
            return Err(A2uiError::validation(
                "/components",
                format!(
                    "A surface holds at most {} components.",
                    self.limits.max_components
                ),
            ));
        }
        let bytes = serde_json::to_vec(&self.model).map_or(usize::MAX, |b| b.len());
        if bytes > self.limits.max_data_model_bytes {
            return Err(A2uiError::validation(
                "/dataModel",
                format!(
                    "The data model is limited to {} bytes.",
                    self.limits.max_data_model_bytes
                ),
            ));
        }
        if self.components.contains_key("root") {
            self.check_structure("root", 1, &mut Vec::new(), &mut HashSet::new())?;
            self.tree()?;
        }
        Ok(())
    }

    fn check_structure<'a>(
        &'a self,
        id: &'a str,
        depth: usize,
        path: &mut Vec<&'a str>,
        done: &mut HashSet<&'a str>,
    ) -> Result<(), A2uiError> {
        if path.contains(&id) {
            return Err(A2uiError::validation(
                "/components",
                format!("Components form a cycle: {} -> {id}.", path.join(" -> ")),
            ));
        }
        if done.contains(id) {
            return Ok(());
        }
        if depth > self.limits.max_depth {
            return Err(A2uiError::validation(
                "/components",
                format!(
                    "Component \"{id}\" is nested deeper than {}.",
                    self.limits.max_depth
                ),
            ));
        }
        let component = self.components.get(id).ok_or_else(|| {
            A2uiError::validation(
                "/components",
                format!(
                    "Component \"{}\" references \"{id}\", which is not defined.",
                    path.last().unwrap_or(&"root")
                ),
            )
        })?;
        path.push(id);
        for child in component.kind.child_ids() {
            self.check_structure(child, depth + 1, path, done)?;
        }
        path.pop();
        done.insert(id);
        Ok(())
    }

    fn build(
        &self,
        id: &str,
        scope: Option<String>,
        budget: &mut usize,
    ) -> Result<Node, A2uiError> {
        *budget = budget.checked_sub(1).ok_or_else(|| {
            A2uiError::validation(
                "/components",
                format!(
                    "The surface expands to more than {} nodes.",
                    self.limits.max_rendered_nodes
                ),
            )
        })?;
        let component = self.components.get(id).ok_or_else(|| {
            A2uiError::validation("/components", format!("Component \"{id}\" is not defined."))
        })?;
        let at = Scope {
            model: &self.model,
            base: scope.as_deref(),
        };
        let binding = |value: &Dynamic| match value {
            Dynamic::Path(path) => Some(pointer::resolve(scope.as_deref(), path)),
            _ => None,
        };
        let errors = || checks::failures(&component.checks, at);
        let children = |list: &ChildList, budget: &mut usize| -> Result<Vec<Node>, A2uiError> {
            match list {
                ChildList::Static(ids) => ids
                    .iter()
                    .map(|c| self.build(c, scope.clone(), budget))
                    .collect(),
                ChildList::Template { component_id, path } => {
                    let base = pointer::resolve(scope.as_deref(), path);
                    let count = pointer::get(&self.model, &base)
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len);
                    (0..count)
                        .map(|i| self.build(component_id, Some(format!("{base}/{i}")), budget))
                        .collect()
                }
            }
        };
        let kind = match &component.kind {
            ComponentKind::Text { text, caption } => NodeKind::Text {
                text: text.eval_text(at),
                caption: *caption,
            },
            ComponentKind::Row {
                children: list,
                justify,
                align,
            } => NodeKind::Row {
                children: children(list, budget)?,
                justify: *justify,
                align: *align,
            },
            ComponentKind::Column {
                children: list,
                justify,
                align,
            } => NodeKind::Column {
                children: children(list, budget)?,
                justify: *justify,
                align: *align,
            },
            ComponentKind::List {
                children: list,
                horizontal,
                align,
            } => NodeKind::List {
                children: children(list, budget)?,
                horizontal: *horizontal,
                align: *align,
            },
            ComponentKind::Card { child } => NodeKind::Card {
                child: Box::new(self.build(child, scope.clone(), budget)?),
            },
            ComponentKind::Divider { vertical } => NodeKind::Divider {
                vertical: *vertical,
            },
            ComponentKind::TextField {
                label,
                value,
                placeholder,
                variant,
            } => NodeKind::TextField {
                label: label.eval_text(at),
                value: value.eval_text(at),
                placeholder: placeholder
                    .as_ref()
                    .map(|p| p.eval_text(at))
                    .unwrap_or_default(),
                variant: *variant,
                binding: binding(value),
                errors: errors(),
            },
            ComponentKind::CheckBox { label, value } => NodeKind::CheckBox {
                label: label.eval_text(at),
                checked: value.eval_bool(at),
                binding: binding(value),
                errors: errors(),
            },
            ComponentKind::ChoicePicker {
                label,
                multiple,
                options,
                value,
            } => NodeKind::ChoicePicker {
                label: label.as_ref().map(|l| l.eval_text(at)),
                multiple: *multiple,
                options: options
                    .iter()
                    .map(|o| (o.label.eval_text(at), o.value.clone()))
                    .collect(),
                selected: match value.eval(at) {
                    Value::Array(items) => items
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect(),
                    Value::String(single) => vec![single],
                    _ => Vec::new(),
                },
                binding: binding(value),
                errors: errors(),
            },
            ComponentKind::Button { child, variant, .. } => NodeKind::Button {
                child: Box::new(self.build(child, scope.clone(), budget)?),
                variant: *variant,
                enabled: errors().is_empty(),
            },
        };
        Ok(Node {
            component_id: id.to_owned(),
            key: match &scope {
                Some(scope) => format!("{id}@{scope}"),
                None => id.to_owned(),
            },
            weight: component.weight,
            accessibility_label: component
                .accessibility_label
                .as_ref()
                .map(|l| l.eval_text(at)),
            scope,
            kind,
        })
    }
}

fn check_strings(value: &Value, limit: usize, path: &str) -> Result<(), A2uiError> {
    let too_long = |s: &str| s.len() > limit && s.chars().count() > limit;
    match value {
        Value::String(s) if too_long(s) => Err(A2uiError::validation(
            path,
            format!("Strings are limited to {limit} characters."),
        )),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .try_for_each(|(i, v)| check_strings(v, limit, &format!("{path}/{i}"))),
        Value::Object(map) => map.iter().try_for_each(|(k, v)| {
            let at = format!("{path}/{}", pointer::escape(k));
            match too_long(k) {
                true => Err(A2uiError::validation(
                    at,
                    format!("Keys are limited to {limit} characters."),
                )),
                false => check_strings(v, limit, &at),
            }
        }),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{Change, Limits, Node, NodeKind, SurfaceState};
    use crate::error::ErrorCode;
    use crate::protocol::{AgentMessage, BASIC_CATALOG_ID};

    fn message(body: Value) -> AgentMessage {
        AgentMessage::from_value(body).unwrap()
    }

    fn create_with(
        limits: Limits,
        components: Value,
        data: Value,
    ) -> Result<SurfaceState, crate::A2uiError> {
        let AgentMessage::CreateSurface(create) = message(json!({
            "version": "1.0",
            "createSurface": {
                "surfaceId": "s",
                "catalogId": BASIC_CATALOG_ID,
                "components": components,
                "dataModel": data,
            }
        })) else {
            unreachable!()
        };
        SurfaceState::create(create, limits)
    }

    fn create(components: Value, data: Value) -> SurfaceState {
        create_with(Limits::default(), components, data).unwrap()
    }

    fn update(state: &mut SurfaceState, components: Value) -> Result<Change, crate::A2uiError> {
        state.apply(&message(json!({
            "version": "1.0",
            "updateComponents": {"surfaceId": "s", "components": components}
        })))
    }

    fn set_data(
        state: &mut SurfaceState,
        path: &str,
        value: Value,
    ) -> Result<Change, crate::A2uiError> {
        state.apply(&message(json!({
            "version": "1.0",
            "updateDataModel": {"surfaceId": "s", "path": path, "value": value}
        })))
    }

    fn tree(state: &SurfaceState) -> Node {
        state.tree().unwrap().expect("root is defined")
    }

    fn find<'a>(root: &'a Node, key: &str) -> &'a Node {
        root.descendants()
            .into_iter()
            .find(|n| n.key == key)
            .unwrap_or_else(|| panic!("no node {key}"))
    }

    fn texts(root: &Node) -> Vec<String> {
        root.descendants()
            .into_iter()
            .filter_map(|n| match &n.kind {
                NodeKind::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn form() -> Value {
        json!([
            {"id": "root", "component": "Column", "children": ["email", "terms", "submit"]},
            {"id": "email", "component": "TextField", "label": "Email", "value": {"@path": "/form/email"},
             "@checks": [{"condition": {"@call": "regex", "args": {"value": {"@path": "/form/email"}, "pattern": "^[^@]+@[^@]+$"}},
                          "message": "Enter an email."}]},
            {"id": "terms", "component": "CheckBox", "label": "I agree", "value": {"@path": "/form/terms"}},
            {"id": "submit_label", "component": "Text", "text": "Send"},
            {"id": "submit", "component": "Button", "child": "submit_label", "variant": "primary",
             "action": {"event": {"name": "submit", "userMessage": {"@path": "/form/email"},
                                  "context": {"email": {"@path": "/form/email"}, "terms": {"@path": "/form/terms"}, "formId": "f1"}}},
             "@checks": [{"condition": {"@call": "and", "args": {"values": [
                 {"@path": "/form/terms"},
                 {"@call": "length", "args": {"value": {"@path": "/form/email"}, "min": 3}}
             ]}}, "message": "Fill the form."}]}
        ])
    }

    fn button_enabled(state: &SurfaceState) -> bool {
        match &find(&tree(state), "submit").kind {
            NodeKind::Button { enabled, .. } => *enabled,
            other => panic!("not a button: {other:?}"),
        }
    }

    #[test]
    fn parses_every_supported_message() {
        let create = message(
            json!({"version": "1.0", "createSurface": {"surfaceId": "s", "catalogId": BASIC_CATALOG_ID, "sendDataModel": true}}),
        );
        assert!(matches!(&create, AgentMessage::CreateSurface(c) if c.send_data_model));
        let update = message(
            json!({"version": "1.0", "updateComponents": {"surfaceId": "s", "components": [{"id": "root", "component": "Divider"}]}}),
        );
        assert!(matches!(&update, AgentMessage::UpdateComponents(u) if u.components.len() == 1));
        let data = message(
            json!({"version": "1.0", "updateDataModel": {"surfaceId": "s", "value": null}}),
        );
        assert!(
            matches!(&data, AgentMessage::UpdateDataModel(d) if d.path.is_none() && d.value.is_null())
        );
        let delete =
            AgentMessage::parse(r#"{"version": "1.0", "deleteSurface": {"surfaceId": "s"}}"#)
                .unwrap();
        assert_eq!(delete.surface_id(), "s");
    }

    #[test]
    fn rejects_malformed_envelopes() {
        let err = |v: Value| AgentMessage::from_value(v).unwrap_err();
        assert_eq!(
            err(json!({"version": "0.9", "deleteSurface": {"surfaceId": "s"}})).path,
            "/version"
        );
        assert!(AgentMessage::parse("not json").is_err());
        let two = err(
            json!({"version": "1.0", "deleteSurface": {"surfaceId": "s"}, "updateComponents": {}}),
        );
        assert_eq!(two.code, ErrorCode::ValidationFailed);
        let themed =
            err(json!({"version": "1.0", "createSurface": {"surfaceId": "s", "theme": {}}}));
        assert_eq!(themed.surface_id.as_deref(), Some("s"));
        let missing_value =
            err(json!({"version": "1.0", "updateDataModel": {"surfaceId": "s", "path": "/a"}}));
        assert_eq!(missing_value.path, "/updateDataModel");
        let call = err(
            json!({"version": "1.0", "callRendererFunction": {"functionCallId": "c1", "callFunction": {}}}),
        );
        assert_eq!(call.code, ErrorCode::InvalidFunctionCall);
        assert_eq!(
            call.to_message(),
            json!({"version": "1.0", "error": {"code": "INVALID_FUNCTION_CALL", "message": call.message, "functionCallId": "c1"}})
        );
    }

    #[test]
    fn create_requires_the_basic_catalog() {
        let AgentMessage::CreateSurface(create) = message(
            json!({"version": "1.0", "createSurface": {"surfaceId": "s", "catalogId": "https://example.com/other"}}),
        ) else {
            unreachable!()
        };
        let e = SurfaceState::create(create, Limits::default()).unwrap_err();
        assert_eq!(e.path, "/catalogId");
    }

    #[test]
    fn create_applies_data_before_components() {
        let state = create(
            json!([{"id": "root", "component": "Text", "text": {"@path": "/name"}}]),
            json!({"name": "John Doe"}),
        );
        assert_eq!(texts(&tree(&state)), vec!["John Doe"]);
    }

    #[test]
    fn data_model_updates_upsert_and_rerender() {
        let mut state = create(
            json!([{"id": "root", "component": "Text", "text": {"@path": "/user/name"}}]),
            json!({}),
        );
        assert_eq!(texts(&tree(&state)), vec![""]);
        set_data(&mut state, "/user/name", json!("Jane")).unwrap();
        assert_eq!(texts(&tree(&state)), vec!["Jane"]);
        set_data(&mut state, "/user/name", Value::Null).unwrap();
        assert_eq!(state.data_model(), &json!({"user": {}}));
        state
            .apply(&message(json!({"version": "1.0", "updateDataModel": {"surfaceId": "s", "value": {"user": {"name": "Root"}}}})))
            .unwrap();
        assert_eq!(texts(&tree(&state)), vec!["Root"]);
    }

    #[test]
    fn list_template_repeats_per_item_with_relative_bindings() {
        let state = create(
            json!([
                {"id": "root", "component": "List", "children": {"componentId": "row", "path": "/rows"}},
                {"id": "row", "component": "Row", "children": ["name", "company"]},
                {"id": "name", "component": "Text", "text": {"@path": "name"}},
                {"id": "company", "component": "Text", "text": {"@path": "/company"}}
            ]),
            json!({"company": "Acme", "rows": [{"name": "Alice"}, {"name": "Bob"}]}),
        );
        let root = tree(&state);
        let NodeKind::List { children, .. } = &root.kind else {
            panic!("expected a list")
        };
        assert_eq!(children.len(), 2);
        assert_eq!(children[1].key, "row@/rows/1");
        assert_eq!(texts(&root), vec!["Alice", "Acme", "Bob", "Acme"]);
        assert_eq!(
            find(&root, "name@/rows/0").scope.as_deref(),
            Some("/rows/0")
        );
    }

    #[test]
    fn nested_templates_resolve_against_the_innermost_item() {
        let state = create(
            json!([
                {"id": "root", "component": "Column", "children": {"componentId": "group", "path": "/groups"}},
                {"id": "group", "component": "Column", "children": {"componentId": "item", "path": "items"}},
                {"id": "item", "component": "Text", "text": {"@path": "label"}}
            ]),
            json!({"groups": [{"items": [{"label": "a"}, {"label": "b"}]}, {"items": [{"label": "c"}]}]}),
        );
        assert_eq!(texts(&tree(&state)), vec!["a", "b", "c"]);
    }

    #[test]
    fn unknown_and_unsupported_components_are_rejected_with_their_path() {
        for kind in [
            "Image",
            "Video",
            "AudioPlayer",
            "Modal",
            "Tabs",
            "Slider",
            "Fancy",
        ] {
            let e = create_with(
                Limits::default(),
                json!([{"id": "root", "component": kind, "url": "x"}]),
                json!({}),
            )
            .unwrap_err();
            assert_eq!(e.code, ErrorCode::ValidationFailed);
            assert_eq!(e.path, "/components/0/component", "{kind}");
            assert_eq!(e.surface_id.as_deref(), Some("s"));
        }
        let e = create_with(
            Limits::default(),
            json!([{"id": "root", "component": "Text", "text": "a", "colour": "red"}]),
            json!({}),
        )
        .unwrap_err();
        assert_eq!(e.path, "/components/0/colour");
        let e = create_with(
            Limits::default(),
            json!([{"id": "root", "component": "Button", "child": "x", "action": {"functionCall": {"@call": "openUrl", "args": {"url": "u"}}}}]),
            json!({}),
        )
        .unwrap_err();
        assert_eq!(e.path, "/components/0/action");
        let e = create_with(
            Limits::default(),
            json!([{"id": "bad-id", "component": "Divider"}]),
            json!({}),
        )
        .unwrap_err();
        assert_eq!(e.path, "/components/0/id");
    }

    #[test]
    fn dangling_children_are_rejected_once_root_exists() {
        let mut state = create(
            json!([{"id": "label", "component": "Text", "text": "hi"}]),
            json!({}),
        );
        assert_eq!(
            state.tree().unwrap(),
            None,
            "components buffer until root arrives"
        );
        let e = update(
            &mut state,
            json!([{"id": "root", "component": "Column", "children": ["label", "ghost"]}]),
        )
        .unwrap_err();
        assert!(e.message.contains("ghost"), "{}", e.message);
        assert!(
            state.component("root").is_none(),
            "a rejected message changes nothing"
        );
        update(
            &mut state,
            json!([{"id": "root", "component": "Column", "children": ["label"]}]),
        )
        .unwrap();
        assert_eq!(texts(&tree(&state)), vec!["hi"]);
    }

    #[test]
    fn cycles_are_rejected() {
        let e = create_with(
            Limits::default(),
            json!([
                {"id": "root", "component": "Column", "children": ["a"]},
                {"id": "a", "component": "Card", "child": "b"},
                {"id": "b", "component": "Row", "children": ["a"]}
            ]),
            json!({}),
        )
        .unwrap_err();
        assert!(e.message.contains("cycle"), "{}", e.message);
        let e = create_with(
            Limits::default(),
            json!([{"id": "root", "component": "List", "children": {"componentId": "root", "path": "/x"}}]),
            json!({}),
        )
        .unwrap_err();
        assert!(e.message.contains("cycle"), "{}", e.message);
    }

    #[test]
    fn duplicate_ids_in_one_message_are_rejected() {
        let e = create_with(
            Limits::default(),
            json!([{"id": "root", "component": "Divider"}, {"id": "root", "component": "Divider"}]),
            json!({}),
        )
        .unwrap_err();
        assert_eq!(e.path, "/components/1/id");
    }

    #[test]
    fn limits_reject_instead_of_growing() {
        let tight = Limits {
            max_components: 3,
            max_depth: 3,
            max_string_len: 12,
            max_data_model_bytes: 64,
            max_rendered_nodes: 5,
        };
        let many: Vec<Value> = (0..4)
            .map(|i| json!({"id": format!("d{i}"), "component": "Divider"}))
            .collect();
        assert!(create_with(tight.clone(), Value::Array(many), json!({})).is_err());

        let deep = json!([
            {"id": "root", "component": "Card", "child": "a"},
            {"id": "a", "component": "Card", "child": "b"},
            {"id": "b", "component": "Card", "child": "c"}
        ]);
        let e = create_with(
            Limits {
                max_components: 10,
                ..tight.clone()
            },
            deep,
            json!({}),
        )
        .unwrap_err();
        assert!(e.message.contains("deeper"), "{}", e.message);

        let e = create_with(
            tight.clone(),
            json!([{"id": "root", "component": "Text", "text": "far too long text"}]),
            json!({}),
        )
        .unwrap_err();
        assert_eq!(e.path, "/components/0/text");

        let mut state = create_with(
            tight.clone(),
            json!([{"id": "root", "component": "Divider"}]),
            json!({}),
        )
        .unwrap();
        let e = set_data(&mut state, "/blob", json!(vec!["0123456789"; 8])).unwrap_err();
        assert_eq!(e.path, "/dataModel");
        assert_eq!(state.data_model(), &json!({}));

        let mut state = create_with(
            tight,
            json!([
                {"id": "root", "component": "List", "children": {"componentId": "item", "path": "/items"}},
                {"id": "item", "component": "Divider"}
            ]),
            json!({"items": [1, 2, 3, 4]}),
        )
        .unwrap();
        let e = set_data(&mut state, "/items/-", json!(5)).unwrap_err();
        assert!(e.message.contains("nodes"), "{}", e.message);
    }

    #[test]
    fn checks_enable_the_button_only_when_they_pass() {
        let mut state = create(form(), json!({"form": {"email": "", "terms": false}}));
        assert!(!button_enabled(&state));
        assert!(
            state.action("submit", None, "t".into()).is_err(),
            "a disabled button sends nothing"
        );
        state.write("/form/email", json!("a@b")).unwrap();
        assert!(!button_enabled(&state));
        state.write("/form/terms", json!(true)).unwrap();
        assert!(button_enabled(&state));
        let root = tree(&state);
        let NodeKind::TextField {
            errors, binding, ..
        } = &find(&root, "email").kind
        else {
            panic!("expected a text field")
        };
        assert!(errors.is_empty());
        assert_eq!(binding.as_deref(), Some("/form/email"));
        state.write("/form/email", json!("nope")).unwrap();
        let root = tree(&state);
        let NodeKind::TextField { errors, .. } = &find(&root, "email").kind else {
            unreachable!()
        };
        assert_eq!(errors, &vec!["Enter an email.".to_owned()]);
    }

    #[test]
    fn action_context_resolves_bound_values_at_click_time() {
        let mut state = create(form(), json!({"form": {"email": "a@b.c", "terms": true}}));
        let action = state
            .action("submit", None, "2026-01-01T00:00:00Z".into())
            .unwrap();
        assert_eq!(action.name, "submit");
        assert_eq!(action.surface_id, "s");
        assert_eq!(action.source_component_id, "submit");
        assert_eq!(action.user_message.as_deref(), Some("a@b.c"));
        assert_eq!(
            action.context,
            json!({"email": "a@b.c", "terms": true, "formId": "f1"})
                .as_object()
                .unwrap()
                .clone()
        );
        state.write("/form/email", json!("x@y.z")).unwrap();
        let later = state.action("submit", None, "t".into()).unwrap();
        assert_eq!(later.context["email"], json!("x@y.z"));
        assert_eq!(
            later.to_message()["action"]["sourceComponentId"],
            json!("submit")
        );
        assert!(
            state.action("email", None, "t".into()).is_err(),
            "only buttons act"
        );
    }

    #[test]
    fn actions_inside_templates_resolve_relative_context() {
        let state = create(
            json!([
                {"id": "root", "component": "List", "children": {"componentId": "pick", "path": "/rows"}},
                {"id": "label", "component": "Text", "text": {"@path": "title"}},
                {"id": "pick", "component": "Button", "child": "label",
                 "action": {"event": {"name": "open", "context": {"rowId": {"@path": "id"}}}}}
            ]),
            json!({"rows": [{"id": 7, "title": "seven"}, {"id": 9, "title": "nine"}]}),
        );
        let button = find(&tree(&state), "pick@/rows/1").clone();
        let action = state
            .action(&button.component_id, button.scope.as_deref(), "t".into())
            .unwrap();
        assert_eq!(action.context["rowId"], json!(9));
    }

    #[test]
    fn choice_picker_and_checkbox_reflect_the_model() {
        let state = create(
            json!([
                {"id": "root", "component": "Column", "children": ["pick", "box"]},
                {"id": "pick", "component": "ChoicePicker", "variant": "multipleSelection",
                 "options": [{"label": "Email", "value": "email"}, {"label": "SMS", "value": "sms"}],
                 "value": {"@path": "/prefs"}},
                {"id": "box", "component": "CheckBox", "label": "Literal", "value": true}
            ]),
            json!({"prefs": ["sms"]}),
        );
        let root = tree(&state);
        let NodeKind::ChoicePicker {
            multiple,
            options,
            selected,
            binding,
            ..
        } = &find(&root, "pick").kind
        else {
            panic!("expected a choice picker")
        };
        assert!(*multiple);
        assert_eq!(options[0], ("Email".to_owned(), "email".to_owned()));
        assert_eq!(selected, &vec!["sms".to_owned()]);
        assert_eq!(binding.as_deref(), Some("/prefs"));
        let NodeKind::CheckBox {
            checked, binding, ..
        } = &find(&root, "box").kind
        else {
            panic!()
        };
        assert!(*checked);
        assert_eq!(binding, &None, "a literal value is read-only");
    }

    #[test]
    fn surface_identity_is_enforced() {
        let mut state = create(json!([{"id": "root", "component": "Divider"}]), json!({}));
        let e = state
            .apply(&message(json!({"version": "1.0", "createSurface": {"surfaceId": "s", "catalogId": BASIC_CATALOG_ID}})))
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::IntegrityError);
        assert_eq!(e.to_message()["error"]["surfaceId"], json!("s"));
        let e = state
            .apply(&message(
                json!({"version": "1.0", "deleteSurface": {"surfaceId": "other"}}),
            ))
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::IntegrityError);
        let deleted = state
            .apply(&message(
                json!({"version": "1.0", "deleteSurface": {"surfaceId": "s"}}),
            ))
            .unwrap();
        assert_eq!(deleted, Change::Deleted);
        assert_eq!(state.tree().unwrap(), None);
    }

    #[test]
    fn validation_errors_serialize_to_the_spec_shape() {
        let e = create_with(
            Limits::default(),
            json!([{"id": "root", "component": "Text", "text": 5}]),
            json!({}),
        )
        .map(|_| ())
        .err();
        assert!(e.is_none(), "a literal number is still a renderable value");
        let e = create_with(
            Limits::default(),
            json!([{"id": "root", "component": "Text"}]),
            json!({}),
        )
        .unwrap_err();
        assert_eq!(
            e.to_message(),
            json!({"version": "1.0", "error": {
                "code": "VALIDATION_FAILED", "surfaceId": "s", "path": "/components/0/text", "message": e.message
            }})
        );
    }

    #[test]
    fn timestamps_are_rfc3339_utc() {
        use std::time::{Duration, UNIX_EPOCH};
        let t = UNIX_EPOCH + Duration::from_secs(1_780_390_643);
        assert_eq!(crate::protocol::utc_timestamp(t), "2026-06-02T08:57:23Z");
        assert_eq!(
            crate::protocol::utc_timestamp(UNIX_EPOCH),
            "1970-01-01T00:00:00Z"
        );
    }
}
