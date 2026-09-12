//! Per-container settings, keyed by the exact container name.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "UPPERCASE")]
pub struct ShellConfig {
    pub user: String,
    pub shell: String,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            user: "root".to_owned(),
            shell: "bash".to_owned(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(transparent)]
pub struct Config(BTreeMap<String, ShellConfig>);

impl Config {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let config: Self = toml::from_str(text)?;
        for (name, settings) in &config.0 {
            anyhow::ensure!(!name.trim().is_empty(), "container name must not be empty");
            for (key, value) in [("USER", &settings.user), ("SHELL", &settings.shell)] {
                anyhow::ensure!(
                    !value.trim().is_empty() && !value.contains('\0'),
                    "{name}: {key} must be nonempty and contain no NUL characters"
                );
            }
        }
        Ok(config)
    }

    /// An absent default file is fine; an explicitly requested file must exist.
    pub fn load(explicit: Option<&Path>) -> anyhow::Result<Self> {
        let path = explicit.map(Path::to_path_buf).or_else(default_path);
        let Some(path) = path else {
            return Ok(Self::default());
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text)
                .with_context(|| format!("invalid configuration in {}", path.display())),
            Err(err) if explicit.is_none() && err.kind() == std::io::ErrorKind::NotFound => {
                Ok(Self::default())
            }
            Err(err) => Err(err).with_context(|| format!("could not read {}", path.display())),
        }
    }

    pub fn for_container(&self, name: &str) -> ShellConfig {
        self.0
            .get(name.trim_start_matches('/'))
            .cloned()
            .unwrap_or_default()
    }
}

fn default_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
        .map(|base| base.join("jikura/config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_exact_names_and_defaults_missing_fields() {
        let config = Config::parse(
            r#"
            [web]
            USER = "app"
            ["db.prod"]
            SHELL = "/bin/sh"
        "#,
        )
        .unwrap();
        assert_eq!(
            config.for_container("/web"),
            ShellConfig {
                user: "app".into(),
                shell: "bash".into()
            }
        );
        assert_eq!(
            config.for_container("db.prod"),
            ShellConfig {
                user: "root".into(),
                shell: "/bin/sh".into()
            }
        );
        assert_eq!(config.for_container("web-other"), ShellConfig::default());
    }

    #[test]
    fn rejects_malformed_unknown_duplicate_and_empty_settings() {
        for text in [
            "[broken",
            "[web]\nUSRE = 'app'",
            "[web]\nUSER = 'a'\nUSER = 'b'",
            "[web]\nSHELL = ' '",
            "[web]\nUSER = ''",
        ] {
            assert!(Config::parse(text).is_err(), "accepted {text}");
        }
    }

    #[test]
    fn an_explicit_missing_file_is_an_error() {
        // A path beneath a file cannot exist, independently of host config.
        assert!(Config::load(Some(Path::new("Cargo.toml/missing-config.toml"))).is_err());
    }
}
