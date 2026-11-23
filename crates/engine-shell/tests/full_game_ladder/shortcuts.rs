//! The pad hand's shortcuts: the places it still writes world state a player
//! reaches only through the pad, each named and counted.
//!
//! The Uru Mais softlock (#653) hid because the hand wrote the pad straight
//! into the world. Its presses now go through the hosts' pad gate
//! ([`HostPad::host_pad`]); a shortcut is any other write of that kind - a
//! seat, a hand-set facing, a teleport back to where the player stood, an
//! option set without its screen. Each one is a place where a host-only
//! defect can hide, so each fires through [`shortcut`] and part C prints the
//! run's tally (`[shortcuts]`). The tally is a ratchet: a change that adds
//! a shortcut shows in it.
//!
//! Only the pad hand counts. The seated tier places the player by
//! definition ([`pad_hand`] is off there); what it proves is doors and
//! scripts, not locomotion.
//!
//! `LEGAIA_FGL_NO_SHORTCUTS=1` makes each closable site refuse instead of
//! writing ([`shortcuts_refused`]): the stretch is then played on the pad
//! alone, and a stall it shows is a candidate host-only defect.

use super::*;

thread_local! {
    static TALLY: std::cell::RefCell<BTreeMap<&'static str, u64>> =
        const { std::cell::RefCell::new(BTreeMap::new()) };
}

/// Count one use of the shortcut `name`, when the pad hand is driving.
pub(super) fn shortcut(name: &'static str) {
    if pad_hand() {
        TALLY.with(|t| *t.borrow_mut().entry(name).or_insert(0) += 1);
    }
}

/// Whether the pad hand must not take a closable shortcut this run
/// (`LEGAIA_FGL_NO_SHORTCUTS=1`).
pub(super) fn shortcuts_refused() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    pad_hand()
        && *OFF.get_or_init(|| {
            std::env::var_os("LEGAIA_FGL_NO_SHORTCUTS").is_some_and(|v| v != "0")
        })
}

/// The tally so far, emptied.
pub(super) fn take_shortcuts() -> BTreeMap<&'static str, u64> {
    TALLY.with(|t| std::mem::take(&mut *t.borrow_mut()))
}

/// The tally as the one line part C prints.
pub(super) fn shortcut_line(tally: &BTreeMap<&'static str, u64>) -> String {
    let total: u64 = tally.values().sum();
    let parts: Vec<String> = tally.iter().map(|(k, n)| format!("{k}={n}")).collect();
    format!(
        "[shortcuts] {total} use(s){}{}",
        if parts.is_empty() { "" } else { ": " },
        parts.join(" ")
    )
}

#[test]
fn the_tally_counts_only_the_pad_hand() {
    let _ = take_shortcuts();
    PAD_HAND.with(|h| h.set(false));
    shortcut("seated");
    PAD_HAND.with(|h| h.set(true));
    shortcut("a");
    shortcut("a");
    shortcut("b");
    PAD_HAND.with(|h| h.set(false));
    let t = take_shortcuts();
    assert_eq!(shortcut_line(&t), "[shortcuts] 3 use(s): a=2 b=1");
    assert!(take_shortcuts().is_empty());
    assert_eq!(shortcut_line(&BTreeMap::new()), "[shortcuts] 0 use(s)");
}
