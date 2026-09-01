use std::fmt;

/// Full container ID as reported by the engine (64 hex chars in practice).
/// The field is private: the only way in is `From<String>`, at the engine boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContainerId(String);

impl ContainerId {
    /// The 12-char prefix docker/podman print in `ps` output.
    pub fn short(&self) -> &str {
        self.0.get(..12).unwrap_or(&self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for ContainerId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Full ID, never truncated -- errors and logs must stay unambiguous.
/// Call `short()` explicitly where a table cell needs it.
impl fmt::Display for ContainerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Full image ID as the engine reports it, algorithm-prefixed:
/// `sha256:5d0da3dc9764...`. Stored verbatim, because that is the form the API
/// accepts back for inspect/remove; the prefix is stripped only for display.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImageId(String);

impl ImageId {
    /// The 12 hex chars `docker images` prints under IMAGE ID.
    pub fn short(&self) -> &str {
        let hex = self.hex();
        hex.get(..12).unwrap_or(hex)
    }

    /// Digest hex with any algorithm prefix removed. Degrades to the whole
    /// string when there is no prefix, so a bare hex ID still displays.
    pub fn hex(&self) -> &str {
        self.0
            .split_once(':')
            .map_or(self.0.as_str(), |(_, hex)| hex)
    }

    /// The algorithm, when the ID carries one (`sha256`).
    pub fn algorithm(&self) -> Option<&str> {
        self.0.split_once(':').map(|(alg, _)| alg)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for ImageId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl fmt::Display for ImageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cid(s: &str) -> ContainerId {
        ContainerId::from(s.to_owned())
    }

    fn iid(s: &str) -> ImageId {
        ImageId::from(s.to_owned())
    }

    #[test]
    fn container_short_takes_the_first_twelve_chars() {
        assert_eq!(cid("3320b75965a8f1c2d3e4").short(), "3320b75965a8");
    }

    #[test]
    fn container_short_returns_a_stub_id_whole_rather_than_panicking() {
        assert_eq!(cid("3320b7").short(), "3320b7");
    }

    #[test]
    fn container_display_is_the_full_id_so_errors_stay_greppable() {
        let id = cid("3320b75965a8f1c2d3e4");
        assert_eq!(id.to_string(), "3320b75965a8f1c2d3e4");
        assert_eq!(id.as_str(), "3320b75965a8f1c2d3e4");
    }

    #[test]
    fn image_short_strips_the_algorithm_prefix() {
        let id = iid("sha256:5d0da3dc976460b7");
        assert_eq!(id.short(), "5d0da3dc9764");
        assert_eq!(id.algorithm(), Some("sha256"));
        assert_eq!(id.hex(), "5d0da3dc976460b7");
    }

    #[test]
    fn image_short_handles_a_bare_hex_id() {
        let id = iid("5d0da3dc976460b7");
        assert_eq!(id.short(), "5d0da3dc9764");
        assert_eq!(id.algorithm(), None);
    }

    #[test]
    fn image_short_never_panics_on_a_stub_id() {
        assert_eq!(iid("sha256:abc").short(), "abc");
    }

    #[test]
    fn image_display_keeps_the_prefix_because_the_api_wants_it_back() {
        assert_eq!(iid("sha256:abc").to_string(), "sha256:abc");
    }
}
