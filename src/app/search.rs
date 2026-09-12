use crate::domain::{Container, Image};

/// Case-insensitive subsequence matching: characters may have gaps, but must
/// appear in order. Match each field separately so IDs and names cannot combine
/// into an accidental match.
fn fuzzy_matches(text: &str, query: &str) -> bool {
    let text = text.to_lowercase();
    let mut chars = text.chars();
    query
        .to_lowercase()
        .chars()
        .all(|wanted| chars.any(|c| c == wanted))
}

/// IDs match contiguous prefixes, with or without an algorithm such as sha256.
fn id_matches(id: &str, query: &str) -> bool {
    let id = id.to_ascii_lowercase();
    let query = query.to_ascii_lowercase();
    id.starts_with(&query)
        || id
            .split_once(':')
            .is_some_and(|(_, hex)| hex.starts_with(&query))
}

fn digest_ref_matches(reference: &str, query: &str) -> bool {
    match reference.split_once('@') {
        Some((repository, digest)) => {
            fuzzy_matches(repository, query)
                || id_matches(digest, query)
                || reference
                    .to_ascii_lowercase()
                    .starts_with(&query.to_ascii_lowercase())
        }
        None => id_matches(reference, query),
    }
}

pub(super) fn container_matches(container: &Container, query: &str) -> bool {
    // The engine may put a raw image ID in the image-name field.
    let image_matches = if container.image == container.image_id.as_str()
        || container.image == container.image_id.hex()
        || container.image.starts_with("sha256:")
    {
        id_matches(&container.image, query)
    } else if container.image.contains('@') {
        digest_ref_matches(&container.image, query)
    } else {
        fuzzy_matches(&container.image, query)
    };
    id_matches(container.id.as_str(), query)
        || container
            .names
            .iter()
            .any(|name| fuzzy_matches(name, query))
        || image_matches
        || id_matches(container.image_id.as_str(), query)
}

pub(super) fn image_matches(image: &Image, query: &str) -> bool {
    id_matches(image.id.as_str(), query)
        || image
            .repo_tags
            .iter()
            .any(|name| fuzzy_matches(&name.to_string(), query))
        || image
            .repo_digests
            .iter()
            .any(|name| digest_ref_matches(name, query))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_case_insensitive_subsequences_including_unicode() {
        assert!(fuzzy_matches("nginx:latest", "NGX"));
        assert!(fuzzy_matches("répertoire", "RÉP"));
        assert!(fuzzy_matches("anything", ""));
        assert!(!fuzzy_matches("nginx", "xgn"));
        assert!(!fuzzy_matches("web", "ww"));
        assert!(!fuzzy_matches("", "a"));
    }

    #[test]
    fn ids_require_a_contiguous_prefix_with_or_without_the_algorithm() {
        for id in [
            "6d336809823dfg97dwoin12cjbiw",
            "sha256:6d336809823dfg97dwoin12cjbiw",
        ] {
            for query in ["", "6d33", "6D33", "6d336809823dfg97dwoin12cjbiw"] {
                assert!(id_matches(id, query), "{id}: {query}");
            }
            for query in ["dfg97", "6c33", "6d38", "6d336809823dfg97dwoin12cjbiwx"] {
                assert!(!id_matches(id, query), "{id}: {query}");
            }
        }
        assert!(id_matches("sha256:6d336809823", "sha256:6d33"));
        assert!(!id_matches("sha256:6d336809823", "sha256:6d38"));
    }

    #[test]
    fn repository_digests_do_not_allow_fuzzy_matches_inside_the_digest() {
        let reference = "registry/nginx@sha256:6d336809823dfg97dwoin12cjbiw";
        assert!(digest_ref_matches(reference, "NGX"));
        assert!(digest_ref_matches(reference, "6d33"));
        assert!(digest_ref_matches(reference, "registry/nginx@sha256:6d33"));
        assert!(!digest_ref_matches(reference, "dfg97"));
        assert!(!digest_ref_matches(reference, "6d38"));
    }
}
