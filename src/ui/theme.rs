//! The single place a domain `Severity` becomes a colour.

use ratatui::style::{Color, Modifier, Style};

use crate::domain::Severity;

pub fn severity_style(severity: Severity) -> Style {
    match severity {
        Severity::Good => Style::default().fg(Color::Green),
        Severity::Warn => Style::default().fg(Color::Yellow),
        Severity::Bad => Style::default().fg(Color::Red),
        Severity::Neutral => Style::default().fg(Color::Gray),
    }
}

pub fn header_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn selected_style() -> Style {
    Style::default()
        .add_modifier(Modifier::REVERSED)
        .add_modifier(Modifier::BOLD)
}

pub fn dim_style() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}
