use chrono::{DateTime, Utc};

/// How a pane's data is doing. `Failed` deliberately keeps whatever rows were
/// last loaded: a refresh that fails must not blank the table the user is
/// reading -- especially since one unmappable field fails a whole response.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LoadState {
    #[default]
    Never,
    Loading,
    Loaded {
        at: DateTime<Utc>,
    },
    Failed {
        message: String,
    },
}

/// A pane's rows plus its cursor. Holds no ratatui types: the render layer
/// derives widget state from `selected` each frame.
#[derive(Debug, Clone, Default)]
pub struct ResourceList<T> {
    pub items: Vec<T>,
    selected: Option<usize>,
    pub load: LoadState,
}

impl<T> ResourceList<T> {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected: None,
            load: LoadState::Never,
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn selected_item(&self) -> Option<&T> {
        self.selected.and_then(|index| self.items.get(index))
    }

    /// Wraps at the ends -- long lists are quicker to reach from either side.
    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = Some(match self.selected {
            Some(index) if index + 1 < self.items.len() => index + 1,
            Some(_) => 0,
            None => 0,
        });
    }

    pub fn select_previous(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = Some(match self.selected {
            Some(0) | None => self.items.len() - 1,
            Some(index) => index - 1,
        });
    }

    pub fn select_first(&mut self) {
        self.selected = (!self.items.is_empty()).then_some(0);
    }

    pub fn select_last(&mut self) {
        self.selected = self.items.len().checked_sub(1);
    }

    /// Swaps in fresh rows, keeping the cursor on the same *item* rather than
    /// the same index -- otherwise a container disappearing above the cursor
    /// silently moves the selection onto a different row.
    pub fn replace<K, F>(&mut self, items: Vec<T>, key: F, at: DateTime<Utc>)
    where
        K: Eq,
        F: Fn(&T) -> K,
    {
        let previous_key = self.selected_item().map(&key);
        let previous_index = self.selected;

        self.items = items;
        self.load = LoadState::Loaded { at };

        self.selected = if self.items.is_empty() {
            None
        } else if let Some(wanted) = previous_key {
            // Same item if it is still here, otherwise hold the position.
            self.items
                .iter()
                .position(|item| key(item) == wanted)
                .or_else(|| previous_index.map(|index| index.min(self.items.len() - 1)))
        } else {
            Some(0)
        };
    }

    /// Records a failed refresh without discarding the rows on screen.
    pub fn fail(&mut self, message: String) {
        self.load = LoadState::Failed { message };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[&str]) -> ResourceList<String> {
        let mut l = ResourceList::new();
        l.replace(
            items.iter().map(|s| (*s).to_owned()).collect(),
            |s: &String| s.clone(),
            DateTime::UNIX_EPOCH,
        );
        l
    }

    #[test]
    fn a_fresh_list_has_no_selection_and_never_loaded() {
        let l: ResourceList<String> = ResourceList::new();
        assert_eq!(l.selected(), None);
        assert_eq!(l.selected_item(), None);
        assert_eq!(l.load, LoadState::Never);
    }

    #[test]
    fn the_first_load_selects_the_first_row() {
        let l = list(&["a", "b", "c"]);
        assert_eq!(l.selected(), Some(0));
        assert_eq!(l.selected_item().map(String::as_str), Some("a"));
        assert!(matches!(l.load, LoadState::Loaded { .. }));
    }

    #[test]
    fn moving_the_cursor_wraps_at_both_ends() {
        let mut l = list(&["a", "b", "c"]);
        l.select_next();
        assert_eq!(l.selected(), Some(1));
        l.select_last();
        assert_eq!(l.selected(), Some(2));
        l.select_next();
        assert_eq!(l.selected(), Some(0), "next past the end wraps to the top");
        l.select_previous();
        assert_eq!(
            l.selected(),
            Some(2),
            "previous past the top wraps to the end"
        );
        l.select_first();
        assert_eq!(l.selected(), Some(0));
    }

    #[test]
    fn moving_the_cursor_in_an_empty_list_does_nothing() {
        let mut l: ResourceList<String> = ResourceList::new();
        l.select_next();
        l.select_previous();
        l.select_last();
        assert_eq!(l.selected(), None);
    }

    #[test]
    fn a_refresh_keeps_the_cursor_on_the_same_item_when_rows_shift() {
        let mut l = list(&["a", "b", "c"]);
        l.select_last();
        assert_eq!(l.selected_item().map(String::as_str), Some("c"));
        // "a" is gone, so "c" is now index 1
        l.replace(
            vec!["b".to_owned(), "c".to_owned()],
            |s: &String| s.clone(),
            DateTime::UNIX_EPOCH,
        );
        assert_eq!(l.selected(), Some(1));
        assert_eq!(l.selected_item().map(String::as_str), Some("c"));
    }

    #[test]
    fn a_refresh_that_removes_the_selected_item_clamps_to_the_same_position() {
        let mut l = list(&["a", "b", "c"]);
        l.select_last();
        l.replace(
            vec!["a".to_owned(), "b".to_owned()],
            |s: &String| s.clone(),
            DateTime::UNIX_EPOCH,
        );
        assert_eq!(l.selected(), Some(1), "clamped to the last remaining row");
    }

    #[test]
    fn a_refresh_to_an_empty_list_clears_the_selection() {
        let mut l = list(&["a"]);
        l.replace(Vec::new(), |s: &String| s.clone(), DateTime::UNIX_EPOCH);
        assert_eq!(l.selected(), None);
        assert!(l.is_empty());
    }

    #[test]
    fn a_failed_refresh_keeps_the_rows_already_on_screen() {
        let mut l = list(&["a", "b"]);
        l.select_next();
        l.fail("engine went away".to_owned());
        assert_eq!(l.len(), 2, "rows must survive a failed refresh");
        assert_eq!(l.selected(), Some(1), "and so must the cursor");
        assert_eq!(
            l.load,
            LoadState::Failed {
                message: "engine went away".to_owned()
            }
        );
    }
}
