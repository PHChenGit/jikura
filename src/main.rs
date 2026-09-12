//! Wiring only: parse arguments, connect, run the event loop.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use crossterm::event;

use jikura::app::{App, Command, EngineEvent, Msg};
use jikura::cli::Args;
use jikura::config::Config;
use jikura::domain::{ActionKind, Target};
use jikura::engine::{BollardEngine, Engine};
use jikura::enter;
use jikura::runtime::Runtime;
use jikura::tui;
use jikura::ui::{self, UiState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = Config::load(args.config.as_deref())?;

    // --host is sugar for DOCKER_HOST, which is what bollard reads.
    if let Some(host) = &args.host {
        // SAFETY: single-threaded at this point; no task has been spawned yet.
        unsafe { std::env::set_var("DOCKER_HOST", host) };
    }

    let engine = BollardEngine::connect().context("could not use the engine endpoint")?;
    let endpoint = engine.endpoint().to_owned();
    let terminal = tui::init().context("could not set up the terminal")?;
    let result = run(terminal, engine, &args, &config, &endpoint).await;
    tui::restore();
    result
}

async fn run<E: Engine>(
    mut terminal: ratatui::DefaultTerminal,
    engine: E,
    args: &Args,
    config: &Config,
    endpoint: &str,
) -> anyhow::Result<()> {
    let (runtime, mut engine_events) = Runtime::new(Arc::new(engine));
    let mut app = App::new(args.all);
    let mut ui_state = UiState::new();
    // Read only on this thread: a background EventStream reader can consume
    // keystrokes that belong to the foreground docker exec session.
    let mut input_tick = tokio::time::interval(Duration::from_millis(20));
    input_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut ticker = tokio::time::interval(Duration::from_secs(args.refresh));

    runtime.spawn_all(app.startup());
    // The first tick fires immediately; startup already asked for the data.
    ticker.tick().await;

    let mut redraw = true;
    while !app.should_quit {
        if redraw {
            terminal.draw(|frame| ui::draw(frame, &app, &mut ui_state))?;
            redraw = false;
        }

        // Whichever arrives first: a keystroke, a finished engine call, or the
        // refresh interval. Nothing here awaits the engine, so a slow `stop`
        // cannot freeze the redraw.
        let msg = tokio::select! {
            _ = ticker.tick() => Some(Msg::Tick),
            _ = input_tick.tick() => {
                if event::poll(Duration::ZERO).context("terminal input failed")? {
                    let event = event::read().context("terminal input failed")?;
                    redraw = true;
                    ui::msg_for(&event)
                } else {
                    None
                }
            },
            event = engine_events.recv() => event.map(Msg::Engine),
        };

        if let Some(msg) = msg {
            redraw = true;
            let commands = app.update(msg);
            for command in commands {
                if let Command::Perform {
                    action: ActionKind::Enter,
                    target: Target::Container(ref id),
                } = command
                {
                    let target = Target::Container(id.clone());
                    let settings = config.for_container(&app.label_for(&target));
                    let result = tui::with_suspended(&mut terminal, || {
                        let result = enter::run(endpoint, id, &settings);
                        if let Err(err) = &result {
                            // Keep the CLI's own diagnostic visible before returning.
                            eprintln!("{err}\nPress Enter to return to Jikura.");
                            let _ = std::io::stdin().read_line(&mut String::new());
                        }
                        result
                    })?;
                    let reload = app.update(Msg::Engine(EngineEvent::ActionDone {
                        action: ActionKind::Enter,
                        target,
                        result,
                    }));
                    runtime.spawn_all(reload);
                    ticker.reset();
                } else {
                    runtime.spawn(command);
                }
            }
        }
    }

    Ok(())
}
