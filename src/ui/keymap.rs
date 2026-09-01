//! The only place that knows about keys. Translating here keeps `app` free of
//! terminal types and makes the bindings testable on their own.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::{Action, Msg};

/// Terminal event to app message. Resizes, mouse moves and focus changes carry
/// no intent: a redraw already happens every frame.
pub fn msg_for(event: &Event) -> Option<Msg> {
    match event {
        Event::Key(key) => action_for(key).map(Msg::Action),
        _ => None,
    }
}

/// `None` for a key jikura does not bind.
pub fn action_for(event: &KeyEvent) -> Option<Action> {
    // Terminals speaking the kitty keyboard protocol report press *and* release;
    // acting on both makes every keystroke fire twice.
    if event.kind == KeyEventKind::Release {
        return None;
    }

    if event.modifiers.contains(KeyModifiers::CONTROL) {
        return match event.code {
            KeyCode::Char('c') => Some(Action::Quit),
            _ => None,
        };
    }

    // Shift is expected (it is how `G` arrives); anything else is unbound.
    if !(event.modifiers.is_empty() || event.modifiers == KeyModifiers::SHIFT) {
        return None;
    }

    match event.code {
        KeyCode::Char('j') | KeyCode::Down => Some(Action::NextItem),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::PrevItem),
        KeyCode::Char('g') | KeyCode::Home => Some(Action::First),
        KeyCode::Char('G') | KeyCode::End => Some(Action::Last),
        KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => Some(Action::NextTab),
        KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => Some(Action::PrevTab),
        KeyCode::Char('a') => Some(Action::ToggleAll),
        KeyCode::Char('r') => Some(Action::Refresh),
        KeyCode::Char('x') => Some(Action::OpenActionMenu),
        KeyCode::Enter => Some(Action::Select),
        KeyCode::Esc => Some(Action::Dismiss),
        KeyCode::Char('?') => Some(Action::ToggleHelp),
        KeyCode::Char('q') => Some(Action::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn vim_and_arrow_keys_both_move_the_cursor() {
        assert_eq!(action_for(&key(KeyCode::Char('j'))), Some(Action::NextItem));
        assert_eq!(action_for(&key(KeyCode::Down)), Some(Action::NextItem));
        assert_eq!(action_for(&key(KeyCode::Char('k'))), Some(Action::PrevItem));
        assert_eq!(action_for(&key(KeyCode::Up)), Some(Action::PrevItem));
        assert_eq!(action_for(&key(KeyCode::Char('g'))), Some(Action::First));
        assert_eq!(action_for(&key(KeyCode::Char('G'))), Some(Action::Last));
    }

    #[test]
    fn tab_and_arrows_switch_panes() {
        assert_eq!(action_for(&key(KeyCode::Tab)), Some(Action::NextTab));
        assert_eq!(action_for(&key(KeyCode::Right)), Some(Action::NextTab));
        assert_eq!(action_for(&key(KeyCode::Left)), Some(Action::PrevTab));
    }

    #[test]
    fn the_working_keys_are_bound() {
        assert_eq!(
            action_for(&key(KeyCode::Char('a'))),
            Some(Action::ToggleAll)
        );
        assert_eq!(action_for(&key(KeyCode::Char('r'))), Some(Action::Refresh));
        assert_eq!(action_for(&key(KeyCode::Enter)), Some(Action::Select));
        assert_eq!(
            action_for(&key(KeyCode::Char('x'))),
            Some(Action::OpenActionMenu)
        );
        assert_eq!(action_for(&key(KeyCode::Esc)), Some(Action::Dismiss));
        assert_eq!(
            action_for(&key(KeyCode::Char('?'))),
            Some(Action::ToggleHelp)
        );
    }

    #[test]
    fn quit_answers_to_q_and_to_ctrl_c() {
        assert_eq!(action_for(&key(KeyCode::Char('q'))), Some(Action::Quit));
        assert_eq!(
            action_for(&KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
    }

    #[test]
    fn a_key_release_is_not_a_second_keypress() {
        // Terminals with the kitty keyboard protocol report press *and* release;
        // acting on both makes every keystroke fire twice.
        let mut event = key(KeyCode::Char('j'));
        event.kind = KeyEventKind::Release;
        assert_eq!(action_for(&event), None);
    }

    #[test]
    fn an_unbound_key_does_nothing() {
        assert_eq!(action_for(&key(KeyCode::Char('z'))), None);
        assert_eq!(action_for(&key(KeyCode::F(5))), None);
    }

    #[test]
    fn a_keypress_becomes_an_action_message() {
        let msg = msg_for(&Event::Key(key(KeyCode::Char('r'))));
        assert!(matches!(msg, Some(Msg::Action(Action::Refresh))));
    }

    #[test]
    fn events_carrying_no_intent_are_dropped() {
        assert!(msg_for(&Event::Resize(80, 24)).is_none());
        assert!(msg_for(&Event::FocusGained).is_none());
        assert!(msg_for(&Event::Key(key(KeyCode::Char('z')))).is_none());
    }

    #[test]
    fn a_modifier_does_not_smuggle_in_an_unrelated_binding() {
        assert_eq!(
            action_for(&KeyEvent::new(KeyCode::Char('q'), KeyModifiers::ALT)),
            None
        );
    }
}
