//! Typed components of the supported basic-catalog subset, parsed from the flat wire list.

use serde_json::{Map, Value};

use crate::checks::{CheckRule, Dynamic};
use crate::error::A2uiError;
use crate::pointer;
use crate::protocol::BASIC_CATALOG_ID;

#[derive(Debug, Clone, PartialEq)]
pub enum ChildList {
    Static(Vec<String>),
    /// One `component_id` instance per item of the array at `path`.
    Template {
        component_id: String,
        path: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Justify {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Default,
    Primary,
    Borderless,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFieldVariant {
    ShortText,
    LongText,
    Number,
    Obscured,
}

/// A `Button`'s `action.event`. Local `functionCall` actions are not supported.
#[derive(Debug, Clone)]
pub struct EventAction {
    pub name: String,
    pub user_message: Option<Dynamic>,
    pub context: Vec<(String, Dynamic)>,
}

#[derive(Debug, Clone)]
pub struct ChoiceOption {
    pub label: Dynamic,
    pub value: String,
}

#[derive(Debug, Clone)]
pub enum ComponentKind {
    Text {
        text: Dynamic,
        caption: bool,
    },
    Row {
        children: ChildList,
        justify: Justify,
        align: Align,
    },
    Column {
        children: ChildList,
        justify: Justify,
        align: Align,
    },
    List {
        children: ChildList,
        horizontal: bool,
        align: Align,
    },
    Card {
        child: String,
    },
    Divider {
        vertical: bool,
    },
    TextField {
        label: Dynamic,
        value: Dynamic,
        placeholder: Option<Dynamic>,
        variant: TextFieldVariant,
    },
    CheckBox {
        label: Dynamic,
        value: Dynamic,
    },
    ChoicePicker {
        label: Option<Dynamic>,
        multiple: bool,
        options: Vec<ChoiceOption>,
        value: Dynamic,
    },
    Button {
        child: String,
        variant: ButtonVariant,
        action: EventAction,
    },
}

#[derive(Debug, Clone)]
pub struct Component {
    pub id: String,
    pub kind: ComponentKind,
    pub checks: Vec<CheckRule>,
    pub weight: Option<f32>,
    pub accessibility_label: Option<Dynamic>,
}

impl ComponentKind {
    /// Child references in declaration order; a template counts as one reference.
    pub fn child_ids(&self) -> Vec<&str> {
        match self {
            Self::Row { children, .. }
            | Self::Column { children, .. }
            | Self::List { children, .. } => match children {
                ChildList::Static(ids) => ids.iter().map(String::as_str).collect(),
                ChildList::Template { component_id, .. } => vec![component_id.as_str()],
            },
            Self::Card { child } | Self::Button { child, .. } => vec![child.as_str()],
            _ => Vec::new(),
        }
    }
}

/// The component properties still unread, so anything the schema does not define is an error.
struct Props<'a> {
    map: Map<String, Value>,
    path: &'a str,
    kind: &'a str,
}

impl Props<'_> {
    fn at(&self, key: &str) -> String {
        format!("{}/{}", self.path, pointer::escape(key))
    }

    fn missing(&self, key: &str) -> A2uiError {
        A2uiError::validation(self.at(key), format!("{} requires \"{key}\".", self.kind))
    }

    fn dynamic(&mut self, key: &str) -> Result<Option<Dynamic>, A2uiError> {
        let at = self.at(key);
        self.map
            .remove(key)
            .map(|v| Dynamic::parse(&v, &at))
            .transpose()
    }

    fn required(&mut self, key: &str) -> Result<Dynamic, A2uiError> {
        self.dynamic(key)?.ok_or_else(|| self.missing(key))
    }

    fn id(&mut self, key: &str) -> Result<String, A2uiError> {
        match self.map.remove(key) {
            Some(Value::String(id)) => Ok(id),
            Some(_) => Err(A2uiError::validation(
                self.at(key),
                "Expected a component id.",
            )),
            None => Err(self.missing(key)),
        }
    }

    fn choice<T: Copy>(
        &mut self,
        key: &str,
        choices: &[(&str, T)],
        default: T,
    ) -> Result<T, A2uiError> {
        let Some(value) = self.map.remove(key) else {
            return Ok(default);
        };
        choices
            .iter()
            .find(|(name, _)| value.as_str() == Some(name))
            .map(|(_, v)| *v)
            .ok_or_else(|| {
                let names: Vec<_> = choices.iter().map(|(n, _)| *n).collect();
                A2uiError::validation(
                    self.at(key),
                    format!("Expected one of {}.", names.join(", ")),
                )
            })
    }

    fn children(&mut self) -> Result<ChildList, A2uiError> {
        let at = self.at("children");
        match self.map.remove("children") {
            Some(Value::Array(ids)) => ids
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    id.as_str().map(str::to_owned).ok_or_else(|| {
                        A2uiError::validation(format!("{at}/{i}"), "Expected a component id.")
                    })
                })
                .collect::<Result<_, _>>()
                .map(ChildList::Static),
            Some(Value::Object(template)) => {
                let field =
                    |key: &str| template.get(key).and_then(Value::as_str).map(str::to_owned);
                match (field("componentId"), field("path"), template.len()) {
                    (Some(component_id), Some(path), 2) => {
                        Ok(ChildList::Template { component_id, path })
                    }
                    _ => Err(A2uiError::validation(
                        at,
                        "A child template is exactly {\"componentId\", \"path\"}.",
                    )),
                }
            }
            Some(_) => Err(A2uiError::validation(
                at,
                "children must be an id array or a template.",
            )),
            None => Err(self.missing("children")),
        }
    }

    fn justify(&mut self) -> Result<Justify, A2uiError> {
        use Justify::*;
        self.choice(
            "justify",
            &[
                ("start", Start),
                ("center", Center),
                ("end", End),
                ("spaceBetween", SpaceBetween),
                ("spaceAround", SpaceAround),
                ("spaceEvenly", SpaceEvenly),
                ("stretch", Stretch),
            ],
            Start,
        )
    }

    fn align(&mut self) -> Result<Align, A2uiError> {
        use Align::*;
        self.choice(
            "align",
            &[
                ("start", Start),
                ("center", Center),
                ("end", End),
                ("stretch", Stretch),
            ],
            Stretch,
        )
    }

    fn options(&mut self) -> Result<Vec<ChoiceOption>, A2uiError> {
        let at = self.at("options");
        let Some(Value::Array(items)) = self.map.remove("options") else {
            return Err(A2uiError::validation(
                at,
                "ChoicePicker requires an options array.",
            ));
        };
        items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let item_at = format!("{at}/{i}");
                match (
                    item.get("label"),
                    item.get("value").and_then(Value::as_str),
                    item.as_object().map(Map::len),
                ) {
                    (Some(label), Some(value), Some(2)) => Ok(ChoiceOption {
                        label: Dynamic::parse(label, &format!("{item_at}/label"))?,
                        value: value.to_owned(),
                    }),
                    _ => Err(A2uiError::validation(
                        item_at,
                        "An option is exactly {\"label\", \"value\"}.",
                    )),
                }
            })
            .collect()
    }

    fn action(&mut self) -> Result<EventAction, A2uiError> {
        let at = self.at("action");
        let event = match self.map.remove("action") {
            Some(Value::Object(mut action)) if action.len() == 1 => match action.remove("event") {
                Some(Value::Object(event)) => event,
                _ => {
                    return Err(A2uiError::validation(
                        at,
                        "Only event actions are supported; local function actions are not.",
                    ));
                }
            },
            None => return Err(self.missing("action")),
            Some(_) => {
                return Err(A2uiError::validation(
                    at,
                    "An action is exactly {\"event\": {...}}.",
                ));
            }
        };
        let at = format!("{at}/event");
        if let Some(key) = event
            .keys()
            .find(|k| !["name", "userMessage", "context"].contains(&k.as_str()))
        {
            return Err(A2uiError::validation(
                format!("{at}/{}", pointer::escape(key)),
                format!("Unexpected property \"{key}\" on an action event."),
            ));
        }
        let name = event
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                A2uiError::validation(format!("{at}/name"), "An action event needs a name.")
            })?
            .to_owned();
        let user_message = event
            .get("userMessage")
            .map(|v| Dynamic::parse(v, &format!("{at}/userMessage")))
            .transpose()?;
        let context = match event.get("context") {
            None => Vec::new(),
            Some(Value::Object(context)) => context
                .iter()
                .map(|(k, v)| {
                    Dynamic::parse(v, &format!("{at}/context/{}", pointer::escape(k)))
                        .map(|d| (k.clone(), d))
                })
                .collect::<Result<_, _>>()?,
            Some(_) => {
                return Err(A2uiError::validation(
                    format!("{at}/context"),
                    "context must be an object.",
                ));
            }
        };
        Ok(EventAction {
            name,
            user_message,
            context,
        })
    }
}

impl Component {
    /// Parses one entry of a `components` list; `path` locates it in the message (`/components/3`).
    pub fn parse(value: &Value, path: &str) -> Result<Self, A2uiError> {
        let Value::Object(map) = value else {
            return Err(A2uiError::validation(
                path,
                "A component must be an object.",
            ));
        };
        let mut map = map.clone();
        let id = match map.remove("id") {
            Some(Value::String(id)) if is_identifier(&id) => id,
            Some(Value::String(id)) => {
                return Err(A2uiError::validation(
                    format!("{path}/id"),
                    format!(
                        "\"{id}\" is not a valid component id (letters, digits and '_', not starting with a digit)."
                    ),
                ));
            }
            _ => {
                return Err(A2uiError::validation(
                    format!("{path}/id"),
                    "A component needs a string id.",
                ));
            }
        };
        let kind_name = match map.remove("component") {
            Some(Value::String(kind)) => kind,
            _ => {
                return Err(A2uiError::validation(
                    format!("{path}/component"),
                    "A component needs a type.",
                ));
            }
        };
        let mut props = Props {
            map,
            path,
            kind: &kind_name,
        };
        match props.map.remove("@catalogId") {
            None => {}
            Some(Value::String(catalog)) if catalog == BASIC_CATALOG_ID => {}
            Some(_) => {
                return Err(A2uiError::validation(
                    props.at("@catalogId"),
                    "Only the basic catalog is supported.",
                ));
            }
        }
        props.map.remove("@metadata");
        let accessibility_label = match props.map.remove("@accessibility") {
            Some(Value::Object(attrs)) => attrs
                .get("label")
                .map(|label| Dynamic::parse(label, &format!("{path}/@accessibility/label")))
                .transpose()?,
            Some(_) => {
                return Err(A2uiError::validation(
                    props.at("@accessibility"),
                    "Expected an object.",
                ));
            }
            None => None,
        };
        let weight = match props.map.remove("weight") {
            Some(w) => Some(
                w.as_f64()
                    .filter(|w| w.is_finite() && *w >= 0.0)
                    .ok_or_else(|| {
                        A2uiError::validation(
                            props.at("weight"),
                            "weight must be a non-negative number.",
                        )
                    })? as f32,
            ),
            None => None,
        };
        let checkable = matches!(
            kind_name.as_str(),
            "TextField" | "CheckBox" | "ChoicePicker" | "Button"
        );
        let checks = match props.map.remove("@checks") {
            Some(Value::Array(rules)) if checkable => rules
                .iter()
                .enumerate()
                .map(|(i, rule)| CheckRule::parse(rule, &format!("{path}/@checks/{i}")))
                .collect::<Result<_, _>>()?,
            Some(_) => {
                return Err(A2uiError::validation(
                    props.at("@checks"),
                    format!("{kind_name} does not take @checks."),
                ));
            }
            None => Vec::new(),
        };
        let kind = match kind_name.as_str() {
            "Text" => ComponentKind::Text {
                text: props.required("text")?,
                caption: props.choice("variant", &[("body", false), ("caption", true)], false)?,
            },
            "Row" => ComponentKind::Row {
                children: props.children()?,
                justify: props.justify()?,
                align: props.align()?,
            },
            "Column" => ComponentKind::Column {
                children: props.children()?,
                justify: props.justify()?,
                align: props.align()?,
            },
            "List" => ComponentKind::List {
                children: props.children()?,
                horizontal: props.choice(
                    "direction",
                    &[("vertical", false), ("horizontal", true)],
                    false,
                )?,
                align: props.align()?,
            },
            "Card" => ComponentKind::Card {
                child: props.id("child")?,
            },
            "Divider" => ComponentKind::Divider {
                vertical: props.choice(
                    "axis",
                    &[("horizontal", false), ("vertical", true)],
                    false,
                )?,
            },
            "TextField" => {
                use TextFieldVariant::*;
                ComponentKind::TextField {
                    label: props.required("label")?,
                    value: props.required("value")?,
                    placeholder: props.dynamic("placeholder")?,
                    variant: props.choice(
                        "variant",
                        &[
                            ("shortText", ShortText),
                            ("longText", LongText),
                            ("number", Number),
                            ("obscured", Obscured),
                        ],
                        ShortText,
                    )?,
                }
            }
            "CheckBox" => ComponentKind::CheckBox {
                label: props.required("label")?,
                value: props.required("value")?,
            },
            "ChoicePicker" => {
                props.choice("displayStyle", &[("checkbox", ()), ("chips", ())], ())?;
                if props
                    .map
                    .remove("filterable")
                    .is_some_and(|f| !f.is_boolean())
                {
                    return Err(A2uiError::validation(
                        props.at("filterable"),
                        "filterable must be a boolean.",
                    ));
                }
                ComponentKind::ChoicePicker {
                    label: props.dynamic("label")?,
                    multiple: props.choice(
                        "variant",
                        &[("mutuallyExclusive", false), ("multipleSelection", true)],
                        false,
                    )?,
                    options: props.options()?,
                    value: props.required("value")?,
                }
            }
            "Button" => {
                use ButtonVariant::*;
                ComponentKind::Button {
                    child: props.id("child")?,
                    variant: props.choice(
                        "variant",
                        &[
                            ("default", Default),
                            ("primary", Primary),
                            ("borderless", Borderless),
                        ],
                        Default,
                    )?,
                    action: props.action()?,
                }
            }
            other => {
                return Err(A2uiError::validation(
                    format!("{path}/component"),
                    format!("Component \"{other}\" is not supported by this renderer."),
                ));
            }
        };
        if let Some(key) = props.map.keys().next() {
            return Err(A2uiError::validation(
                props.at(key),
                format!("{kind_name} has no property \"{key}\"."),
            ));
        }
        Ok(Self {
            id,
            kind,
            checks,
            weight,
            accessibility_label,
        })
    }
}

/// UAX #31 identifier: a letter or `_`, then letters, digits or `_`.
fn is_identifier(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}
