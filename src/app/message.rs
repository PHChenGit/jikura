use crate::domain::{ActionKind, Container, Image, Target};
use crate::engine::{EngineError, EngineInfo};

/// A user intent, already decoded from a keypress. The keymap lives in `ui`, so
/// `app` never sees a terminal type and `update` stays trivially testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    NextItem,
    PrevItem,
    First,
    Last,
    NextTab,
    PrevTab,
    /// Show stopped containers too.
    ToggleAll,
    Refresh,
    /// Open the action menu for the selected row.
    OpenActionMenu,
    /// Enter: pick the highlighted menu entry, or answer a confirmation yes.
    Select,
    /// Esc: close whatever is open, answering any confirmation no.
    Dismiss,
    ToggleHelp,
    Quit,
}

/// Something finished in the background.
#[derive(Debug)]
pub enum EngineEvent {
    Info(Result<EngineInfo, EngineError>),
    Containers(Result<Vec<Container>, EngineError>),
    Images(Result<Vec<Image>, EngineError>),
    ActionDone {
        action: ActionKind,
        target: Target,
        result: Result<(), EngineError>,
    },
}

/// Everything that reaches `App::update`.
#[derive(Debug)]
pub enum Msg {
    Action(Action),
    Engine(EngineEvent),
    /// The refresh interval elapsed.
    Tick,
}

/// Work for the runtime to perform. `update` returns these instead of doing I/O,
/// which is what keeps it synchronous and pure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    LoadInfo,
    LoadContainers { all: bool },
    LoadImages { all: bool },
    Perform { action: ActionKind, target: Target },
}
