//! Runs the `Command`s that `App::update` returns. Every command becomes a
//! spawned task, so a `stop` that takes the engine ten seconds never blocks a
//! keystroke or a redraw; its outcome arrives later as an `EngineEvent`.

use std::sync::Arc;

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::app::{Command, EngineEvent};
use crate::engine::Engine;

pub struct Runtime<E> {
    engine: Arc<E>,
    events: UnboundedSender<EngineEvent>,
}

impl<E: Engine> Runtime<E> {
    /// Returns the runtime and the receiver the event loop selects on.
    pub fn new(engine: Arc<E>) -> (Self, UnboundedReceiver<EngineEvent>) {
        let (events, rx) = mpsc::unbounded_channel();
        (Self { engine, events }, rx)
    }

    pub fn spawn_all(&self, commands: Vec<Command>) {
        for command in commands {
            self.spawn(command);
        }
    }

    pub fn spawn(&self, command: Command) {
        let engine = Arc::clone(&self.engine);
        let events = self.events.clone();

        tokio::spawn(async move {
            let event = match command {
                Command::LoadInfo => EngineEvent::Info(engine.info().await),
                Command::LoadContainers { all } => {
                    EngineEvent::Containers(engine.list_containers(all).await)
                }
                Command::LoadImages { all } => EngineEvent::Images(engine.list_images(all).await),
                Command::Perform { action, target } => EngineEvent::ActionDone {
                    action,
                    target: target.clone(),
                    result: engine.perform(action, target).await,
                },
            };
            // A closed receiver means the UI is already shutting down.
            let _ = events.send(event);
        });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::DateTime;

    use crate::domain::{ActionKind, Container, ContainerId, ContainerState, ImageId, Target};
    use crate::engine::EngineError;
    use crate::engine::fake::{Call, FakeEngine};

    use super::*;

    fn container(name: &str) -> Container {
        Container {
            id: ContainerId::from("aaaa000000001111".to_owned()),
            names: vec![name.to_owned()],
            image: "nginx:latest".to_owned(),
            image_id: ImageId::from("sha256:5d0da3dc976460b7".to_owned()),
            command: "nginx".to_owned(),
            created: DateTime::UNIX_EPOCH,
            state: ContainerState::Running,
            status_text: "Up".to_owned(),
            ports: Vec::new(),
            labels: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn a_load_command_comes_back_as_rows_with_the_filter_it_was_given() {
        let engine = Arc::new(FakeEngine::new().with_containers(vec![container("web")]));
        let (runtime, mut events) = Runtime::new(Arc::clone(&engine));

        runtime.spawn(Command::LoadContainers { all: true });

        match events.recv().await.expect("an event") {
            EngineEvent::Containers(Ok(rows)) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].display_name(), "web");
            }
            other => panic!("expected containers, got {other:?}"),
        }
        assert_eq!(engine.calls(), vec![Call::ListContainers { all: true }]);
    }

    #[tokio::test]
    async fn an_action_reports_back_with_its_own_target() {
        let engine = Arc::new(FakeEngine::new());
        let (runtime, mut events) = Runtime::new(Arc::clone(&engine));
        let target = Target::Container(ContainerId::from("aaaa000000001111".to_owned()));

        runtime.spawn(Command::Perform {
            action: ActionKind::Stop,
            target: target.clone(),
        });

        match events.recv().await.expect("an event") {
            EngineEvent::ActionDone {
                action,
                target: reported,
                result,
            } => {
                assert_eq!(action, ActionKind::Stop);
                assert_eq!(reported, target, "the reply must identify its own row");
                assert!(result.is_ok());
            }
            other => panic!("expected an action result, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_engine_failure_arrives_as_an_event_rather_than_a_panic() {
        let engine = Arc::new(FakeEngine::failing(EngineError::Unreachable {
            endpoint: "unix:///run/x.sock".to_owned(),
            detail: "refused".to_owned(),
        }));
        let (runtime, mut events) = Runtime::new(engine);

        runtime.spawn(Command::LoadImages { all: false });

        match events.recv().await.expect("an event") {
            EngineEvent::Images(Err(EngineError::Unreachable { endpoint, .. })) => {
                assert_eq!(endpoint, "unix:///run/x.sock");
            }
            other => panic!("expected an unreachable engine, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn commands_run_concurrently_not_one_after_another() {
        let engine = Arc::new(FakeEngine::new());
        let (runtime, mut events) = Runtime::new(Arc::clone(&engine));

        runtime.spawn_all(vec![
            Command::LoadInfo,
            Command::LoadContainers { all: false },
            Command::LoadImages { all: false },
        ]);

        let mut seen = 0;
        while seen < 3 {
            assert!(events.recv().await.is_some());
            seen += 1;
        }
        assert_eq!(engine.calls().len(), 3);
    }
}
