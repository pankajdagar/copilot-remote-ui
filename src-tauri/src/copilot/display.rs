use serde_json::Value;

pub(super) struct DisplayArguments {
    pub fields: Vec<(String, String)>,
    pub complete: bool,
}

fn sensitive_argument(name: &str) -> bool {
    let name: String = name
        .to_ascii_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    name == "env"
        || name == "environment"
        || [
            "token",
            "secret",
            "password",
            "passphrase",
            "credential",
            "auth",
            "authorization",
            "cookie",
            "privatekey",
            "apikey",
            "accesskey",
        ]
        .iter()
        .any(|part| name.contains(part))
}

fn reviewable_value(value: &Value, depth: usize) -> Option<(Value, bool)> {
    if depth > 6 {
        return None;
    }
    match value {
        Value::Object(object) => {
            if object.len() > 32 {
                return None;
            }
            let mut next = serde_json::Map::new();
            let mut complete = true;
            for (name, entry) in object {
                if sensitive_argument(name) {
                    next.insert(name.clone(), Value::String("[redacted]".into()));
                    complete = false;
                } else {
                    let (safe, visible) = reviewable_value(entry, depth + 1)?;
                    next.insert(name.clone(), safe);
                    complete &= visible;
                }
            }
            Some((Value::Object(next), complete))
        }
        Value::Array(items) => {
            if items.len() > 64 {
                return None;
            }
            let mut safe = Vec::with_capacity(items.len());
            let mut complete = true;
            for item in items {
                let (value, visible) = reviewable_value(item, depth + 1)?;
                safe.push(value);
                complete &= visible;
            }
            Some((Value::Array(safe), complete))
        }
        Value::String(value) => {
            (value.chars().count() <= 4096).then(|| (value.clone().into(), true))
        }
        _ => Some((value.clone(), true)),
    }
}

pub(super) fn argument_details(arguments: &Value) -> Option<DisplayArguments> {
    let map = arguments.as_object()?;
    if map.len() > 12 {
        return None;
    }
    let mut total = 0;
    let mut complete = true;
    let fields = map
        .iter()
        .map(|(label, value)| {
            let (safe, visible) = reviewable_value(value, 0)?;
            complete &= visible;
            let text = match safe {
                Value::String(text) => text,
                other => serde_json::to_string_pretty(&other).ok()?,
            };
            total += text.len();
            if total > 12_000 {
                return None;
            }
            Some((label.clone(), text))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(DisplayArguments { fields, complete })
}
