//! Preserve the requested output mode on generated follow-up commands.
use serde_json::Value;

pub(crate) fn render(mut value: Value) -> String {
    commands(&mut value);
    format!("{value:#}\n")
}

fn commands(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(commands),
        Value::Object(fields) => {
            for (key, value) in fields {
                if (key == "command" || key.ends_with("_command"))
                    && let Value::String(command) = value
                    && let Some(rest) = command.strip_prefix("dots ")
                    && !rest.starts_with("--json ")
                {
                    *command = format!("dots --json {rest}");
                } else {
                    commands(value);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_generated_dots_command_fields_change() {
        let input = serde_json::json!({
            "body": "dots github ci --search --json",
            "warnings": ["dots github ci"],
            "next_command": "dots github ci logs REF --search '--json'",
            "matches": [{"command": "dots github ci output REF --snapshot SAVED"}],
            "full_log_command": "gh run view --log",
            "previous_command": null
        });
        let output: Value = serde_json::from_str(&render(input)).unwrap();
        assert_eq!(
            output.get("next_command").unwrap(),
            "dots --json github ci logs REF --search '--json'"
        );
        assert_eq!(
            output.pointer("/matches/0/command").unwrap(),
            "dots --json github ci output REF --snapshot SAVED"
        );
        assert_eq!(
            output.get("body").unwrap(),
            "dots github ci --search --json"
        );
        assert_eq!(output.get("full_log_command").unwrap(), "gh run view --log");
        assert_eq!(
            render(output.clone()),
            render(serde_json::from_str(&render(output)).unwrap())
        );
    }
}
