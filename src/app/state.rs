use std::collections::HashMap;

use chrono::{DateTime, TimeDelta, Utc};

use crate::domain::{ActionKind, Container, Image, Severity, Target};
use crate::engine::EngineInfo;

use super::list::{LoadState, ResourceList};
use super::message::{Action, Command, EngineEvent, Msg};
use crate::engine::EngineError;

/// How long a toast stays on screen.
pub const TOAST_TTL: TimeDelta = TimeDelta::seconds(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Containers,
    Images,
}

impl Tab {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Containers => "Containers",
            Self::Images => "Images",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Containers => Self::Images,
            Self::Images => Self::Containers,
        }
    }
}

/// A transient message in the corner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub text: String,
    pub severity: Severity,
    pub at: DateTime<Utc>,
}

impl Toast {
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        now - self.at >= TOAST_TTL
    }
}

/// What is covering the table, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modal {
    ActionMenu {
        target: Target,
        options: Vec<ActionKind>,
        cursor: usize,
    },
    Confirm {
        action: ActionKind,
        target: Target,
        prompt: String,
    },
    Help,
}

/// The whole of jikura's state. `update` is the only thing that mutates it, and
/// it performs no I/O -- it returns `Command`s for the runtime to run.
#[derive(Debug)]
pub struct App {
    pub tab: Tab,
    pub containers: ResourceList<Container>,
    pub images: ResourceList<Image>,
    /// Include stopped containers (the `-a` of `docker ps`).
    pub show_all: bool,
    pub engine: Option<EngineInfo>,
    pub modal: Option<Modal>,
    pub toasts: Vec<Toast>,
    /// Actions the engine has not answered yet, keyed by target so a row can
    /// show "stopping..." and refuse a second request.
    pub inflight: HashMap<Target, ActionKind>,
    pub should_quit: bool,
}

impl App {
    pub fn new(show_all: bool) -> Self {
        Self {
            tab: Tab::default(),
            containers: ResourceList::new(),
            images: ResourceList::new(),
            show_all,
            engine: None,
            modal: None,
            toasts: Vec::new(),
            inflight: HashMap::new(),
            should_quit: false,
        }
    }

    /// What to load before the first frame.
    pub fn startup(&self) -> Vec<Command> {
        vec![
            Command::LoadInfo,
            Command::LoadContainers { all: self.show_all },
            Command::LoadImages { all: self.show_all },
        ]
    }

    /// The target under the cursor in the active tab.
    pub fn selected_target(&self) -> Option<Target> {
        match self.tab {
            Tab::Containers => self
                .containers
                .selected_item()
                .map(|c| Target::Container(c.id.clone())),
            Tab::Images => self
                .images
                .selected_item()
                .map(|i| Target::Image(i.id.clone())),
        }
    }

    /// Actions offered for the selected row, filtered by what its state permits.
    /// Menu order is fixed so muscle memory works; only membership varies.
    pub fn available_actions(&self) -> Vec<ActionKind> {
        match self.tab {
            Tab::Containers => {
                let Some(container) = self.containers.selected_item() else {
                    return Vec::new();
                };
                const ORDER: [ActionKind; 9] = [
                    ActionKind::Start,
                    ActionKind::Stop,
                    ActionKind::Restart,
                    ActionKind::Pause,
                    ActionKind::Unpause,
                    ActionKind::Enter,
                    ActionKind::Kill,
                    ActionKind::RemoveContainer { force: false },
                    ActionKind::RemoveContainer { force: true },
                ];
                ORDER
                    .into_iter()
                    .filter(|action| container.state.allows(*action))
                    .collect()
            }
            Tab::Images => {
                let Some(image) = self.images.selected_item() else {
                    return Vec::new();
                };
                // The engine refuses a plain removal while a container uses the
                // image, so offering it would only produce a 409.
                if image.is_in_use() == Some(true) {
                    vec![ActionKind::RemoveImage { force: true }]
                } else {
                    vec![
                        ActionKind::RemoveImage { force: false },
                        ActionKind::RemoveImage { force: true },
                    ]
                }
            }
        }
    }

    /// Drops toasts older than [`TOAST_TTL`].
    pub fn prune_toasts(&mut self, now: DateTime<Utc>) {
        self.toasts.retain(|toast| !toast.is_expired(now));
    }

    pub fn update(&mut self, msg: Msg) -> Vec<Command> {
        match msg {
            Msg::Tick => {
                self.prune_toasts(Utc::now());
                self.reload_active()
            }
            Msg::Action(action) => self.on_action(action),
            Msg::Engine(event) => self.on_engine(event),
        }
    }

    fn on_action(&mut self, action: Action) -> Vec<Command> {
        if self.modal.is_some() {
            return self.on_action_in_modal(action);
        }

        match action {
            Action::NextItem => {
                self.active_list_mut(|list| list.select_next(), |list| list.select_next());
                Vec::new()
            }
            Action::PrevItem => {
                self.active_list_mut(|list| list.select_previous(), |list| list.select_previous());
                Vec::new()
            }
            Action::First => {
                self.active_list_mut(|list| list.select_first(), |list| list.select_first());
                Vec::new()
            }
            Action::Last => {
                self.active_list_mut(|list| list.select_last(), |list| list.select_last());
                Vec::new()
            }
            // Two tabs: next and previous are the same move.
            Action::NextTab | Action::PrevTab => {
                self.tab = self.tab.next();
                self.load_if_never()
            }
            Action::ToggleAll => {
                self.show_all = !self.show_all;
                self.reload_active()
            }
            Action::Refresh => self.reload_active(),
            Action::OpenActionMenu | Action::Select => self.open_action_menu(),
            Action::ToggleHelp => {
                self.modal = Some(Modal::Help);
                Vec::new()
            }
            Action::Dismiss => Vec::new(),
            Action::Quit => {
                self.should_quit = true;
                Vec::new()
            }
        }
    }

    fn on_action_in_modal(&mut self, action: Action) -> Vec<Command> {
        match action {
            // Quit closes what is open first, so it never exits by surprise.
            Action::Dismiss | Action::Quit | Action::ToggleHelp => {
                self.modal = None;
                Vec::new()
            }
            Action::NextItem | Action::PrevItem | Action::First | Action::Last => {
                if let Some(Modal::ActionMenu {
                    options, cursor, ..
                }) = &mut self.modal
                {
                    let len = options.len();
                    if len > 0 {
                        *cursor = match action {
                            Action::NextItem => (*cursor + 1) % len,
                            Action::PrevItem => (*cursor + len - 1) % len,
                            Action::First => 0,
                            _ => len - 1,
                        };
                    }
                }
                Vec::new()
            }
            Action::Select => self.activate_modal(),
            _ => Vec::new(),
        }
    }

    fn activate_modal(&mut self) -> Vec<Command> {
        match self.modal.take() {
            Some(Modal::ActionMenu {
                target,
                options,
                cursor,
            }) => {
                let Some(&chosen) = options.get(cursor) else {
                    return Vec::new();
                };
                if chosen.is_destructive() {
                    let prompt = format!("{} {}?", chosen.label(), self.label_for(&target));
                    self.modal = Some(Modal::Confirm {
                        action: chosen,
                        target,
                        prompt,
                    });
                    Vec::new()
                } else {
                    self.dispatch(chosen, target)
                }
            }
            Some(Modal::Confirm { action, target, .. }) => self.dispatch(action, target),
            Some(Modal::Help) | None => Vec::new(),
        }
    }

    fn dispatch(&mut self, action: ActionKind, target: Target) -> Vec<Command> {
        if action == ActionKind::Enter
            && !self.containers.items.iter().any(|container| {
                target == Target::Container(container.id.clone()) && container.state.is_running()
            })
        {
            self.toast(
                "Enter requires a running container".to_owned(),
                Severity::Warn,
            );
            return Vec::new();
        }
        self.inflight.insert(target.clone(), action);
        vec![Command::Perform { action, target }]
    }

    fn open_action_menu(&mut self) -> Vec<Command> {
        let Some(target) = self.selected_target() else {
            return Vec::new();
        };

        if let Some(&busy) = self.inflight.get(&target) {
            let text = format!(
                "already {} {}",
                busy.present_participle(),
                self.label_for(&target)
            );
            self.toast(text, Severity::Warn);
            return Vec::new();
        }

        let options = self.available_actions();
        if options.is_empty() {
            let text = format!("no actions available for {}", self.label_for(&target));
            self.toast(text, Severity::Neutral);
            return Vec::new();
        }

        self.modal = Some(Modal::ActionMenu {
            target,
            options,
            cursor: 0,
        });
        Vec::new()
    }

    fn on_engine(&mut self, event: EngineEvent) -> Vec<Command> {
        match event {
            EngineEvent::Info(Ok(info)) => {
                self.engine = Some(info);
                Vec::new()
            }
            EngineEvent::Info(Err(err)) => {
                self.toast(err.to_string(), Severity::Bad);
                Vec::new()
            }
            EngineEvent::Containers(Ok(items)) => {
                self.containers.replace(items, |c| c.id.clone(), Utc::now());
                Vec::new()
            }
            EngineEvent::Containers(Err(err)) => {
                let text = err.to_string();
                self.containers.fail(text.clone());
                self.toast(text, Severity::Bad);
                Vec::new()
            }
            EngineEvent::Images(Ok(items)) => {
                self.images.replace(items, |i| i.id.clone(), Utc::now());
                Vec::new()
            }
            EngineEvent::Images(Err(err)) => {
                let text = err.to_string();
                self.images.fail(text.clone());
                self.toast(text, Severity::Bad);
                Vec::new()
            }
            EngineEvent::ActionDone {
                action,
                target,
                result,
            } => {
                self.inflight.remove(&target);
                let name = self.label_for(&target);
                match result {
                    Ok(()) => {
                        self.toast(format!("{} {name}: done", action.label()), Severity::Good)
                    }
                    // 304: the engine considers the request already satisfied.
                    Err(EngineError::NotModified) => self.toast(
                        format!("{name} is already in that state"),
                        Severity::Neutral,
                    ),
                    Err(err) => self.toast(format!("{}: {err}", action.label()), Severity::Bad),
                }
                // Reload regardless: a failure often means the state moved anyway.
                if action.targets_image() {
                    vec![Command::LoadImages { all: self.show_all }]
                } else {
                    vec![Command::LoadContainers { all: self.show_all }]
                }
            }
        }
    }

    fn active_list_mut(
        &mut self,
        on_containers: impl FnOnce(&mut ResourceList<Container>),
        on_images: impl FnOnce(&mut ResourceList<Image>),
    ) {
        match self.tab {
            Tab::Containers => on_containers(&mut self.containers),
            Tab::Images => on_images(&mut self.images),
        }
    }

    fn reload_active(&self) -> Vec<Command> {
        match self.tab {
            Tab::Containers => vec![Command::LoadContainers { all: self.show_all }],
            Tab::Images => vec![Command::LoadImages { all: self.show_all }],
        }
    }

    /// Loads a pane the first time it is looked at, and not again.
    fn load_if_never(&self) -> Vec<Command> {
        let load = match self.tab {
            Tab::Containers => &self.containers.load,
            Tab::Images => &self.images.load,
        };
        if *load == LoadState::Never {
            self.reload_active()
        } else {
            Vec::new()
        }
    }

    /// A human name for a target, falling back to its short ID if the row has
    /// since disappeared from the list.
    pub fn label_for(&self, target: &Target) -> String {
        match target {
            Target::Container(id) => self
                .containers
                .items
                .iter()
                .find(|c| &c.id == id)
                .map_or_else(|| id.short().to_owned(), |c| c.display_name().to_owned()),
            Target::Image(id) => self
                .images
                .items
                .iter()
                .find(|i| &i.id == id)
                .map_or_else(|| id.short().to_owned(), Image::name_label),
        }
    }

    fn toast(&mut self, text: String, severity: Severity) {
        self.toasts.push(Toast {
            text,
            severity,
            at: Utc::now(),
        });
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::domain::{ByteSize, ContainerId, ContainerState, ImageId};

    use super::*;

    fn container(name: &str, id: &str, state: ContainerState) -> Container {
        Container {
            id: ContainerId::from(id.to_owned()),
            names: vec![name.to_owned()],
            image: "nginx:latest".to_owned(),
            image_id: ImageId::from("sha256:5d0da3dc976460b7".to_owned()),
            command: "nginx".to_owned(),
            created: DateTime::UNIX_EPOCH,
            state,
            status_text: "Up 2 hours".to_owned(),
            ports: Vec::new(),
            labels: BTreeMap::new(),
        }
    }

    fn image(id: &str, containers: Option<i64>) -> Image {
        Image {
            id: ImageId::from(id.to_owned()),
            repo_tags: vec![crate::domain::ImageRef::parse("nginx:latest")],
            repo_digests: Vec::new(),
            created: DateTime::UNIX_EPOCH,
            size: ByteSize(1_000_000),
            shared_size: None,
            containers,
            labels: BTreeMap::new(),
        }
    }

    /// An app with one running and one stopped container, and one image.
    fn loaded_app() -> App {
        let mut app = App::new(true);
        app.update(Msg::Engine(EngineEvent::Containers(Ok(vec![
            container("web", "aaaa000000001111", ContainerState::Running),
            container("db", "bbbb000000002222", ContainerState::Exited),
        ]))));
        app.update(Msg::Engine(EngineEvent::Images(Ok(vec![image(
            "sha256:cccc000000003333",
            Some(0),
        )]))));
        app
    }

    fn act(app: &mut App, action: Action) -> Vec<Command> {
        app.update(Msg::Action(action))
    }

    #[test]
    fn startup_loads_the_engine_banner_and_both_panes() {
        let app = App::new(false);
        assert_eq!(
            app.startup(),
            vec![
                Command::LoadInfo,
                Command::LoadContainers { all: false },
                Command::LoadImages { all: false },
            ]
        );
    }

    #[test]
    fn a_successful_load_fills_the_pane_and_selects_the_first_row() {
        let app = loaded_app();
        assert_eq!(app.containers.len(), 2);
        assert!(matches!(app.containers.load, LoadState::Loaded { .. }));
        assert_eq!(
            app.containers.selected_item().unwrap().display_name(),
            "web"
        );
    }

    #[test]
    fn a_failed_load_keeps_the_rows_and_reports_it_once() {
        let mut app = loaded_app();
        app.update(Msg::Engine(EngineEvent::Containers(Err(
            EngineError::Unreachable {
                endpoint: "unix:///run/x.sock".to_owned(),
                detail: "refused".to_owned(),
            },
        ))));
        assert_eq!(app.containers.len(), 2, "rows must stay on screen");
        assert!(matches!(app.containers.load, LoadState::Failed { .. }));
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].severity, Severity::Bad);
        assert!(app.toasts[0].text.contains("unix:///run/x.sock"));
    }

    #[test]
    fn the_cursor_moves_within_the_active_pane_only() {
        let mut app = loaded_app();
        act(&mut app, Action::NextItem);
        assert_eq!(app.containers.selected(), Some(1));
        assert_eq!(app.images.selected(), Some(0), "images pane untouched");
    }

    #[test]
    fn switching_tabs_moves_the_cursor_in_the_other_pane_instead() {
        let mut app = loaded_app();
        act(&mut app, Action::NextTab);
        assert_eq!(app.tab, Tab::Images);
        act(&mut app, Action::NextItem);
        assert_eq!(app.containers.selected(), Some(0), "containers untouched");
    }

    #[test]
    fn switching_to_a_never_loaded_pane_asks_for_its_data() {
        let mut app = App::new(false);
        let commands = act(&mut app, Action::NextTab);
        assert_eq!(commands, vec![Command::LoadImages { all: false }]);
    }

    #[test]
    fn refresh_reloads_only_the_active_pane() {
        let mut app = loaded_app();
        assert_eq!(
            act(&mut app, Action::Refresh),
            vec![Command::LoadContainers { all: true }]
        );
        act(&mut app, Action::NextTab);
        assert_eq!(
            act(&mut app, Action::Refresh),
            vec![Command::LoadImages { all: true }]
        );
    }

    #[test]
    fn toggling_all_reloads_containers_with_the_new_filter() {
        let mut app = App::new(false);
        let commands = act(&mut app, Action::ToggleAll);
        assert!(app.show_all);
        assert_eq!(commands, vec![Command::LoadContainers { all: true }]);
    }

    #[test]
    fn the_action_menu_offers_only_what_the_state_permits() {
        let mut app = loaded_app();
        act(&mut app, Action::OpenActionMenu);
        match app.modal.as_ref().expect("menu should open") {
            Modal::ActionMenu { options, .. } => {
                assert!(options.contains(&ActionKind::Stop), "running: can stop");
                assert!(options.contains(&ActionKind::Pause));
                assert!(
                    !options.contains(&ActionKind::Start),
                    "running: cannot start"
                );
                assert!(!options.contains(&ActionKind::Unpause));
            }
            other => panic!("expected an action menu, got {other:?}"),
        }
    }

    #[test]
    fn the_menu_for_a_stopped_container_offers_start_and_plain_removal() {
        let mut app = loaded_app();
        act(&mut app, Action::NextItem); // the exited one
        act(&mut app, Action::OpenActionMenu);
        match app.modal.as_ref().unwrap() {
            Modal::ActionMenu { options, .. } => {
                assert!(options.contains(&ActionKind::Start));
                assert!(options.contains(&ActionKind::RemoveContainer { force: false }));
                assert!(!options.contains(&ActionKind::Stop));
                assert!(!options.contains(&ActionKind::Enter));
            }
            other => panic!("expected an action menu, got {other:?}"),
        }
    }

    #[test]
    fn entering_uses_the_selected_full_id_without_confirmation() {
        let mut app = loaded_app();
        let target = app.selected_target().unwrap();
        act(&mut app, Action::OpenActionMenu);
        let position = app
            .available_actions()
            .iter()
            .position(|action| *action == ActionKind::Enter)
            .unwrap();
        for _ in 0..position {
            act(&mut app, Action::NextItem);
        }
        assert_eq!(
            act(&mut app, Action::Select),
            vec![Command::Perform {
                action: ActionKind::Enter,
                target: target.clone(),
            }]
        );
        assert!(app.modal.is_none());
        assert_eq!(app.inflight.get(&target), Some(&ActionKind::Enter));
    }

    #[test]
    fn entering_is_refused_if_the_container_stops_while_the_menu_is_open() {
        let mut app = loaded_app();
        act(&mut app, Action::OpenActionMenu);
        let position = app
            .available_actions()
            .iter()
            .position(|action| *action == ActionKind::Enter)
            .unwrap();
        for _ in 0..position {
            act(&mut app, Action::NextItem);
        }
        app.update(Msg::Engine(EngineEvent::Containers(Ok(vec![container(
            "web",
            "aaaa000000001111",
            ContainerState::Exited,
        )]))));
        assert!(act(&mut app, Action::Select).is_empty());
        assert!(app.inflight.is_empty());
        assert!(app.toasts.last().unwrap().text.contains("running"));
    }

    #[test]
    fn the_cursor_keys_drive_the_menu_while_it_is_open() {
        let mut app = loaded_app();
        act(&mut app, Action::OpenActionMenu);
        act(&mut app, Action::NextItem);
        match app.modal.as_ref().unwrap() {
            Modal::ActionMenu { cursor, .. } => assert_eq!(*cursor, 1),
            other => panic!("expected an action menu, got {other:?}"),
        }
        assert_eq!(
            app.containers.selected(),
            Some(0),
            "the table cursor must not move while a menu is open"
        );
    }

    #[test]
    fn a_harmless_action_runs_straight_away_and_marks_the_row_busy() {
        let mut app = loaded_app();
        let target = app.selected_target().unwrap();
        act(&mut app, Action::OpenActionMenu);
        // Stop is first in the menu for a running container.
        let commands = act(&mut app, Action::Select);
        assert_eq!(
            commands,
            vec![Command::Perform {
                action: ActionKind::Stop,
                target: target.clone()
            }]
        );
        assert_eq!(app.inflight.get(&target), Some(&ActionKind::Stop));
        assert!(app.modal.is_none(), "menu closes once the action is sent");
    }

    #[test]
    fn a_destructive_action_asks_first_and_sends_nothing_yet() {
        let mut app = loaded_app();
        act(&mut app, Action::NextItem); // exited container: removable
        act(&mut app, Action::OpenActionMenu);
        // Walk to the plain Remove entry.
        let removal = ActionKind::RemoveContainer { force: false };
        let position = app
            .available_actions()
            .iter()
            .position(|a| *a == removal)
            .expect("removal offered");
        for _ in 0..position {
            act(&mut app, Action::NextItem);
        }
        let commands = act(&mut app, Action::Select);
        assert!(commands.is_empty(), "nothing runs before confirmation");
        assert!(matches!(
            app.modal,
            Some(Modal::Confirm { action, .. }) if action == removal
        ));
        assert!(app.inflight.is_empty());
    }

    #[test]
    fn confirming_sends_the_action_and_declining_sends_nothing() {
        let mut app = loaded_app();
        let target = Target::Container(ContainerId::from("bbbb000000002222".to_owned()));
        app.modal = Some(Modal::Confirm {
            action: ActionKind::RemoveContainer { force: false },
            target: target.clone(),
            prompt: "Remove db?".to_owned(),
        });
        let commands = act(&mut app, Action::Select);
        assert_eq!(
            commands,
            vec![Command::Perform {
                action: ActionKind::RemoveContainer { force: false },
                target: target.clone()
            }]
        );
        assert!(app.modal.is_none());

        let mut app = loaded_app();
        app.modal = Some(Modal::Confirm {
            action: ActionKind::RemoveContainer { force: false },
            target,
            prompt: "Remove db?".to_owned(),
        });
        assert!(act(&mut app, Action::Dismiss).is_empty());
        assert!(app.modal.is_none());
        assert!(app.inflight.is_empty());
    }

    #[test]
    fn a_second_action_on_a_busy_row_is_refused_with_an_explanation() {
        let mut app = loaded_app();
        let target = app.selected_target().unwrap();
        app.inflight.insert(target, ActionKind::Stop);
        let commands = act(&mut app, Action::OpenActionMenu);
        assert!(commands.is_empty());
        assert!(app.modal.is_none());
        assert_eq!(app.toasts.len(), 1);
        assert!(app.toasts[0].text.contains("stopping"));
    }

    #[test]
    fn a_finished_action_clears_the_marker_reports_it_and_reloads() {
        let mut app = loaded_app();
        let target = app.selected_target().unwrap();
        app.inflight.insert(target.clone(), ActionKind::Stop);
        let commands = app.update(Msg::Engine(EngineEvent::ActionDone {
            action: ActionKind::Stop,
            target,
            result: Ok(()),
        }));
        assert!(app.inflight.is_empty());
        assert_eq!(commands, vec![Command::LoadContainers { all: true }]);
        assert_eq!(app.toasts[0].severity, Severity::Good);
    }

    #[test]
    fn a_failed_action_reports_the_engines_reason_and_still_reloads() {
        let mut app = loaded_app();
        let target = app.selected_target().unwrap();
        app.inflight.insert(target.clone(), ActionKind::Stop);
        let commands = app.update(Msg::Engine(EngineEvent::ActionDone {
            action: ActionKind::Stop,
            target,
            result: Err(EngineError::Conflict {
                message: "container is paused".to_owned(),
            }),
        }));
        assert!(app.inflight.is_empty());
        assert_eq!(commands, vec![Command::LoadContainers { all: true }]);
        assert_eq!(app.toasts[0].severity, Severity::Bad);
        assert!(app.toasts[0].text.contains("container is paused"));
    }

    #[test]
    fn an_already_in_that_state_reply_is_information_not_an_error() {
        let mut app = loaded_app();
        let target = app.selected_target().unwrap();
        app.inflight.insert(target.clone(), ActionKind::Start);
        app.update(Msg::Engine(EngineEvent::ActionDone {
            action: ActionKind::Start,
            target,
            result: Err(EngineError::NotModified),
        }));
        assert_eq!(app.toasts[0].severity, Severity::Neutral);
    }

    #[test]
    fn an_image_in_use_may_only_be_force_removed() {
        let mut app = loaded_app();
        app.update(Msg::Engine(EngineEvent::Images(Ok(vec![image(
            "sha256:cccc000000003333",
            Some(2),
        )]))));
        act(&mut app, Action::NextTab);
        let options = app.available_actions();
        assert_eq!(options, vec![ActionKind::RemoveImage { force: true }]);
    }

    #[test]
    fn an_unused_image_offers_plain_removal_too() {
        let mut app = loaded_app();
        act(&mut app, Action::NextTab);
        assert_eq!(
            app.available_actions(),
            vec![
                ActionKind::RemoveImage { force: false },
                ActionKind::RemoveImage { force: true },
            ]
        );
    }

    #[test]
    fn acting_on_an_empty_pane_does_nothing() {
        let mut app = App::new(true);
        assert!(act(&mut app, Action::OpenActionMenu).is_empty());
        assert!(app.modal.is_none());
        assert_eq!(app.selected_target(), None);
    }

    #[test]
    fn escape_closes_a_modal_and_quit_only_bites_once_nothing_is_open() {
        let mut app = loaded_app();
        act(&mut app, Action::ToggleHelp);
        assert_eq!(app.modal, Some(Modal::Help));
        act(&mut app, Action::Quit);
        assert!(!app.should_quit, "quit closes the modal first");
        assert!(app.modal.is_none());
        act(&mut app, Action::Quit);
        assert!(app.should_quit);
    }

    #[test]
    fn a_tick_refreshes_the_active_pane_and_expires_old_toasts() {
        let mut app = loaded_app();
        let now = Utc::now();
        app.toasts.push(Toast {
            text: "old".to_owned(),
            severity: Severity::Neutral,
            at: now - TOAST_TTL,
        });
        app.toasts.push(Toast {
            text: "fresh".to_owned(),
            severity: Severity::Neutral,
            at: now,
        });
        let commands = app.update(Msg::Tick);
        assert_eq!(commands, vec![Command::LoadContainers { all: true }]);
        assert_eq!(app.toasts.len(), 1);
        assert_eq!(app.toasts[0].text, "fresh");
    }

    #[test]
    fn the_engine_banner_is_remembered_and_a_failure_to_read_it_is_reported() {
        let mut app = App::new(false);
        app.update(Msg::Engine(EngineEvent::Info(Ok(EngineInfo {
            flavor: crate::engine::EngineFlavor::Podman,
            version: "5.6.2".to_owned(),
            api_version: "1.52".to_owned(),
        }))));
        assert_eq!(app.engine.as_ref().unwrap().version, "5.6.2");

        app.update(Msg::Engine(EngineEvent::Info(Err(
            EngineError::Unreachable {
                endpoint: "unix:///run/x.sock".to_owned(),
                detail: "refused".to_owned(),
            },
        ))));
        assert_eq!(app.toasts.last().unwrap().severity, Severity::Bad);
    }
}
