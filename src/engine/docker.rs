//! The bollard-backed `Engine`. Talks to whatever `DOCKER_HOST` names -- the
//! Docker API, or podman's compatible endpoint.

use bollard::Docker;
use bollard::query_parameters::{
    ListContainersOptionsBuilder, ListImagesOptionsBuilder, RemoveContainerOptionsBuilder,
    RemoveImageOptionsBuilder,
};

use crate::domain::{ActionKind, Container, Image, Target};

use super::{Engine, EngineError, EngineInfo, classify, map};

/// Default when `DOCKER_HOST` is unset, matching bollard's own fallback.
const DEFAULT_ENDPOINT: &str = "unix:///var/run/docker.sock";

pub struct BollardEngine {
    client: Docker,
    /// Kept for error messages: "cannot reach an engine at ..." is only useful
    /// if it names where jikura looked.
    endpoint: String,
}

impl BollardEngine {
    /// Connects using `DOCKER_HOST` (or the platform default). Lazy: a failure
    /// here means the address is unusable, not that the engine is down.
    pub fn connect() -> Result<Self, EngineError> {
        let endpoint = std::env::var("DOCKER_HOST").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
        let client = Docker::connect_with_defaults().map_err(|err| classify(err, &endpoint))?;
        Ok(Self { client, endpoint })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn wrap(&self, err: bollard::errors::Error) -> EngineError {
        classify(err, &self.endpoint)
    }
}

impl Engine for BollardEngine {
    async fn info(&self) -> Result<EngineInfo, EngineError> {
        let version = self.client.version().await.map_err(|err| self.wrap(err))?;
        Ok(super::engine_info(version))
    }

    async fn list_containers(&self, all: bool) -> Result<Vec<Container>, EngineError> {
        let options = ListContainersOptionsBuilder::new().all(all).build();
        let summaries = self
            .client
            .list_containers(Some(options))
            .await
            .map_err(|err| self.wrap(err))?;
        // filter_map: an entry with no ID cannot be acted on, so it is dropped
        // rather than failing the whole refresh.
        Ok(summaries.into_iter().filter_map(map::container).collect())
    }

    async fn list_images(&self, all: bool) -> Result<Vec<Image>, EngineError> {
        let options = ListImagesOptionsBuilder::new().all(all).build();
        let summaries = self
            .client
            .list_images(Some(options))
            .await
            .map_err(|err| self.wrap(err))?;
        Ok(summaries.into_iter().map(map::image).collect())
    }

    async fn perform(&self, action: ActionKind, target: Target) -> Result<(), EngineError> {
        use ActionKind::*;
        let reference = target.as_str().to_owned();

        match (action, &target) {
            (Start, Target::Container(_)) => self
                .client
                .start_container(&reference, None)
                .await
                .map_err(|err| self.wrap(err)),
            (Stop, Target::Container(_)) => self
                .client
                .stop_container(&reference, None)
                .await
                .map_err(|err| self.wrap(err)),
            (Restart, Target::Container(_)) => self
                .client
                .restart_container(&reference, None)
                .await
                .map_err(|err| self.wrap(err)),
            (Kill, Target::Container(_)) => self
                .client
                .kill_container(&reference, None)
                .await
                .map_err(|err| self.wrap(err)),
            (Pause, Target::Container(_)) => self
                .client
                .pause_container(&reference)
                .await
                .map_err(|err| self.wrap(err)),
            (Unpause, Target::Container(_)) => self
                .client
                .unpause_container(&reference)
                .await
                .map_err(|err| self.wrap(err)),
            (RemoveContainer { force }, Target::Container(_)) => self
                .client
                .remove_container(
                    &reference,
                    Some(RemoveContainerOptionsBuilder::new().force(force).build()),
                )
                .await
                .map_err(|err| self.wrap(err)),
            (RemoveImage { force }, Target::Image(_)) => self
                .client
                .remove_image(
                    &reference,
                    Some(RemoveImageOptionsBuilder::new().force(force).build()),
                    None,
                )
                .await
                .map(|_| ())
                .map_err(|err| self.wrap(err)),
            // The UI never offers these, but the type system permits them.
            (action, target) => Err(EngineError::Other {
                message: format!(
                    "{} does not apply to {}",
                    action.label(),
                    match target {
                        Target::Container(_) => "a container",
                        Target::Image(_) => "an image",
                    }
                ),
            }),
        }
    }
}
