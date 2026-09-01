//! Wiring only: parse arguments, connect, run the event loop.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use crossterm::event::EventStream;
use futures_util::StreamExt;

use jikura::app::{App, Msg};
use jikura::cli::Args;
use jikura::engine::{BollardEngine, Engine};
use jikura::runtime::Runtime;
use jikura::tui;
use jikura::ui::{self, UiState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // --host is sugar for DOCKER_HOST, which is what bollard reads.
    if let Some(host) = &args.host {
        // SAFETY: single-threaded at this point; no task has been spawned yet.
        unsafe { std::env::set_var("DOCKER_HOST", host) };
    }

    let engine = BollardEngine::connect().context("could not use the engine endpoint")?;
    let terminal = tui::init().context("could not set up the terminal")?;
    let result = run(terminal, engine, &args).await;
    tui::restore();
    result
}

async fn run<E: Engine>(
    mut terminal: ratatui::DefaultTerminal,
    engine: E,
    args: &Args,
) -> anyhow::Result<()> {
    let (runtime, mut engine_events) = Runtime::new(Arc::new(engine));
    let mut app = App::new(args.all);
    let mut ui_state = UiState::new();
    let mut input = EventStream::new();
    let mut ticker = tokio::time::interval(Duration::from_secs(args.refresh));

    runtime.spawn_all(app.startup());
    // The first tick fires immediately; startup already asked for the data.
    ticker.tick().await;

    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &app, &mut ui_state))?;

        // Whichever arrives first: a keystroke, a finished engine call, or the
        // refresh interval. Nothing here awaits the engine, so a slow `stop`
        // cannot freeze the redraw.
        let msg = tokio::select! {
            _ = ticker.tick() => Some(Msg::Tick),
            event = input.next() => match event {
                Some(Ok(event)) => ui::msg_for(&event),
                // A closed or broken input stream means the terminal is gone.
                Some(Err(err)) => return Err(err).context("terminal input failed"),
                None => break,
            },
            event = engine_events.recv() => event.map(Msg::Engine),
        };

        if let Some(msg) = msg {
            let commands = app.update(msg);
            runtime.spawn_all(commands);
        }
    }

    Ok(())
}
