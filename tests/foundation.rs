//! Repository-wide toolchain, task-scope, and cache contracts.
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;
    use toml::{Table, Value};

    fn config(path: &str) -> Table {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        toml::from_str(&fs::read_to_string(root.join(path)).unwrap()).unwrap()
    }

    fn table<'a>(value: &'a Table, key: &str) -> &'a Table {
        value.get(key).and_then(Value::as_table).unwrap()
    }

    fn members() -> Vec<String> {
        table(&config("Cargo.toml"), "workspace")["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn tool_selection_is_exact_and_rust_has_one_version() {
        let mise = config("mise.toml");
        let tools = table(&mise, "tools");
        for (name, entry) in tools {
            let version = entry
                .as_str()
                .or_else(|| entry.get("version").and_then(Value::as_str))
                .unwrap();
            let parts: Vec<_> = version.split('.').collect();
            assert_eq!(parts.len(), 3, "{name}: declare an exact SDK version");
            assert!(
                parts
                    .iter()
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            );
        }
        let rust = tools.get("rust").and_then(Value::as_table).unwrap();
        let native = config("rust-toolchain.toml");
        let native = table(&native, "toolchain");
        assert_eq!(native.get("channel"), rust.get("version"));
        assert_eq!(native.get("profile"), rust.get("profile"));
        let components: BTreeSet<_> = native["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let selected: BTreeSet<_> = rust["components"].as_str().unwrap().split(',').collect();
        assert_eq!(components, selected);
        assert_eq!(
            table(&mise, "settings").get("locked"),
            Some(&Value::Boolean(true))
        );
        assert_eq!(
            table(&mise, "env")
                .get("MISE_TASK_CACHE")
                .and_then(Value::as_str),
            Some("local-only")
        );
    }

    #[test]
    fn only_the_pi_distributable_has_an_outer_artifact_cache() {
        let mut paths: BTreeSet<_> = members()
            .into_iter()
            .map(|p| format!("{p}/mise.toml"))
            .collect();
        paths.extend(["mise.toml", "src/herdr/mise.toml"].map(str::to_owned));
        let mise = config("mise.toml");
        paths.extend(
            table(table(&mise, "monorepo"), "projects")
                .values()
                .filter_map(|project| project.get("root").and_then(Value::as_str))
                .map(|root| format!("{root}/mise.toml")),
        );
        let mut cached = Vec::new();
        for path in paths {
            let config = config(&path);
            for (name, task) in table(&config, "tasks") {
                let task = task.as_table().unwrap();
                if task
                    .get("cache")
                    .and_then(|v| v.get("enabled"))
                    .and_then(Value::as_bool)
                    == Some(true)
                {
                    assert!(
                        task.get("sources").is_some(),
                        "{path}:{name}: cache sources"
                    );
                    assert!(
                        task.get("outputs").is_some(),
                        "{path}:{name}: cache outputs"
                    );
                    cached.push((path.clone(), name.clone()));
                }
            }
        }
        assert_eq!(
            cached,
            [("src/pi/mise.toml".to_owned(), "package".to_owned())]
        );
    }

    #[test]
    fn rust_components_have_scoped_checks_and_native_staging() {
        for member in members() {
            let manifest = config(&format!("{member}/Cargo.toml"));
            let config = config(&format!("{member}/mise.toml"));
            let tasks = table(&config, "tasks");
            let check = table(tasks, "check");
            assert_eq!(
                check.get("extends").and_then(Value::as_str),
                Some("rust-check")
            );
            assert_eq!(
                table(check, "vars").get("package"),
                table(&manifest, "package").get("name")
            );
            if let Some(dependencies) = check.get("depends").and_then(Value::as_array) {
                assert!(
                    !dependencies
                        .iter()
                        .any(|v| v.as_str() == Some("//:rust-check"))
                );
            }
            if let Some(build) = tasks.get("build") {
                let command = build.get("run").and_then(Value::as_str).unwrap();
                // A Herdr plugin stages into its own directory, which
                // `herdr plugin link` registers in place.
                let plugin = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .join(&member)
                    .join("herdr-plugin.toml")
                    .is_file();
                if plugin {
                    assert_eq!(command, "cargo install --locked --path . --root . --force");
                } else {
                    assert!(command.starts_with("cargo install --frozen --path . "));
                    assert!(command.contains("$MISE_MONOREPO_ROOT/dist"));
                }
                assert!(!command.contains("--no-track"));
            }
        }
    }
}
