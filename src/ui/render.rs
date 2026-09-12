//! Draws app state. Holds no state of its own beyond scroll positions, which
//! belong to the widgets rather than to the app.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Cell, Clear, Paragraph, Row, Table, TableState, Tabs};

use crate::app::{App, LoadState, Modal, Tab};
use crate::domain::{Severity, Target};

use super::theme;

/// Shown in the footer when there is nothing more urgent to say.
const KEY_HINTS: &str =
    "j/k move | tab switch pane | x actions | a all | r refresh | ? help | q quit";

/// Widget-owned state that must persist between frames: the scroll offsets.
#[derive(Debug, Default)]
pub struct UiState {
    pub containers: TableState,
    pub images: TableState,
}

impl UiState {
    pub fn new() -> Self {
        Self::default()
    }
}

pub fn draw(frame: &mut Frame, app: &App, ui: &mut UiState) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    draw_header(frame, app, header);
    match app.tab {
        Tab::Containers => draw_containers(frame, app, &mut ui.containers, body),
        Tab::Images => draw_images(frame, app, &mut ui.images, body),
    }
    draw_footer(frame, app, footer);

    if let Some(modal) = &app.modal {
        draw_modal(frame, app, modal);
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let [tabs_area, engine_area] =
        Layout::horizontal([Constraint::Min(24), Constraint::Length(34)]).areas(area);

    let active = match app.tab {
        Tab::Containers => 0,
        Tab::Images => 1,
    };
    frame.render_widget(
        Tabs::new(vec![Tab::Containers.title(), Tab::Images.title()])
            .select(active)
            .highlight_style(theme::selected_style()),
        tabs_area,
    );

    // Naming the engine matters here: docker and podman are easy to confuse
    // when one socket is bind-mounted at the other's path.
    let banner = match &app.engine {
        Some(info) => format!(
            "{} {} (API {})",
            info.flavor, info.version, info.api_version
        ),
        None => "connecting...".to_owned(),
    };
    frame.render_widget(
        Paragraph::new(banner)
            .right_aligned()
            .style(theme::dim_style()),
        engine_area,
    );
}

fn draw_containers(frame: &mut Frame, app: &App, state: &mut TableState, area: Rect) {
    let filter = if app.show_all { "all" } else { "running" };
    let block =
        Block::bordered().title(format!(" containers ({}, {filter}) ", app.containers.len()));

    if app.containers.is_empty() {
        frame.render_widget(empty_note("no containers", block), area);
        return;
    }

    let rows = app.containers.items.iter().map(|c| {
        let busy = app.inflight.get(&Target::Container(c.id.clone()));
        // A row mid-action says what it is doing rather than a stale state.
        let (state_text, state_style) = match busy {
            Some(action) => (
                format!("{}...", action.present_participle()),
                theme::severity_style(Severity::Warn),
            ),
            None => (
                c.state.as_str().to_owned(),
                theme::severity_style(c.state.severity()),
            ),
        };
        Row::new(vec![
            Cell::from(c.display_name().to_owned()),
            Cell::from(c.image_label().to_owned()),
            Cell::from(state_text).style(state_style),
            Cell::from(c.status_text.clone()),
            Cell::from(c.ports_summary()),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(18),
            Constraint::Percentage(22),
            Constraint::Length(12),
            Constraint::Percentage(20),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec!["NAME", "IMAGE", "STATE", "STATUS", "PORTS"]).style(theme::header_style()),
    )
    .row_highlight_style(theme::selected_style())
    .highlight_symbol("> ")
    .block(block);

    state.select(app.containers.selected());
    frame.render_stateful_widget(table, area, state);
}

fn draw_images(frame: &mut Frame, app: &App, state: &mut TableState, area: Rect) {
    let block = Block::bordered().title(format!(" images ({}) ", app.images.len()));

    if app.images.is_empty() {
        frame.render_widget(empty_note("no images", block), area);
        return;
    }

    let rows = app.images.items.iter().map(|img| {
        let busy = app.inflight.get(&Target::Image(img.id.clone()));
        let used_by = match img.containers {
            Some(count) => count.to_string(),
            None => "-".to_owned(),
        };
        let mut name = img.name_label();
        if img.extra_tag_count() > 0 {
            name.push_str(&format!(" (+{})", img.extra_tag_count()));
        }
        let status = match busy {
            Some(action) => format!("{}...", action.present_participle()),
            None if img.is_dangling() => "dangling".to_owned(),
            None => String::new(),
        };
        Row::new(vec![
            Cell::from(name),
            Cell::from(img.id.short().to_owned()),
            Cell::from(img.size.to_string()),
            Cell::from(img.created.format("%Y-%m-%d").to_string()),
            Cell::from(used_by),
            Cell::from(status).style(theme::severity_style(Severity::Warn)),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(34),
            Constraint::Length(14),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Min(8),
        ],
    )
    .header(
        Row::new(vec![
            "REPOSITORY:TAG",
            "ID",
            "SIZE",
            "CREATED",
            "USED BY",
            "",
        ])
        .style(theme::header_style()),
    )
    .row_highlight_style(theme::selected_style())
    .highlight_symbol("> ")
    .block(block);

    state.select(app.images.selected());
    frame.render_stateful_widget(table, area, state);
}

fn empty_note<'a>(text: &'a str, block: Block<'a>) -> Paragraph<'a> {
    Paragraph::new(text)
        .centered()
        .style(theme::dim_style())
        .block(block)
}

/// Latest message wins, then a standing load failure, then the key hints.
fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let active_load = match app.tab {
        Tab::Containers => &app.containers.load,
        Tab::Images => &app.images.load,
    };

    let (text, style) = if let Some(toast) = app.toasts.last() {
        (toast.text.clone(), theme::severity_style(toast.severity))
    } else if let LoadState::Failed { message } = active_load {
        (message.clone(), theme::severity_style(Severity::Bad))
    } else {
        (KEY_HINTS.to_owned(), theme::dim_style())
    };

    frame.render_widget(Paragraph::new(text).style(style), area);
}

fn draw_modal(frame: &mut Frame, app: &App, modal: &Modal) {
    let area = match modal {
        Modal::Help => centred(frame.area(), 56, 60),
        _ => centred(frame.area(), 44, 46),
    };
    // Clear first: a popup over a table must not show the table through it.
    frame.render_widget(Clear, area);

    match modal {
        Modal::ActionMenu {
            target,
            options,
            cursor,
        } => {
            let lines: Vec<Line> = options
                .iter()
                .enumerate()
                .map(|(index, action)| {
                    let label = format!(
                        "{} {}",
                        if index == *cursor { ">" } else { " " },
                        action.label()
                    );
                    if index == *cursor {
                        Line::from(Span::styled(label, theme::selected_style()))
                    } else {
                        Line::from(label)
                    }
                })
                .collect();
            frame.render_widget(
                Paragraph::new(Text::from(lines))
                    .block(Block::bordered().title(format!(" {} ", app.label_for(target)))),
                area,
            );
        }
        Modal::Confirm { prompt, .. } => {
            let body = Text::from(vec![
                Line::from(prompt.clone()),
                Line::from(""),
                Line::from(Span::styled("enter = yes    esc = no", theme::dim_style())),
            ]);
            frame.render_widget(
                Paragraph::new(body).block(Block::bordered().title(" confirm ".red().bold())),
                area,
            );
        }
        Modal::Help => {
            let body = Text::from(vec![
                Line::from("j / down      move down"),
                Line::from("k / up        move up"),
                Line::from("g / G         first / last row"),
                Line::from("tab           switch pane"),
                Line::from("x / enter     actions for the row"),
                Line::from("a             show all (stopped too)"),
                Line::from("r             refresh now"),
                Line::from("esc           close"),
                Line::from("q / ctrl-c    quit"),
            ]);
            frame.render_widget(
                Paragraph::new(body).block(Block::bordered().title(" keys ")),
                area,
            );
        }
    }
}

/// A centred popup of the given proportions.
fn centred(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let [_, middle, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .areas(area);
    let [_, centre, _] = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .areas(middle);
    centre
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::{DateTime, Utc};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::app::{Action, EngineEvent, Modal, Msg, Toast};
    use crate::domain::{
        ActionKind, ByteSize, Container, ContainerId, ContainerState, Image, ImageId, ImageRef,
        PortMapping, Protocol, Severity, Target,
    };
    use crate::engine::{EngineError, EngineFlavor, EngineInfo};

    use super::*;

    fn screen(app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(110, 22)).expect("test terminal");
        let mut ui = UiState::new();
        terminal
            .draw(|frame| draw(frame, app, &mut ui))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let area = buffer.area();
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| {
                        buffer
                            .cell((x, y))
                            .map(|cell| cell.symbol())
                            .unwrap_or(" ")
                            .to_owned()
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn container(name: &str, id: &str, state: ContainerState) -> Container {
        Container {
            id: ContainerId::from(id.to_owned()),
            names: vec![name.to_owned()],
            image: "nginx:latest".to_owned(),
            image_id: ImageId::from("sha256:5d0da3dc976460b7".to_owned()),
            command: "nginx".to_owned(),
            created: DateTime::UNIX_EPOCH,
            state,
            status_text: "Up 2 hours".to_owned(),
            ports: vec![PortMapping {
                host_ip: None,
                host_port: Some(8080),
                container_port: 80,
                protocol: Protocol::Tcp,
            }],
            labels: BTreeMap::new(),
        }
    }

    fn image() -> Image {
        Image {
            id: ImageId::from("sha256:cccc000000003333".to_owned()),
            repo_tags: vec![ImageRef::parse("nginx:latest")],
            repo_digests: Vec::new(),
            created: DateTime::UNIX_EPOCH,
            size: ByteSize(20_440_000),
            shared_size: None,
            containers: Some(0),
            labels: BTreeMap::new(),
        }
    }

    fn app() -> App {
        let mut app = App::new(true);
        app.update(Msg::Engine(EngineEvent::Info(Ok(EngineInfo {
            flavor: EngineFlavor::Podman,
            version: "5.6.2".to_owned(),
            api_version: "1.52".to_owned(),
        }))));
        app.update(Msg::Engine(EngineEvent::Containers(Ok(vec![
            container("web", "aaaa000000001111", ContainerState::Running),
            container("db", "bbbb000000002222", ContainerState::Exited),
        ]))));
        app.update(Msg::Engine(EngineEvent::Images(Ok(vec![image()]))));
        app
    }

    #[test]
    fn the_container_pane_shows_names_states_and_ports() {
        let output = screen(&app());
        assert!(output.contains("web"), "missing container name:\n{output}");
        assert!(output.contains("running"), "missing state:\n{output}");
        assert!(output.contains("exited"));
        assert!(output.contains("8080->80/tcp"), "missing ports:\n{output}");
        assert!(output.contains("nginx:latest"));
    }

    #[test]
    fn the_header_names_the_engine_that_answered() {
        let output = screen(&app());
        assert!(
            output.contains("podman"),
            "missing engine flavour:\n{output}"
        );
        assert!(output.contains("5.6.2"));
    }

    #[test]
    fn both_tabs_are_offered_and_the_active_one_is_shown() {
        let output = screen(&app());
        assert!(output.contains("Containers"));
        assert!(output.contains("Images"));
    }

    #[test]
    fn the_image_pane_shows_tags_and_sizes() {
        let mut app = app();
        app.update(Msg::Action(Action::NextTab));
        let output = screen(&app);
        assert!(output.contains("nginx:latest"));
        assert!(output.contains("20.44MB"), "missing size:\n{output}");
        assert!(
            output.contains("cccc00000000"),
            "missing short id:\n{output}"
        );
    }

    #[test]
    fn a_row_with_an_action_in_flight_says_so() {
        let mut app = app();
        app.inflight.insert(
            Target::Container(ContainerId::from("aaaa000000001111".to_owned())),
            ActionKind::Stop,
        );
        let output = screen(&app);
        assert!(output.contains("stopping"), "no busy marker:\n{output}");
    }

    #[test]
    fn an_empty_pane_says_it_is_empty_rather_than_looking_broken() {
        let mut app = App::new(false);
        app.update(Msg::Engine(EngineEvent::Containers(Ok(Vec::new()))));
        let output = screen(&app);
        assert!(
            output.to_lowercase().contains("no containers"),
            "no empty-state message:\n{output}"
        );
    }

    #[test]
    fn a_failed_refresh_is_reported_without_hiding_the_rows() {
        let mut app = app();
        app.update(Msg::Engine(EngineEvent::Containers(Err(
            EngineError::Unreachable {
                endpoint: "unix:///run/x.sock".to_owned(),
                detail: "refused".to_owned(),
            },
        ))));
        let output = screen(&app);
        assert!(output.contains("web"), "rows disappeared:\n{output}");
        assert!(
            output.contains("unix:///run/x.sock"),
            "failure not reported:\n{output}"
        );
    }

    #[test]
    fn the_action_menu_lists_what_is_offered() {
        let mut app = app();
        app.update(Msg::Action(Action::OpenActionMenu));
        let output = screen(&app);
        assert!(output.contains("Stop"), "menu not drawn:\n{output}");
        assert!(output.contains("Restart"));
        assert!(output.contains("Enter"));
        assert!(
            output.contains("web"),
            "menu should name its target:\n{output}"
        );
    }

    #[test]
    fn a_confirmation_shows_the_prompt_and_the_keys_to_answer_it() {
        let mut app = app();
        app.modal = Some(Modal::Confirm {
            action: ActionKind::RemoveContainer { force: true },
            target: Target::Container(ContainerId::from("aaaa000000001111".to_owned())),
            prompt: "Remove (force) web?".to_owned(),
        });
        let output = screen(&app);
        assert!(
            output.contains("Remove (force) web?"),
            "no prompt:\n{output}"
        );
        assert!(
            output.to_lowercase().contains("enter"),
            "no answer keys:\n{output}"
        );
    }

    #[test]
    fn help_lists_the_bindings() {
        let mut app = app();
        app.update(Msg::Action(Action::ToggleHelp));
        let output = screen(&app);
        assert!(output.contains("quit"), "no help text:\n{output}");
        assert!(output.contains("refresh"));
    }

    #[test]
    fn the_footer_shows_the_latest_message() {
        let mut app = app();
        app.toasts.push(Toast {
            text: "Stop web: done".to_owned(),
            severity: Severity::Good,
            at: Utc::now(),
        });
        let output = screen(&app);
        assert!(
            output.contains("Stop web: done"),
            "toast missing:\n{output}"
        );
    }

    #[test]
    fn a_narrow_terminal_still_draws_without_panicking() {
        let mut terminal = Terminal::new(TestBackend::new(20, 5)).unwrap();
        let mut ui = UiState::new();
        let app = app();
        terminal
            .draw(|frame| draw(frame, &app, &mut ui))
            .expect("must survive a tiny viewport");
    }
}
