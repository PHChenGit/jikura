use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};

use super::ImageId;

/// A byte count that renders the way docker/podman render sizes: decimal units,
/// four significant digits (`2.91GB`, `20.44MB`, `512B`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct ByteSize(pub u64);

impl fmt::Display for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const UNITS: [&str; 6] = ["B", "kB", "MB", "GB", "TB", "PB"];
        let mut value = self.0 as f64;
        let mut unit = 0;
        while value >= 1000.0 && unit < UNITS.len() - 1 {
            value /= 1000.0;
            unit += 1;
        }
        if unit == 0 {
            return write!(f, "{}B", self.0);
        }
        // Four significant digits, trailing zeros trimmed: 1.093GB, 20.44MB, 512MB.
        let magnitude = value.log10().floor() as i32;
        let decimals = (3 - magnitude).clamp(0, 3) as usize;
        let mut text = format!("{value:.decimals$}");
        if text.contains('.') {
            text = text.trim_end_matches('0').trim_end_matches('.').to_owned();
        }
        write!(f, "{text}{}", UNITS[unit])
    }
}

/// A `repository[:tag]` reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImageRef {
    pub repository: String,
    pub tag: Option<String>,
}

impl ImageRef {
    /// Splits a repo-tag string. The trailing colon only introduces a tag when
    /// no `/` follows it -- otherwise it is a registry port, as in
    /// `localhost:5000/team/app`.
    pub fn parse(s: &str) -> Self {
        match s.rsplit_once(':') {
            Some((repository, tag)) if !tag.contains('/') => Self {
                repository: repository.to_owned(),
                tag: Some(tag.to_owned()),
            },
            _ => Self {
                repository: s.to_owned(),
                tag: None,
            },
        }
    }
}

impl fmt::Display for ImageRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.tag {
            Some(tag) => write!(f, "{}:{tag}", self.repository),
            None => f.write_str(&self.repository),
        }
    }
}

/// An image as the list endpoint describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub id: ImageId,
    /// Empty for a dangling image. `<none>:<none>` entries are dropped at the
    /// engine boundary rather than carried in here.
    pub repo_tags: Vec<ImageRef>,
    pub repo_digests: Vec<String>,
    pub created: DateTime<Utc>,
    pub size: ByteSize,
    /// `None` when the engine did not compute it (the API sends `-1`).
    pub shared_size: Option<ByteSize>,
    /// How many containers use this image; `None` when not computed (`-1`).
    pub containers: Option<i64>,
    pub labels: BTreeMap<String, String>,
}

impl Image {
    /// No tags left: the image is only reachable by ID.
    pub fn is_dangling(&self) -> bool {
        self.repo_tags.is_empty()
    }

    /// The tag shown in the REPOSITORY:TAG column.
    pub fn primary_ref(&self) -> Option<&ImageRef> {
        self.repo_tags.first()
    }

    /// Column text, `<none>` for a dangling image -- matching `docker images`.
    pub fn name_label(&self) -> String {
        self.primary_ref()
            .map_or_else(|| "<none>".to_owned(), ImageRef::to_string)
    }

    /// Tags beyond the primary one, so the UI can hint `+2 more`.
    pub fn extra_tag_count(&self) -> usize {
        self.repo_tags.len().saturating_sub(1)
    }

    /// Whether any container uses it; `None` when the engine did not say.
    pub fn is_in_use(&self) -> Option<bool> {
        self.containers.map(|count| count > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(tags: &[&str], containers: Option<i64>) -> Image {
        Image {
            id: ImageId::from("sha256:5d0da3dc976460b7".to_owned()),
            repo_tags: tags.iter().map(|t| ImageRef::parse(t)).collect(),
            repo_digests: Vec::new(),
            created: DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            size: ByteSize(2_910_000_000),
            shared_size: None,
            containers,
            labels: BTreeMap::new(),
        }
    }

    #[test]
    fn bytes_below_a_kilobyte_render_exactly() {
        assert_eq!(ByteSize(0).to_string(), "0B");
        assert_eq!(ByteSize(512).to_string(), "512B");
        assert_eq!(ByteSize(999).to_string(), "999B");
    }

    #[test]
    fn sizes_use_decimal_units_like_the_docker_cli() {
        assert_eq!(ByteSize(1_000).to_string(), "1kB");
        assert_eq!(ByteSize(20_440_000).to_string(), "20.44MB");
        assert_eq!(ByteSize(1_093_000_000).to_string(), "1.093GB");
        assert_eq!(ByteSize(2_910_000_000).to_string(), "2.91GB");
    }

    #[test]
    fn sizes_keep_four_significant_digits_at_a_unit_boundary() {
        assert_eq!(ByteSize(999_900).to_string(), "999.9kB");
    }

    #[test]
    fn parse_splits_a_tag_from_a_repository() {
        let r = ImageRef::parse("nginx:latest");
        assert_eq!(r.repository, "nginx");
        assert_eq!(r.tag.as_deref(), Some("latest"));
        assert_eq!(r.to_string(), "nginx:latest");
    }

    #[test]
    fn parse_leaves_an_untagged_reference_alone() {
        let r = ImageRef::parse("nginx");
        assert_eq!(r.repository, "nginx");
        assert_eq!(r.tag, None);
        assert_eq!(r.to_string(), "nginx");
    }

    #[test]
    fn parse_does_not_mistake_a_registry_port_for_a_tag() {
        let r = ImageRef::parse("localhost:5000/team/app");
        assert_eq!(r.repository, "localhost:5000/team/app");
        assert_eq!(r.tag, None);
    }

    #[test]
    fn parse_finds_the_tag_after_a_registry_port() {
        let r = ImageRef::parse("localhost:5000/team/app:1.2");
        assert_eq!(r.repository, "localhost:5000/team/app");
        assert_eq!(r.tag.as_deref(), Some("1.2"));
    }

    #[test]
    fn a_tagless_image_is_dangling_and_labelled_none() {
        let img = image(&[], Some(0));
        assert!(img.is_dangling());
        assert_eq!(img.name_label(), "<none>");
        assert_eq!(img.primary_ref(), None);
    }

    #[test]
    fn a_tagged_image_reports_its_first_tag_and_counts_the_rest() {
        let img = image(&["nginx:latest", "nginx:1.27", "nginx:stable"], Some(2));
        assert!(!img.is_dangling());
        assert_eq!(img.name_label(), "nginx:latest");
        assert_eq!(img.extra_tag_count(), 2);
    }

    #[test]
    fn usage_is_unknown_when_the_engine_did_not_count() {
        assert_eq!(image(&["a:1"], None).is_in_use(), None);
        assert_eq!(image(&["a:1"], Some(0)).is_in_use(), Some(false));
        assert_eq!(image(&["a:1"], Some(3)).is_in_use(), Some(true));
    }
}
