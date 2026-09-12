use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

use chrono::{DateTime, Utc};

use super::{ActionKind, ContainerId, ImageId};

/// Where a value sits on the "is this fine?" scale. A domain-level stand-in for
/// colour, so `domain` never imports ratatui; `ui::theme` owns the mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Good,
    Warn,
    Bad,
    Neutral,
}

/// Transport protocol of a published port. The API's port type is exactly these
/// three; an absent or empty value means tcp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Protocol {
    #[default]
    Tcp,
    Udp,
    Sctp,
}

/// A protocol string the engine sent that jikura does not know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownProtocol(pub String);

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
            Self::Sctp => "sctp",
        })
    }
}

impl FromStr for Protocol {
    type Err = UnknownProtocol;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "tcp" => Ok(Self::Tcp),
            "udp" => Ok(Self::Udp),
            "sctp" => Ok(Self::Sctp),
            other => Err(UnknownProtocol(other.to_owned())),
        }
    }
}

/// One entry from a container's port list.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PortMapping {
    /// `None` when the port is exposed but not published to the host.
    pub host_ip: Option<IpAddr>,
    pub host_port: Option<u16>,
    pub container_port: u16,
    pub protocol: Protocol,
}

impl PortMapping {
    /// True when the port is reachable from the host.
    pub fn is_published(&self) -> bool {
        self.host_port.is_some()
    }
}

/// Docker-style: `0.0.0.0:8080->80/tcp` published, `80/tcp` merely exposed.
impl fmt::Display for PortMapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(host_port) = self.host_port {
            if let Some(ip) = self.host_ip {
                write!(f, "{ip}:")?;
            }
            write!(f, "{host_port}->")?;
        }
        write!(f, "{}/{}", self.container_port, self.protocol)
    }
}

/// Lifecycle state of a container.
///
/// Payload-free on purpose: the list endpoint carries neither `StartedAt` nor
/// `ExitCode` (those need an inspect per container), and `Container::status_text`
/// already holds the engine's own human summary. `Unknown` corresponds to the
/// API's empty state value -- an unrecognized *string* cannot reach us, because
/// bollard's generated enum has no catch-all and fails the whole response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainerState {
    Created,
    Running,
    Paused,
    Restarting,
    Exited,
    Dead,
    Removing,
    Unknown,
}

impl ContainerState {
    /// Canonical engine spelling, matching the API's `State` field.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Restarting => "restarting",
            Self::Exited => "exited",
            Self::Dead => "dead",
            Self::Removing => "removing",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// Has a process at all -- suspended or bouncing included. This is what the
    /// UI means by "up", and what the default filter keeps when --all is off.
    pub fn is_up(&self) -> bool {
        matches!(self, Self::Running | Self::Paused | Self::Restarting)
    }

    /// A state the engine is actively moving through: worth re-polling sooner.
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Restarting | Self::Removing)
    }

    /// Running first, then suspended, then stopped -- a stable table order that
    /// does not reshuffle as `status_text` ticks over.
    pub fn sort_rank(&self) -> u8 {
        match self {
            Self::Running => 0,
            Self::Restarting => 1,
            Self::Paused => 2,
            Self::Created => 3,
            Self::Exited => 4,
            Self::Dead => 5,
            Self::Removing => 6,
            Self::Unknown => 7,
        }
    }

    pub fn severity(&self) -> Severity {
        match self {
            Self::Running => Severity::Good,
            Self::Paused | Self::Restarting | Self::Removing => Severity::Warn,
            Self::Dead => Severity::Bad,
            Self::Created | Self::Exited | Self::Unknown => Severity::Neutral,
        }
    }

    /// Advisory only: greys out entries in the action menu. The engine stays the
    /// authority -- an action it rejects still round-trips and reports its error.
    pub fn allows(&self, action: ActionKind) -> bool {
        use ActionKind::*;
        match (self, action) {
            // An image action is never a container action.
            (_, RemoveImage { .. }) => false,
            // Mid-removal or unintelligible: refuse everything.
            (Self::Removing | Self::Unknown, _) => false,
            (_, RemoveContainer { force: true }) => true,
            (Self::Created | Self::Exited, Start) => true,
            (Self::Running | Self::Restarting | Self::Paused, Stop | Kill) => true,
            (
                Self::Created | Self::Exited | Self::Running | Self::Restarting | Self::Paused,
                Restart,
            ) => true,
            (Self::Running, Pause) => true,
            (Self::Running, Enter) => true,
            (Self::Paused, Unpause) => true,
            (Self::Created | Self::Exited | Self::Dead, RemoveContainer { force: false }) => true,
            // Deny by default; the engine corrects us if we are wrong.
            _ => false,
        }
    }
}

impl fmt::Display for ContainerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A container as the list endpoint describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Container {
    pub id: ContainerId,
    /// Engine-reported names with the historic leading `/` stripped.
    pub names: Vec<String>,
    /// The image reference as given at creation. May be a tag
    /// (`docker.io/library/ubuntu:latest`) or, if that tag was since removed,
    /// a digest -- so it stays a raw string rather than a parsed `ImageRef`.
    pub image: String,
    pub image_id: ImageId,
    pub command: String,
    pub created: DateTime<Utc>,
    pub state: ContainerState,
    /// The engine's human summary, e.g. `Exited (137) 2 hours ago`.
    pub status_text: String,
    pub ports: Vec<PortMapping>,
    pub labels: BTreeMap<String, String>,
}

impl Container {
    /// First name, falling back to the short ID for an unnamed container.
    pub fn display_name(&self) -> &str {
        self.names
            .first()
            .map_or_else(|| self.id.short(), String::as_str)
    }

    /// The image column: a digest reference shortened the way `docker ps` does.
    /// A tag is left alone -- only a bare `algorithm:hex` reference is cut down.
    pub fn image_label(&self) -> &str {
        match self.image.split_once(':') {
            Some((alg, hex)) if is_digest_algorithm(alg) => hex.get(..12).unwrap_or(hex),
            _ => &self.image,
        }
    }

    /// Comma-separated port list for the table cell.
    pub fn ports_summary(&self) -> String {
        self.ports
            .iter()
            .map(PortMapping::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Distinguishes `sha256:abc...` (a digest reference) from `nginx:latest`
/// (a tag) -- both are `name:value` to a naive split.
fn is_digest_algorithm(alg: &str) -> bool {
    matches!(alg, "sha256" | "sha512")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container(state: ContainerState, names: Vec<&str>, image: &str) -> Container {
        Container {
            id: ContainerId::from("3320b75965a8f1c2".to_owned()),
            names: names.into_iter().map(str::to_owned).collect(),
            image: image.to_owned(),
            image_id: ImageId::from("sha256:5d0da3dc976460b7".to_owned()),
            command: "/bin/sh".to_owned(),
            created: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            state,
            status_text: "Up 2 hours".to_owned(),
            ports: Vec::new(),
            labels: BTreeMap::new(),
        }
    }

    #[test]
    fn protocol_parses_the_engines_spellings_and_treats_empty_as_tcp() {
        assert_eq!("tcp".parse::<Protocol>(), Ok(Protocol::Tcp));
        assert_eq!("UDP".parse::<Protocol>(), Ok(Protocol::Udp));
        assert_eq!("sctp".parse::<Protocol>(), Ok(Protocol::Sctp));
        assert_eq!("".parse::<Protocol>(), Ok(Protocol::Tcp));
    }

    #[test]
    fn protocol_rejects_an_unknown_spelling() {
        assert_eq!(
            "quic".parse::<Protocol>(),
            Err(UnknownProtocol("quic".to_owned()))
        );
    }

    #[test]
    fn published_port_renders_like_docker_ps() {
        let p = PortMapping {
            host_ip: Some("0.0.0.0".parse().unwrap()),
            host_port: Some(8080),
            container_port: 80,
            protocol: Protocol::Tcp,
        };
        assert!(p.is_published());
        assert_eq!(p.to_string(), "0.0.0.0:8080->80/tcp");
    }

    #[test]
    fn exposed_but_unpublished_port_renders_without_a_host_side() {
        let p = PortMapping {
            host_ip: None,
            host_port: None,
            container_port: 5432,
            protocol: Protocol::Udp,
        };
        assert!(!p.is_published());
        assert_eq!(p.to_string(), "5432/udp");
    }

    #[test]
    fn running_is_up_but_a_paused_container_is_up_without_running() {
        assert!(ContainerState::Running.is_running());
        assert!(ContainerState::Running.is_up());
        assert!(!ContainerState::Paused.is_running());
        assert!(ContainerState::Paused.is_up());
        assert!(!ContainerState::Exited.is_up());
    }

    #[test]
    fn restarting_and_removing_are_transient() {
        assert!(ContainerState::Restarting.is_transient());
        assert!(ContainerState::Removing.is_transient());
        assert!(!ContainerState::Running.is_transient());
    }

    #[test]
    fn sort_rank_puts_running_first_and_unknown_last() {
        let mut states = vec![
            ContainerState::Exited,
            ContainerState::Unknown,
            ContainerState::Running,
            ContainerState::Paused,
        ];
        states.sort_by_key(ContainerState::sort_rank);
        assert_eq!(
            states,
            vec![
                ContainerState::Running,
                ContainerState::Paused,
                ContainerState::Exited,
                ContainerState::Unknown,
            ]
        );
    }

    #[test]
    fn severity_flags_running_good_and_dead_bad() {
        assert_eq!(ContainerState::Running.severity(), Severity::Good);
        assert_eq!(ContainerState::Paused.severity(), Severity::Warn);
        assert_eq!(ContainerState::Dead.severity(), Severity::Bad);
        assert_eq!(ContainerState::Exited.severity(), Severity::Neutral);
    }

    #[test]
    fn a_stopped_container_can_start_but_not_stop() {
        let s = ContainerState::Exited;
        assert!(s.allows(ActionKind::Start));
        assert!(!s.allows(ActionKind::Stop));
        assert!(!s.allows(ActionKind::Pause));
        assert!(s.allows(ActionKind::RemoveContainer { force: false }));
    }

    #[test]
    fn a_running_container_can_stop_pause_and_kill_but_not_start() {
        let s = ContainerState::Running;
        assert!(!s.allows(ActionKind::Start));
        assert!(s.allows(ActionKind::Stop));
        assert!(s.allows(ActionKind::Kill));
        assert!(s.allows(ActionKind::Pause));
        assert!(!s.allows(ActionKind::Unpause));
    }

    #[test]
    fn only_running_containers_can_be_entered() {
        for state in [
            ContainerState::Created,
            ContainerState::Running,
            ContainerState::Paused,
            ContainerState::Restarting,
            ContainerState::Exited,
            ContainerState::Dead,
            ContainerState::Removing,
            ContainerState::Unknown,
        ] {
            assert_eq!(
                state.allows(ActionKind::Enter),
                state == ContainerState::Running,
                "{state}"
            );
        }
    }

    #[test]
    fn only_a_paused_container_can_unpause() {
        assert!(ContainerState::Paused.allows(ActionKind::Unpause));
        assert!(!ContainerState::Running.allows(ActionKind::Unpause));
    }

    #[test]
    fn a_running_container_needs_force_to_be_removed() {
        let s = ContainerState::Running;
        assert!(!s.allows(ActionKind::RemoveContainer { force: false }));
        assert!(s.allows(ActionKind::RemoveContainer { force: true }));
    }

    #[test]
    fn a_container_being_removed_allows_nothing() {
        let s = ContainerState::Removing;
        assert!(ActionKind::ALL.iter().all(|&a| !s.allows(a)));
    }

    #[test]
    fn container_actions_never_apply_to_images() {
        for state in [ContainerState::Running, ContainerState::Exited] {
            assert!(!state.allows(ActionKind::RemoveImage { force: false }));
            assert!(!state.allows(ActionKind::RemoveImage { force: true }));
        }
    }

    #[test]
    fn display_name_falls_back_to_the_short_id() {
        let named = container(ContainerState::Running, vec!["web"], "nginx:latest");
        assert_eq!(named.display_name(), "web");
        let unnamed = container(ContainerState::Running, vec![], "nginx:latest");
        assert_eq!(unnamed.display_name(), "3320b75965a8");
    }

    #[test]
    fn image_label_keeps_a_tag_but_shortens_a_digest_reference() {
        let tagged = container(ContainerState::Running, vec!["web"], "nginx:latest");
        assert_eq!(tagged.image_label(), "nginx:latest");
        let untagged = container(
            ContainerState::Running,
            vec!["web"],
            "sha256:5d0da3dc976460b7c8e9",
        );
        assert_eq!(untagged.image_label(), "5d0da3dc9764");
    }

    #[test]
    fn ports_summary_joins_entries_and_is_empty_when_there_are_none() {
        let mut c = container(ContainerState::Running, vec!["web"], "nginx:latest");
        assert_eq!(c.ports_summary(), "");
        c.ports = vec![
            PortMapping {
                host_ip: Some("0.0.0.0".parse().unwrap()),
                host_port: Some(8080),
                container_port: 80,
                protocol: Protocol::Tcp,
            },
            PortMapping {
                host_ip: None,
                host_port: None,
                container_port: 443,
                protocol: Protocol::Tcp,
            },
        ];
        assert_eq!(c.ports_summary(), "0.0.0.0:8080->80/tcp, 443/tcp");
    }
}
