//! Jumping straight to an entry in the browse modes (`prot`, a `tmd` or
//! `stage` directory): type its number, press Enter. Stepping one entry at a
//! time through 1233 PROT entries is the only other way there.
//!
//! The typed number is shown in the window title while it is entered, since
//! the title is the only text these views have.

/// The number being typed, if any.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct GotoInput {
    digits: String,
}

/// Longest number the field takes (PROT indices are four digits).
const MAX_DIGITS: usize = 6;

impl GotoInput {
    pub(crate) fn is_active(&self) -> bool {
        !self.digits.is_empty()
    }

    /// Append one decimal digit; extra digits past [`MAX_DIGITS`] are
    /// ignored.
    pub(crate) fn push(&mut self, digit: u8) {
        if digit < 10 && self.digits.len() < MAX_DIGITS {
            self.digits.push(char::from(b'0' + digit));
        }
    }

    pub(crate) fn backspace(&mut self) {
        self.digits.pop();
    }

    /// Drop the typed number. `true` when there was one (so `Esc` cancels
    /// the entry instead of quitting).
    pub(crate) fn cancel(&mut self) -> bool {
        let had = self.is_active();
        self.digits.clear();
        had
    }

    /// The typed number, clearing the field.
    pub(crate) fn take(&mut self) -> Option<u32> {
        let n = self.digits.parse().ok();
        self.digits.clear();
        n
    }

    /// The window-title prefix while a number is being typed.
    pub(crate) fn prompt(&self, what: &str) -> Option<String> {
        self.is_active()
            .then(|| format!("Go to {what} {}_ (Enter, Esc cancels)", self.digits))
    }
}

/// The position a typed number names in a list of `count` items: a PROT
/// index as typed (`one_based = false`), a directory walk's 1-based position
/// as its title shows it (`one_based = true`). `None` when out of range.
pub(crate) fn goto_target(typed: u32, count: usize, one_based: bool) -> Option<usize> {
    let pos = if one_based {
        (typed as usize).checked_sub(1)?
    } else {
        typed as usize
    };
    (pos < count).then_some(pos)
}

/// The keys the browse modes answer, worded for the window title.
pub(crate) const BROWSE_KEYS: &str =
    "N/P or arrows step, PgDn/PgUp 10, Home/End, type a number + Enter to jump, Esc quits";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_digits_become_a_number_once() {
        let mut g = GotoInput::default();
        assert!(!g.is_active() && g.prompt("entry").is_none());
        for d in [8, 6, 5] {
            g.push(d);
        }
        assert_eq!(
            g.prompt("entry").as_deref(),
            Some("Go to entry 865_ (Enter, Esc cancels)")
        );
        g.backspace();
        g.push(7);
        assert_eq!(g.take(), Some(867));
        assert_eq!(g.take(), None, "taking clears the field");
    }

    #[test]
    fn escape_cancels_a_typed_number_before_it_quits() {
        let mut g = GotoInput::default();
        assert!(!g.cancel(), "nothing typed: Esc quits");
        g.push(1);
        assert!(g.cancel());
        assert!(!g.is_active());
    }

    #[test]
    fn length_is_capped_and_non_digits_ignored() {
        let mut g = GotoInput::default();
        for _ in 0..10 {
            g.push(9);
        }
        g.push(12);
        assert_eq!(g.take(), Some(999_999));
    }

    #[test]
    fn targets_respect_the_index_base_and_range() {
        assert_eq!(goto_target(0, 1233, false), Some(0));
        assert_eq!(goto_target(1232, 1233, false), Some(1232));
        assert_eq!(goto_target(1233, 1233, false), None);
        assert_eq!(goto_target(1, 10, true), Some(0));
        assert_eq!(goto_target(10, 10, true), Some(9));
        assert_eq!(goto_target(0, 10, true), None);
        assert_eq!(goto_target(11, 10, true), None);
    }
}
