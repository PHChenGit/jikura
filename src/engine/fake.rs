//! An in-memory `Engine` for tests: scripted answers plus a record of calls.
//! Compiled only under `cfg(test)`, so it never ships in the binary.

use std::sync::Mutex;

use crate::domain::{ActionKind, Container, Image, Target};

use super::{Engine, EngineError, EngineFlavor, EngineInfo};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Info,
    ListContainers { all: bool },
    ListImages { all: bool },
    Perform { action: ActionKind, target: Target },
}

#[derive(Debug, Default)]
pub struct FakeEngine {
    containers: Vec<Container>,
    images: Vec<Image>,
    /// When set, every call fails with it.
    error: Option<EngineError>,
    calls: Mutex<Vec<Call>>,
}

impl FakeEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_containers(mut self, containers: Vec<Container>) -> Self {
        self.containers = containers;
        self
    }

    pub fn with_images(mut self, images: Vec<Image>) -> Self {
        self.images = images;
        self
    }

    pub fn failing(error: EngineError) -> Self {
        Self {
            error: Some(error),
            ..Self::default()
        }
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls
            .lock()
            .expect("no test holds this across a panic")
            .clone()
    }

    fn record(&self, call: Call) -> Result<(), EngineError> {
        self.calls.lock().expect("uncontended").push(call);
        match &self.error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

impl Engine for FakeEngine {
    async fn info(&self) -> Result<EngineInfo, EngineError> {
        self.record(Call::Info)?;
        Ok(EngineInfo {
            flavor: EngineFlavor::Podman,
            version: "5.6.2".to_owned(),
            api_version: "1.52".to_owned(),
        })
    }

    async fn list_containers(&self, all: bool) -> Result<Vec<Container>, EngineError> {
        self.record(Call::ListContainers { all })?;
        Ok(self.containers.clone())
    }

    async fn list_images(&self, all: bool) -> Result<Vec<Image>, EngineError> {
        self.record(Call::ListImages { all })?;
        Ok(self.images.clone())
    }

    async fn perform(&self, action: ActionKind, target: Target) -> Result<(), EngineError> {
        self.record(Call::Perform { action, target })
    }
}
