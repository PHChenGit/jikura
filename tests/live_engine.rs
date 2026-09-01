//! Read-only checks against whatever engine `DOCKER_HOST` points at. Ignored by
//! default so `cargo test` stays hermetic:
//!
//!     cargo test --test live_engine -- --ignored
//!
//! Nothing here mutates the user's containers or images.

use jikura::domain::ContainerState;
use jikura::engine::{BollardEngine, Engine, EngineFlavor};

async fn engine() -> BollardEngine {
    BollardEngine::connect().expect("DOCKER_HOST should be connectable")
}

#[tokio::test]
#[ignore = "needs a live engine socket"]
async fn reports_which_engine_answered() {
    let info = engine()
        .await
        .info()
        .await
        .expect("engine should answer /version");
    assert_ne!(
        info.flavor,
        EngineFlavor::Unknown,
        "engine did not identify itself"
    );
    assert_ne!(info.version, "unknown");
    assert_ne!(info.api_version, "unknown");
}

#[tokio::test]
#[ignore = "needs a live engine socket"]
async fn lists_containers_without_a_decode_failure() {
    let containers = engine()
        .await
        .list_containers(true)
        .await
        .expect("listing all containers should not fail");

    for c in &containers {
        assert!(!c.id.as_str().is_empty(), "mapped container without an id");
        // Unknown means the engine sent a state jikura's bindings do not model.
        assert_ne!(
            c.state,
            ContainerState::Unknown,
            "unmapped state for {}: {}",
            c.display_name(),
            c.status_text
        );
        assert!(!c.image_id.as_str().is_empty());
    }
}

#[tokio::test]
#[ignore = "needs a live engine socket"]
async fn the_default_listing_is_the_running_subset() {
    let e = engine().await;
    let all = e.list_containers(true).await.expect("all");
    let up = e.list_containers(false).await.expect("running only");

    assert!(up.len() <= all.len());
    for c in &up {
        assert!(
            c.state.is_up(),
            "{} is not up but was listed",
            c.display_name()
        );
    }
}

#[tokio::test]
#[ignore = "needs a live engine socket"]
async fn lists_images_with_digests_and_sizes() {
    let images = engine()
        .await
        .list_images(true)
        .await
        .expect("listing images");
    assert!(!images.is_empty(), "this host is expected to have images");

    for img in &images {
        assert!(
            img.id.algorithm().is_some(),
            "image id lost its algorithm prefix: {}",
            img.id
        );
        assert_eq!(img.id.short().len(), 12);
    }
    assert!(
        images.iter().any(|img| img.size.0 > 0),
        "no image reported a size"
    );
}
