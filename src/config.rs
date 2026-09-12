//! Application settings and per-container shell settings.

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
#[serde(default, deny_unknown_fields, rename_all = "UPPERCASE")]
pub struct Settings {
    #[serde(alias = "GITLAB_IAMGE_API")]
    pub gitlab_image_api: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub settings: Settings,
    #[serde(rename = "CONTAINERS")]
    containers: BTreeMap<String, ShellConfig>,
}

impl Config {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let value: toml::Value = toml::from_str(text)?;
        // Keep existing flat container files working. A structured file must
        // put every container under CONTAINERS; mixed layouts are rejected.
        let mut config: Self =
            if value.get("settings").is_some() || value.get("CONTAINERS").is_some() {
                value.try_into()?
            } else {
                Self {
                    containers: value.try_into()?,
                    ..Self::default()
                }
            };
        if let Some(endpoint) = &mut config.settings.gitlab_image_api {
            *endpoint = endpoint.trim().to_owned();
            anyhow::ensure!(
                !endpoint.contains('\0'),
                "GITLAB_IMAGE_API must contain no NUL characters"
            );
            if endpoint.is_empty() {
                config.settings.gitlab_image_api = None;
            }
        }
        for (name, settings) in &config.containers {
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
        self.containers
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
    fn structured_configuration_separates_settings_from_container_names() {
        let config = Config::parse(r#"
            [settings]
            GITLAB_IMAGE_API = "https://gitlab.example.com/api/v4/projects/123/registry/repositories"
            [CONTAINERS.web]
            USER = "app"
            [CONTAINERS."db.prod"]
            SHELL = "/bin/sh"
            [CONTAINERS.settings]
            USER = "service"
        "#).unwrap();
        assert_eq!(
            config.settings.gitlab_image_api.as_deref(),
            Some("https://gitlab.example.com/api/v4/projects/123/registry/repositories")
        );
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
        assert_eq!(config.for_container("settings").user, "service");
        assert_eq!(config.for_container("missing"), ShellConfig::default());
    }

    #[test]
    fn missing_or_blank_gitlab_settings_are_optional() {
        for text in [
            "",
            "[settings]",
            "[settings]\nGITLAB_IMAGE_API = '  '",
            "[CONTAINERS.web]",
        ] {
            let config = Config::parse(text).unwrap();
            assert!(config.settings.gitlab_image_api.is_none());
            assert_eq!(config.for_container("web"), ShellConfig::default());
        }
    }

    #[test]
    fn accepts_the_original_misspelling_but_rejects_duplicate_keys() {
        let config =
            Config::parse("[settings]\nGITLAB_IAMGE_API = ' https://gitlab.example.com/api/v4 '")
                .unwrap();
        assert_eq!(
            config.settings.gitlab_image_api.as_deref(),
            Some("https://gitlab.example.com/api/v4")
        );
        assert!(
            Config::parse("[settings]\nGITLAB_IMAGE_API = 'a'\nGITLAB_IAMGE_API = 'b'").is_err()
        );
    }

    #[test]
    fn rejects_unknown_or_mixed_structured_configuration() {
        for text in [
            "[settings]\nUNKNOWN = 'a'",
            "[settings]\nGITLAB_IMAGE_API = 12",
            "[settings]\n[web]\nUSER = 'app'",
            "[CONTAINERS.web]\nUSRE = 'app'",
            "[CONTAINERS.web]\nSHELL = ''",
            "[CONTAINERS.web]\nUSER = '  '",
            "[CONTAINERS.' ']",
        ] {
            assert!(Config::parse(text).is_err(), "accepted {text}");
        }
    }

    #[test]
    fn the_example_configuration_loads() {
        let config = Config::parse(include_str!("../config.example.toml")).unwrap();
        assert_eq!(config.for_container("my-container"), ShellConfig::default());
        assert_eq!(config.for_container("app.production").shell, "/bin/sh");
    }

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
