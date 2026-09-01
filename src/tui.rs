//! Terminal setup and teardown. Isolated here so that every exit path -- clean
//! return, error, or panic -- leaves the terminal usable.

use std::io;
use std::panic;

use ratatui::DefaultTerminal;

/// Enters the alternate screen and installs a panic hook that restores the
/// terminal first. Without the hook a panic leaves the tty in raw mode with no
/// echo, which looks like a hung shell.
pub fn init() -> io::Result<DefaultTerminal> {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
    ratatui::try_init()
}

/// Leaves the alternate screen and disables raw mode. Safe to call twice.
pub fn restore() {
    ratatui::restore();
}
