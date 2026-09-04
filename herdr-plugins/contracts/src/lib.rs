//! Cross-plugin manifest contracts.
//!
//! This crate intentionally contains only integration tests. It keeps build
//! and dispatch conventions owned by the workspace instead of making any one
//! plugin responsible for policing its siblings.

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::{Path, PathBuf};

    use anyhow::{Context as _, Result, ensure};
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Manifest {
        id: String,
        build: Vec<Command>,
        #[serde(default)]
        startup: Vec<Command>,
        #[serde(default)]
        actions: Vec<Entrypoint>,
        #[serde(default)]
        panes: Vec<Entrypoint>,
    }

    #[derive(Debug, Deserialize)]
    struct Command {
        command: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct Entrypoint {
        id: String,
        command: Vec<String>,
    }

    #[derive(Debug, Deserialize)]
    struct HerdrConfig {
        #[serde(default)]
        keys: KeyBindings,
    }

    #[derive(Debug, Default, Deserialize)]
    struct KeyBindings {
        #[serde(default)]
        command: Vec<KeyCommand>,
    }

    #[derive(Debug, Deserialize)]
    struct KeyCommand {
        #[serde(rename = "type")]
        kind: String,
        command: String,
    }

    fn workspace() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }

    fn manifest(plugin: &str) -> Result<Manifest> {
        let path = workspace().join(plugin).join("herdr-plugin.toml");
        Ok(toml::from_str(&fs::read_to_string(path)?)?)
    }

    fn ids(entries: &[Entrypoint]) -> BTreeSet<&str> {
        entries.iter().map(|entry| entry.id.as_str()).collect()
    }

    fn herdr_config() -> Result<HerdrConfig> {
        let path = workspace()
            .parent()
            .context("plugin workspace has no repository root")?
            .join("herdr/.config/herdr/config.toml");
        Ok(toml::from_str(&fs::read_to_string(path)?)?)
    }

    fn argument_after<'a>(command: &'a str, flag: &str) -> Option<&'a str> {
        let mut words = command.split_whitespace();
        while let Some(word) = words.next() {
            if word == flag {
                return words.next();
            }
        }
        None
    }

    #[test]
    fn every_plugin_uses_its_own_make_build_primitive() -> Result<()> {
        for plugin in ["find", "github", "nav", "nvim"] {
            let manifest = manifest(plugin)?;
            ensure!(
                manifest.id == format!("herdr-{plugin}"),
                "{plugin} manifest id is inconsistent"
            );
            ensure!(
                manifest.build.len() == 1,
                "{plugin} must have exactly one build primitive"
            );
            let expected = vec![
                "make".to_owned(),
                "-C".to_owned(),
                "../..".to_owned(),
                format!("plugin-build-{plugin}"),
            ];
            ensure!(
                manifest.build.first().map(|build| build.command.as_slice())
                    == Some(expected.as_slice()),
                "{plugin} does not use its per-plugin Make target"
            );
        }
        Ok(())
    }

    #[test]
    fn herdr_identity_is_not_duplicated_in_plugin_argv() -> Result<()> {
        for plugin in ["find", "github", "nav", "nvim"] {
            let manifest = manifest(plugin)?;
            let binary = format!("./bin/herdr-{plugin}");
            for entry in manifest.actions.iter().chain(&manifest.panes) {
                if entry.command.first() == Some(&binary) {
                    ensure!(
                        entry.command == [binary.as_str()],
                        "{} entrypoint {} duplicates its Herdr identity in argv",
                        manifest.id,
                        entry.id
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn manifest_entrypoint_ids_match_binary_dispatch_contracts() -> Result<()> {
        let find = manifest("find")?;
        ensure!(ids(&find.actions).is_empty(), "find declares actions");
        ensure!(
            ids(&find.panes) == BTreeSet::from(["picker"]),
            "find pane dispatch drifted"
        );

        let nav = manifest("nav")?;
        ensure!(
            ids(&nav.actions) == BTreeSet::from(["down", "left", "right", "up"]),
            "nav action dispatch drifted"
        );
        ensure!(ids(&nav.panes).is_empty(), "nav declares panes");

        let nvim = manifest("nvim")?;
        ensure!(
            ids(&nvim.actions) == BTreeSet::from(["open"]),
            "nvim action dispatch drifted"
        );
        ensure!(
            ids(&nvim.panes) == BTreeSet::from(["picker"]),
            "nvim pane dispatch drifted"
        );

        let github = manifest("github")?;
        ensure!(
            ids(&github.actions) == BTreeSet::from(["ci-fix", "ci-refresh", "open-link"]),
            "GitHub action dispatch drifted"
        );
        ensure!(
            ids(&github.panes) == BTreeSet::from(["ci-arm", "ci-brief", "dashboard"]),
            "GitHub pane dispatch drifted"
        );
        Ok(())
    }

    #[test]
    fn one_shot_startup_hooks_do_not_host_daemons() -> Result<()> {
        for plugin in ["find", "github", "nav", "nvim"] {
            ensure!(
                manifest(plugin)?.startup.is_empty(),
                "{plugin} declares an unsupported resident startup command"
            );
        }
        Ok(())
    }

    #[test]
    fn herdr_keybindings_reference_declared_plugin_entrypoints() -> Result<()> {
        let manifests = ["find", "github", "nav", "nvim"]
            .into_iter()
            .map(manifest)
            .collect::<Result<Vec<_>>>()?;
        let actions = manifests
            .iter()
            .map(|manifest| (manifest.id.as_str(), ids(&manifest.actions)))
            .collect::<BTreeMap<_, _>>();
        let panes = manifests
            .iter()
            .map(|manifest| (manifest.id.as_str(), ids(&manifest.panes)))
            .collect::<BTreeMap<_, _>>();

        for binding in herdr_config()?.keys.command {
            if binding.kind == "plugin_action" {
                let (plugin, action) = binding
                    .command
                    .split_once('.')
                    .with_context(|| format!("invalid plugin action {}", binding.command))?;
                let declared = actions
                    .get(plugin)
                    .with_context(|| format!("keybinding references unknown plugin {plugin}"))?;
                ensure!(
                    declared.contains(action),
                    "keybinding references undeclared action {}",
                    binding.command
                );
            } else if binding.kind == "shell" && binding.command.contains(" plugin pane open ") {
                let plugin = argument_after(&binding.command, "--plugin").with_context(|| {
                    format!("plugin pane command has no --plugin: {}", binding.command)
                })?;
                let entrypoint =
                    argument_after(&binding.command, "--entrypoint").with_context(|| {
                        format!(
                            "plugin pane command has no --entrypoint: {}",
                            binding.command
                        )
                    })?;
                let declared = panes
                    .get(plugin)
                    .with_context(|| format!("keybinding references unknown plugin {plugin}"))?;
                ensure!(
                    declared.contains(entrypoint),
                    "keybinding references undeclared pane {plugin}.{entrypoint}"
                );
            }
        }
        Ok(())
    }
}
