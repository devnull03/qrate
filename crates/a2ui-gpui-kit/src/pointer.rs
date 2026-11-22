//! RFC 6901 JSON Pointers over the surface data model, with A2UI's upsert semantics.

use serde_json::{Map, Value};

/// The reference tokens of `pointer`. `""` and `"/"` both name the whole model, as A2UI's
/// `updateDataModel` treats them.
pub fn tokens(pointer: &str) -> Result<Vec<String>, String> {
    if pointer.is_empty() || pointer == "/" {
        return Ok(Vec::new());
    }
    let Some(rest) = pointer.strip_prefix('/') else {
        return Err(format!("JSON Pointer \"{pointer}\" must start with '/'."));
    };
    rest.split('/')
        .map(|raw| {
            let mut token = String::with_capacity(raw.len());
            let mut chars = raw.chars();
            while let Some(c) = chars.next() {
                if c != '~' {
                    token.push(c);
                    continue;
                }
                match chars.next() {
                    Some('0') => token.push('~'),
                    Some('1') => token.push('/'),
                    _ => return Err(format!("JSON Pointer \"{pointer}\" has a bad '~' escape.")),
                }
            }
            Ok(token)
        })
        .collect()
}

/// Resolves `path` in the scope `base`: absolute paths ignore it, relative ones append to it.
pub fn resolve(base: Option<&str>, path: &str) -> String {
    if path.starts_with('/') {
        return path.to_owned();
    }
    match base {
        None => format!("/{path}"),
        Some(base) if path.is_empty() => base.to_owned(),
        Some(base) => format!("{}/{path}", base.trim_end_matches('/')),
    }
}

pub fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

pub fn get<'a>(model: &'a Value, pointer: &str) -> Option<&'a Value> {
    tokens(pointer)
        .ok()?
        .iter()
        .try_fold(model, |value, token| match value {
            Value::Object(map) => map.get(token),
            Value::Array(items) => items.get(token.parse::<usize>().ok()?),
            _ => None,
        })
}

/// Upserts `value` at `pointer`: missing objects along the way are created, `null` removes the
/// key (or array element), and an array index one past the end (or `-`) appends.
pub fn set(model: &mut Value, pointer: &str, value: Value) -> Result<(), String> {
    let tokens = tokens(pointer)?;
    let Some((last, parents)) = tokens.split_last() else {
        *model = match value {
            Value::Null => Value::Object(Map::new()),
            Value::Object(_) => value,
            _ => return Err("The data model root must be an object.".into()),
        };
        return Ok(());
    };
    let mut current = model;
    for token in parents {
        if current.is_null() {
            *current = Value::Object(Map::new());
        }
        current = match current {
            Value::Object(map) => map
                .entry(token.clone())
                .or_insert_with(|| Value::Object(Map::new())),
            Value::Array(items) => {
                let index = array_index(token, items.len())?;
                if index == items.len() {
                    items.push(Value::Object(Map::new()));
                }
                &mut items[index]
            }
            _ => {
                return Err(format!(
                    "\"{pointer}\" passes through a non-container value."
                ));
            }
        };
    }
    if current.is_null() && !value.is_null() {
        *current = Value::Object(Map::new());
    }
    match current {
        Value::Object(map) if value.is_null() => {
            map.remove(last);
        }
        Value::Object(map) => {
            map.insert(last.clone(), value);
        }
        Value::Array(items) => {
            let index = array_index(last, items.len())?;
            match (index < items.len(), value.is_null()) {
                (true, true) => {
                    items.remove(index);
                }
                (true, false) => items[index] = value,
                (false, true) => {}
                (false, false) => items.push(value),
            }
        }
        Value::Null => {}
        _ => {
            return Err(format!(
                "\"{pointer}\" passes through a non-container value."
            ));
        }
    }
    Ok(())
}

fn array_index(token: &str, len: usize) -> Result<usize, String> {
    let index = match token {
        "-" => len,
        _ => token
            .parse::<usize>()
            .map_err(|_| format!("\"{token}\" is not an array index."))?,
    };
    match index <= len {
        true => Ok(index),
        false => Err(format!(
            "Array index {index} is past the end ({len} items)."
        )),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{get, resolve, set, tokens};

    #[test]
    fn tokens_unescape_and_treat_slash_as_root() {
        assert_eq!(tokens("/").unwrap(), Vec::<String>::new());
        assert_eq!(tokens("/a~1b/c~0d").unwrap(), vec!["a/b", "c~d"]);
        assert!(tokens("a/b").is_err());
        assert!(tokens("/a~2").is_err());
    }

    #[test]
    fn set_upserts_missing_objects_and_null_removes() {
        let mut model = json!({});
        set(&mut model, "/user/name", json!("Ada")).unwrap();
        assert_eq!(model, json!({"user": {"name": "Ada"}}));
        set(&mut model, "/user/name", json!("Grace")).unwrap();
        assert_eq!(get(&model, "/user/name"), Some(&json!("Grace")));
        set(&mut model, "/user/name", json!(null)).unwrap();
        assert_eq!(model, json!({"user": {}}));
    }

    #[test]
    fn set_appends_to_arrays_and_rejects_gaps() {
        let mut model = json!({"rows": [1]});
        set(&mut model, "/rows/1", json!(2)).unwrap();
        set(&mut model, "/rows/-", json!(3)).unwrap();
        assert_eq!(model, json!({"rows": [1, 2, 3]}));
        assert!(set(&mut model, "/rows/9", json!(4)).is_err());
        set(&mut model, "/rows/0", json!(null)).unwrap();
        assert_eq!(model, json!({"rows": [2, 3]}));
    }

    #[test]
    fn set_refuses_to_clobber_scalars_and_non_object_roots() {
        let mut model = json!({"name": "Ada"});
        assert!(set(&mut model, "/name/first", json!("x")).is_err());
        assert!(set(&mut model, "/", json!([1])).is_err());
        set(&mut model, "/", json!({"fresh": true})).unwrap();
        assert_eq!(model, json!({"fresh": true}));
    }

    #[test]
    fn relative_paths_resolve_against_the_scope() {
        assert_eq!(resolve(Some("/rows/2"), "name"), "/rows/2/name");
        assert_eq!(resolve(Some("/rows/2"), "/title"), "/title");
        assert_eq!(resolve(None, "name"), "/name");
        assert_eq!(resolve(Some("/rows/2"), ""), "/rows/2");
    }
}
