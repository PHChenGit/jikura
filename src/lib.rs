//! jikura -- a terminal UI for browsing local docker/podman images and containers.
//!
//! Layering, strictly one-directional:
//!   domain  -- plain data, no engine or terminal types
//!   engine  -- the Engine port, its bollard adapter, and API->domain mapping
//!   app     -- state plus a pure `update`; owns all decisions
//!   ui/tui  -- renders app state; owns no state of its own
pub mod app;
pub mod cli;
pub mod config;
pub mod domain;
pub mod engine;
pub mod enter;
pub mod runtime;
pub mod tui;
pub mod ui;
