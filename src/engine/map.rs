//! The only module that knows bollard's API types exist. Everything above it
//! sees `domain` values.

use std::collections::BTreeMap;

use bollard::models::{
    ContainerSummary, ContainerSummaryStateEnum, ImageSummary, PortSummary, PortSummaryTypeEnum,
};
use chrono::{DateTime, Utc};

use crate::domain::{
    ByteSize, Container, ContainerId, ContainerState, Image, ImageId, ImageRef, PortMapping,
    Protocol,
};

/// Maps one list entry. `None` when the entry has no ID: it could not be
/// addressed for any action, so it is dropped rather than failing the refresh.
pub fn container(summary: ContainerSummary) -> Option<Container> {
    let id = ContainerId::from(summary.id?);

    let mut ports: Vec<PortMapping> = summary
        .ports
        .unwrap_or_default()
        .into_iter()
        .map(port)
        .collect();
    // Sorted by the container-side port, not by the derived field order, so a
    // published low port stays above an unpublished high one. Both engines
    // repeat a mapping per address family, hence the dedup.
    ports.sort_by_key(|p| (p.container_port, p.protocol));
    ports.dedup();

    Some(Container {
        id,
        names: summary
            .names
            .unwrap_or_default()
            .into_iter()
            .map(|name| name.trim_start_matches('/').to_owned())
            .collect(),
        image: summary.image.unwrap_or_default(),
        image_id: ImageId::from(summary.image_id.unwrap_or_default()),
        command: summary.command.unwrap_or_default(),
        created: timestamp(summary.created.unwrap_or_default()),
        state: state(summary.state),
        status_text: summary.status.unwrap_or_default(),
        ports,
        labels: summary.labels.unwrap_or_default().into_iter().collect(),
    })
}

pub fn image(summary: ImageSummary) -> Image {
    Image {
        id: ImageId::from(summary.id),
        repo_tags: summary
            .repo_tags
            .into_iter()
            .filter(|tag| !is_placeholder(tag))
            .map(|tag| ImageRef::parse(&tag))
            .collect(),
        repo_digests: summary
            .repo_digests
            .into_iter()
            .filter(|digest| !is_placeholder(digest))
            .collect(),
        created: timestamp(summary.created),
        size: ByteSize(summary.size.max(0) as u64),
        shared_size: computed(summary.shared_size).map(|value| ByteSize(value as u64)),
        containers: computed(summary.containers),
        labels: summary.labels.into_iter().collect::<BTreeMap<_, _>>(),
    }
}

fn state(raw: Option<ContainerSummaryStateEnum>) -> ContainerState {
    use ContainerSummaryStateEnum as Api;
    match raw {
        Some(Api::CREATED) => ContainerState::Created,
        Some(Api::RUNNING) => ContainerState::Running,
        Some(Api::PAUSED) => ContainerState::Paused,
        Some(Api::RESTARTING) => ContainerState::Restarting,
        Some(Api::EXITED) => ContainerState::Exited,
        Some(Api::REMOVING) => ContainerState::Removing,
        Some(Api::DEAD) => ContainerState::Dead,
        Some(Api::EMPTY) | None => ContainerState::Unknown,
    }
}

fn port(raw: PortSummary) -> PortMapping {
    PortMapping {
        // An address we cannot parse is dropped; the mapping is still published.
        host_ip: raw.ip.as_deref().and_then(|ip| ip.parse().ok()),
        host_port: raw.public_port,
        container_port: raw.private_port,
        protocol: protocol(raw.typ),
    }
}

fn protocol(raw: Option<PortSummaryTypeEnum>) -> Protocol {
    use PortSummaryTypeEnum as Api;
    match raw {
        Some(Api::UDP) => Protocol::Udp,
        Some(Api::SCTP) => Protocol::Sctp,
        Some(Api::TCP | Api::EMPTY) | None => Protocol::Tcp,
    }
}

/// Epoch seconds to a timestamp, clamping nonsense to the epoch rather than
/// dropping a row over a cosmetic field.
fn timestamp(secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(secs, 0).unwrap_or(DateTime::UNIX_EPOCH)
}

/// The API sends `-1` for counters it did not compute.
fn computed(value: i64) -> Option<i64> {
    (value >= 0).then_some(value)
}

/// `<none>:<none>` and `<none>@<none>` are the API's way of saying "untagged",
/// not real references.
fn is_placeholder(reference: &str) -> bool {
    reference.starts_with("<none>")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> ContainerSummary {
        ContainerSummary {
            id: Some("3320b75965a8f1c2d3e4".to_owned()),
            names: Some(vec!["/web".to_owned(), "/web-alias".to_owned()]),
            image: Some("nginx:latest".to_owned()),
            image_id: Some("sha256:5d0da3dc976460b7".to_owned()),
            command: Some("nginx -g daemon off;".to_owned()),
            created: Some(1_700_000_000),
            state: Some(ContainerSummaryStateEnum::RUNNING),
            status: Some("Up 2 hours".to_owned()),
            ..Default::default()
        }
    }

    fn image_summary() -> ImageSummary {
        ImageSummary {
            id: "sha256:5d0da3dc976460b7".to_owned(),
            repo_tags: vec!["nginx:latest".to_owned()],
            repo_digests: vec!["nginx@sha256:abc".to_owned()],
            created: 1_700_000_000,
            size: 2_910_000_000,
            shared_size: -1,
            containers: -1,
            ..Default::default()
        }
    }

    #[test]
    fn maps_the_fields_the_table_shows() {
        let c = container(summary()).expect("has an id");
        assert_eq!(c.id.short(), "3320b75965a8");
        assert_eq!(c.image, "nginx:latest");
        assert_eq!(c.image_id.short(), "5d0da3dc9764");
        assert_eq!(c.command, "nginx -g daemon off;");
        assert_eq!(c.state, ContainerState::Running);
        assert_eq!(c.status_text, "Up 2 hours");
        assert_eq!(c.created.timestamp(), 1_700_000_000);
    }

    #[test]
    fn strips_the_historic_leading_slash_from_names() {
        let c = container(summary()).unwrap();
        assert_eq!(c.names, vec!["web".to_owned(), "web-alias".to_owned()]);
        assert_eq!(c.display_name(), "web");
    }

    #[test]
    fn an_entry_without_an_id_is_dropped_not_fatal() {
        let mut raw = summary();
        raw.id = None;
        assert!(container(raw).is_none());
    }

    #[test]
    fn an_absent_or_empty_state_becomes_unknown() {
        let mut raw = summary();
        raw.state = None;
        assert_eq!(
            container(raw.clone()).unwrap().state,
            ContainerState::Unknown
        );
        raw.state = Some(ContainerSummaryStateEnum::EMPTY);
        assert_eq!(container(raw).unwrap().state, ContainerState::Unknown);
    }

    #[test]
    fn every_api_state_maps_to_a_domain_state() {
        let pairs = [
            (ContainerSummaryStateEnum::CREATED, ContainerState::Created),
            (ContainerSummaryStateEnum::RUNNING, ContainerState::Running),
            (ContainerSummaryStateEnum::PAUSED, ContainerState::Paused),
            (
                ContainerSummaryStateEnum::RESTARTING,
                ContainerState::Restarting,
            ),
            (ContainerSummaryStateEnum::EXITED, ContainerState::Exited),
            (
                ContainerSummaryStateEnum::REMOVING,
                ContainerState::Removing,
            ),
            (ContainerSummaryStateEnum::DEAD, ContainerState::Dead),
        ];
        for (api, expected) in pairs {
            let mut raw = summary();
            raw.state = Some(api);
            assert_eq!(container(raw).unwrap().state, expected, "for {api:?}");
        }
    }

    #[test]
    fn ports_are_sorted_deduped_and_default_to_tcp() {
        let mut raw = summary();
        raw.ports = Some(vec![
            PortSummary {
                ip: None,
                private_port: 443,
                public_port: None,
                typ: Some(PortSummaryTypeEnum::EMPTY),
            },
            PortSummary {
                ip: Some("0.0.0.0".to_owned()),
                private_port: 80,
                public_port: Some(8080),
                typ: Some(PortSummaryTypeEnum::TCP),
            },
            // podman and docker both repeat a mapping per address family
            PortSummary {
                ip: Some("0.0.0.0".to_owned()),
                private_port: 80,
                public_port: Some(8080),
                typ: Some(PortSummaryTypeEnum::TCP),
            },
        ]);
        let c = container(raw).unwrap();
        assert_eq!(c.ports_summary(), "0.0.0.0:8080->80/tcp, 443/tcp");
    }

    #[test]
    fn an_unparseable_host_ip_leaves_the_port_published_without_one() {
        let mut raw = summary();
        raw.ports = Some(vec![PortSummary {
            ip: Some("not-an-ip".to_owned()),
            private_port: 80,
            public_port: Some(8080),
            typ: None,
        }]);
        let c = container(raw).unwrap();
        assert_eq!(c.ports[0].host_ip, None);
        assert!(c.ports[0].is_published());
        assert_eq!(c.ports_summary(), "8080->80/tcp");
    }

    #[test]
    fn a_nonsense_timestamp_falls_back_to_the_epoch() {
        let mut raw = summary();
        raw.created = Some(i64::MAX);
        assert_eq!(container(raw).unwrap().created.timestamp(), 0);
    }

    #[test]
    fn maps_the_image_fields_the_table_shows() {
        let img = image(image_summary());
        assert_eq!(img.id.short(), "5d0da3dc9764");
        assert_eq!(img.name_label(), "nginx:latest");
        assert_eq!(img.size, ByteSize(2_910_000_000));
        assert_eq!(img.created.timestamp(), 1_700_000_000);
        assert_eq!(img.repo_digests, vec!["nginx@sha256:abc".to_owned()]);
    }

    #[test]
    fn uncomputed_counters_become_none_rather_than_minus_one() {
        let img = image(image_summary());
        assert_eq!(img.containers, None);
        assert_eq!(img.shared_size, None);
        assert_eq!(img.is_in_use(), None);
    }

    #[test]
    fn computed_counters_are_kept() {
        let mut raw = image_summary();
        raw.containers = 2;
        raw.shared_size = 1_000_000;
        let img = image(raw);
        assert_eq!(img.containers, Some(2));
        assert_eq!(img.shared_size, Some(ByteSize(1_000_000)));
        assert_eq!(img.is_in_use(), Some(true));
    }

    #[test]
    fn the_none_placeholder_tag_is_dropped_so_the_image_reads_as_dangling() {
        let mut raw = image_summary();
        raw.repo_tags = vec!["<none>:<none>".to_owned()];
        let img = image(raw);
        assert!(img.is_dangling());
        assert_eq!(img.name_label(), "<none>");
    }

    #[test]
    fn multiple_tags_survive_in_order() {
        let mut raw = image_summary();
        raw.repo_tags = vec!["nginx:latest".to_owned(), "nginx:1.27".to_owned()];
        let img = image(raw);
        assert_eq!(img.repo_tags.len(), 2);
        assert_eq!(img.primary_ref(), Some(&ImageRef::parse("nginx:latest")));
        assert_eq!(img.extra_tag_count(), 1);
    }

    #[test]
    fn a_negative_size_clamps_to_zero_instead_of_wrapping() {
        let mut raw = image_summary();
        raw.size = -1;
        assert_eq!(image(raw).size, ByteSize(0));
    }
}
