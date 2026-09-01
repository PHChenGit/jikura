//! The engine port: what jikura needs from a container engine, plus the error
//! taxonomy the UI reports. `map` and `docker` are the bollard-facing parts.

pub mod docker;
#[cfg(test)]
pub mod fake;
pub mod map;

use std::fmt;
use std::future::Future;

use bollard::models::SystemVersion;

use crate::domain::{ActionKind, Container, Image, Target};

pub use docker::BollardEngine;

/// Everything jikura asks of an engine. Not dyn-compatible on purpose: the
/// futures are `Send` so tasks can be spawned, and the app never needs a
/// trait object -- only the task spawner holds the engine.
pub trait Engine: Send + Sync + 'static {
    fn info(&self) -> impl Future<Output = Result<EngineInfo, EngineError>> + Send;

    fn list_containers(
        &self,
        all: bool,
    ) -> impl Future<Output = Result<Vec<Container>, EngineError>> + Send;

    fn list_images(
        &self,
        all: bool,
    ) -> impl Future<Output = Result<Vec<Image>, EngineError>> + Send;

    /// One entry point for every lifecycle action, so the dispatcher stays a
    /// single line and new actions need no new plumbing.
    fn perform(
        &self,
        action: ActionKind,
        target: Target,
    ) -> impl Future<Output = Result<(), EngineError>> + Send;
}

/// Which engine answered, for the status bar and for quirk handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineFlavor {
    Docker,
    Podman,
    Unknown,
}

impl fmt::Display for EngineFlavor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Docker => "docker",
            Self::Podman => "podman",
            Self::Unknown => "engine",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineInfo {
    pub flavor: EngineFlavor,
    pub version: String,
    pub api_version: String,
}

/// Failures the UI has to explain to a human. Deliberately coarse: each variant
/// maps to a different thing the user might do about it.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum EngineError {
    #[error("cannot reach a container engine at {endpoint}: {detail}")]
    Unreachable { endpoint: String, detail: String },

    #[error("permission denied at {endpoint}: {detail}")]
    PermissionDenied { endpoint: String, detail: String },

    #[error("{message}")]
    NotFound { message: String },

    #[error("{message}")]
    Conflict { message: String },

    #[error("already in that state")]
    NotModified,

    #[error("engine returned {status}: {message}")]
    Api { status: u16, message: String },

    #[error("could not read the engine's reply ({message}); docker/podman API version skew?")]
    Decode { message: String },

    #[error("{message}")]
    Other { message: String },
}

impl EngineError {
    /// Whether retrying the same call unchanged could plausibly succeed.
    /// Drives whether the UI keeps polling or waits for the user.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::Unreachable { .. }
                | Self::Api {
                    status: 500..=599,
                    ..
                }
        )
    }
}

/// Translates a bollard failure into something a user can act on. `endpoint` is
/// the DOCKER_HOST jikura was pointed at -- the single most useful detail when
/// the socket is missing or unreadable.
pub fn classify(err: bollard::errors::Error, endpoint: &str) -> EngineError {
    use bollard::errors::Error as Bollard;
    match err {
        Bollard::DockerResponseServerError {
            status_code,
            message,
        } => match status_code {
            304 => EngineError::NotModified,
            401 | 403 => EngineError::PermissionDenied {
                endpoint: endpoint.to_owned(),
                detail: message,
            },
            404 => EngineError::NotFound { message },
            409 => EngineError::Conflict { message },
            status => EngineError::Api { status, message },
        },
        // The generated bindings have no catch-all for unknown enum values, so
        // one unexpected field fails the whole response: worth naming as skew.
        Bollard::JsonDataError { message, .. } => EngineError::Decode { message },
        Bollard::JsonSerdeError { err } => EngineError::Decode {
            message: err.to_string(),
        },
        Bollard::IOError { err } => match err.kind() {
            std::io::ErrorKind::PermissionDenied => EngineError::PermissionDenied {
                endpoint: endpoint.to_owned(),
                detail: err.to_string(),
            },
            _ => EngineError::Unreachable {
                endpoint: endpoint.to_owned(),
                detail: err.to_string(),
            },
        },
        Bollard::HyperResponseError { err } => EngineError::Unreachable {
            endpoint: endpoint.to_owned(),
            detail: err.to_string(),
        },
        Bollard::HyperLegacyError { err } => EngineError::Unreachable {
            endpoint: endpoint.to_owned(),
            detail: err.to_string(),
        },
        Bollard::SocketNotFoundError(path) => EngineError::Unreachable {
            endpoint: endpoint.to_owned(),
            detail: format!("no socket at {path}"),
        },
        other => EngineError::Other {
            message: other.to_string(),
        },
    }
}

/// Reads `/version` into the status-bar summary. Podman announces itself in
/// `components`, docker in `platform.name`.
pub fn engine_info(version: SystemVersion) -> EngineInfo {
    let announces_podman = version
        .components
        .iter()
        .flatten()
        .any(|component| component.name.to_ascii_lowercase().contains("podman"))
        || version
            .platform
            .as_ref()
            .is_some_and(|platform| platform.name.to_ascii_lowercase().contains("podman"));

    let announces_docker = version
        .platform
        .as_ref()
        .is_some_and(|platform| platform.name.to_ascii_lowercase().contains("docker"))
        || version.components.iter().flatten().any(|component| {
            component
                .name
                .to_ascii_lowercase()
                .contains("docker engine")
        });

    let flavor = match (announces_podman, announces_docker) {
        // Podman wins a tie: its compat endpoint can mention docker.
        (true, _) => EngineFlavor::Podman,
        (false, true) => EngineFlavor::Docker,
        (false, false) => EngineFlavor::Unknown,
    };

    EngineInfo {
        flavor,
        version: version.version.unwrap_or_else(|| "unknown".to_owned()),
        api_version: version.api_version.unwrap_or_else(|| "unknown".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use bollard::errors::Error as Bollard;
    use bollard::models::{SystemVersionComponents, SystemVersionPlatform};

    use super::*;

    fn server_error(status_code: u16, message: &str) -> Bollard {
        Bollard::DockerResponseServerError {
            status_code,
            message: message.to_owned(),
        }
    }

    #[test]
    fn a_404_becomes_not_found_carrying_the_engines_own_wording() {
        let err = classify(
            server_error(404, "No such container: abc"),
            "unix:///run/x.sock",
        );
        assert_eq!(
            err,
            EngineError::NotFound {
                message: "No such container: abc".to_owned()
            }
        );
    }

    #[test]
    fn a_409_becomes_conflict_because_the_user_can_force_it() {
        let err = classify(
            server_error(409, "container is running: stop it first"),
            "unix:///run/x.sock",
        );
        assert!(matches!(err, EngineError::Conflict { .. }));
    }

    #[test]
    fn a_304_is_not_an_error_the_user_should_worry_about() {
        let err = classify(server_error(304, ""), "unix:///run/x.sock");
        assert_eq!(err, EngineError::NotModified);
    }

    #[test]
    fn a_403_names_the_endpoint_so_the_socket_can_be_fixed() {
        let err = classify(
            server_error(403, "forbidden"),
            "unix:///run/user/1000/podman.sock",
        );
        match err {
            EngineError::PermissionDenied { endpoint, .. } => {
                assert_eq!(endpoint, "unix:///run/user/1000/podman.sock");
            }
            other => panic!("expected PermissionDenied, got {other:?}"),
        }
    }

    #[test]
    fn an_unmapped_status_keeps_its_code() {
        let err = classify(server_error(500, "boom"), "unix:///run/x.sock");
        assert_eq!(
            err,
            EngineError::Api {
                status: 500,
                message: "boom".to_owned()
            }
        );
    }

    #[test]
    fn a_json_failure_is_reported_as_version_skew_not_a_crash() {
        let err = classify(
            Bollard::JsonDataError {
                message: "unknown variant `stopped`".to_owned(),
                column: 42,
            },
            "unix:///run/x.sock",
        );
        match err {
            EngineError::Decode { message } => assert!(message.contains("stopped")),
            other => panic!("expected Decode, got {other:?}"),
        }
    }

    #[test]
    fn a_refused_connection_names_the_endpoint() {
        let err = classify(
            Bollard::IOError {
                err: io::Error::new(io::ErrorKind::ConnectionRefused, "refused"),
            },
            "unix:///run/user/1000/podman.sock",
        );
        match err {
            EngineError::Unreachable { endpoint, .. } => {
                assert_eq!(endpoint, "unix:///run/user/1000/podman.sock");
            }
            other => panic!("expected Unreachable, got {other:?}"),
        }
    }

    #[test]
    fn an_unreadable_socket_is_a_permission_problem_not_an_absent_engine() {
        let err = classify(
            Bollard::IOError {
                err: io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
            },
            "unix:///run/user/1000/podman.sock",
        );
        assert!(matches!(err, EngineError::PermissionDenied { .. }));
    }

    #[test]
    fn only_reachability_failures_are_worth_retrying() {
        assert!(
            EngineError::Unreachable {
                endpoint: "x".to_owned(),
                detail: "y".to_owned()
            }
            .is_transient()
        );
        assert!(
            !EngineError::NotFound {
                message: "gone".to_owned()
            }
            .is_transient()
        );
        assert!(
            !EngineError::Decode {
                message: "skew".to_owned()
            }
            .is_transient()
        );
    }

    #[test]
    fn podman_is_recognised_from_its_component_list() {
        let info = engine_info(SystemVersion {
            version: Some("5.6.2".to_owned()),
            api_version: Some("1.52".to_owned()),
            components: Some(vec![SystemVersionComponents {
                name: "Podman Engine".to_owned(),
                version: "5.6.2".to_owned(),
                details: None,
            }]),
            ..Default::default()
        });
        assert_eq!(
            info,
            EngineInfo {
                flavor: EngineFlavor::Podman,
                version: "5.6.2".to_owned(),
                api_version: "1.52".to_owned(),
            }
        );
    }

    #[test]
    fn docker_is_recognised_from_its_platform_name() {
        let info = engine_info(SystemVersion {
            version: Some("27.5.1".to_owned()),
            api_version: Some("1.47".to_owned()),
            platform: Some(SystemVersionPlatform {
                name: "Docker Engine - Community".to_owned(),
            }),
            ..Default::default()
        });
        assert_eq!(info.flavor, EngineFlavor::Docker);
    }

    #[test]
    fn an_unannounced_engine_still_yields_usable_info() {
        let info = engine_info(SystemVersion::default());
        assert_eq!(info.flavor, EngineFlavor::Unknown);
        assert_eq!(info.version, "unknown");
        assert_eq!(info.api_version, "unknown");
    }
}
