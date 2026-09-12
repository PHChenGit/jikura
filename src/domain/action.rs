use super::{ContainerId, ImageId};

/// What an action acts on. Carries the full ID, never a name: names are not
/// unique and can change under us between a refresh and a keypress.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Target {
    Container(ContainerId),
    Image(ImageId),
}

impl Target {
    /// The reference to send to the API.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Container(id) => id.as_str(),
            Self::Image(id) => id.as_str(),
        }
    }

    /// Short form for messages.
    pub fn short(&self) -> &str {
        match self {
            Self::Container(id) => id.short(),
            Self::Image(id) => id.short(),
        }
    }
}

/// An engine operation jikura can ask for. Lives in `domain` rather than `app`
/// because both the `Engine` port and `ContainerState::allows` speak it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionKind {
    Start,
    Stop,
    Restart,
    Kill,
    Pause,
    Unpause,
    Enter,
    /// `force` also removes a running container (API: `?force=1`).
    RemoveContainer {
        force: bool,
    },
    RemoveImage {
        force: bool,
    },
}

impl ActionKind {
    /// Every variant, for exhaustive test matrices and the action menu.
    pub const ALL: &'static [ActionKind] = &[
        ActionKind::Start,
        ActionKind::Stop,
        ActionKind::Restart,
        ActionKind::Kill,
        ActionKind::Pause,
        ActionKind::Unpause,
        ActionKind::Enter,
        ActionKind::RemoveContainer { force: false },
        ActionKind::RemoveContainer { force: true },
        ActionKind::RemoveImage { force: false },
        ActionKind::RemoveImage { force: true },
    ];

    /// Menu label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Stop => "Stop",
            Self::Restart => "Restart",
            Self::Kill => "Kill",
            Self::Pause => "Pause",
            Self::Unpause => "Unpause",
            Self::Enter => "Enter",
            Self::RemoveContainer { force: false } | Self::RemoveImage { force: false } => "Remove",
            Self::RemoveContainer { force: true } | Self::RemoveImage { force: true } => {
                "Remove (force)"
            }
        }
    }

    /// Present tense, for the in-flight indicator ("stopping...").
    pub fn present_participle(&self) -> &'static str {
        match self {
            Self::Start => "starting",
            Self::Stop => "stopping",
            Self::Restart => "restarting",
            Self::Kill => "killing",
            Self::Pause => "pausing",
            Self::Unpause => "unpausing",
            Self::Enter => "entering",
            Self::RemoveContainer { .. } | Self::RemoveImage { .. } => "removing",
        }
    }

    /// Irreversible, so the UI demands a confirmation first.
    pub fn is_destructive(&self) -> bool {
        matches!(
            self,
            Self::RemoveContainer { .. } | Self::RemoveImage { .. }
        )
    }

    /// Whether this acts on an image rather than a container.
    pub fn targets_image(&self) -> bool {
        matches!(self, Self::RemoveImage { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removals_are_destructive_and_nothing_else_is() {
        let destructive: Vec<_> = ActionKind::ALL
            .iter()
            .filter(|a| a.is_destructive())
            .copied()
            .collect();
        assert_eq!(
            destructive,
            vec![
                ActionKind::RemoveContainer { force: false },
                ActionKind::RemoveContainer { force: true },
                ActionKind::RemoveImage { force: false },
                ActionKind::RemoveImage { force: true },
            ]
        );
    }

    #[test]
    fn kill_is_not_destructive_because_the_container_survives() {
        assert!(!ActionKind::Kill.is_destructive());
    }

    #[test]
    fn only_image_removal_targets_an_image() {
        assert!(ActionKind::RemoveImage { force: false }.targets_image());
        assert!(!ActionKind::RemoveContainer { force: false }.targets_image());
        assert!(!ActionKind::Stop.targets_image());
    }

    #[test]
    fn labels_distinguish_forced_removal() {
        assert_eq!(
            ActionKind::RemoveContainer { force: false }.label(),
            "Remove"
        );
        assert_eq!(
            ActionKind::RemoveContainer { force: true }.label(),
            "Remove (force)"
        );
        assert_eq!(ActionKind::Stop.present_participle(), "stopping");
    }
}
