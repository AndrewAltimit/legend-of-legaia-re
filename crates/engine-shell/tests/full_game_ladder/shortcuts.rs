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
    /// The same uses by the line of the hand that took them, for
    /// `LEGAIA_FGL_SHORTCUT_SITES=1`'s breakdown.
    static SITES: std::cell::RefCell<BTreeMap<(&'static str, String), u64>> =
        const { std::cell::RefCell::new(BTreeMap::new()) };
}

/// A prop by scene name and anchor tile.
type PropKey = (String, (u8, u8));

thread_local! {
    /// Prop placements (`examine_*`, `touch_*`) the pad hand has made this
    /// thread, never reset: [`placements_made`].
    static PLACED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// Props whose last beat was played by placement alone and moved no
    /// flag, by scene and anchor, with the flags as they stood:
    /// [`dry_placement`].
    static DRY_PLACED: std::cell::RefCell<BTreeMap<PropKey, BTreeSet<u16>>> =
        const { std::cell::RefCell::new(BTreeMap::new()) };
}

/// The prop placements made so far: a beat that raised it was played, at
/// least in part, from a placed seat.
pub(super) fn placements_made() -> u64 {
    PLACED.with(std::cell::Cell::get)
}

/// Whether the walked hand skips prop `anchor` in `scene`: its last beat
/// was played from placed seats alone and moved no flag, and no flag has
/// moved since. The beats pass plays a prop again each pass; placed again to
/// the same no effect, `retona`'s prop at (64, 79), ringed by bands that
/// walk the party back, took sixteen placements a run and gave nothing. Any
/// flag re-arms it, not only those its record tests: the same prop is the
/// way out to `map02` once the boss on its band ring has raised `0x355` /
/// `0x356`, which it does not test.
pub(super) fn dry_placement(scene: &str, anchor: (u8, u8), flags: &BTreeSet<u16>) -> bool {
    pad_props()
        && DRY_PLACED.with(|d| {
            d.borrow()
                .get(&(scene.to_string(), anchor))
                .is_some_and(|then| then == flags)
        })
}

/// Record how the walked hand's beat on prop `anchor` in `scene` went:
/// `dry` when it was played from placed seats alone and moved no flag
/// (`flags` as they stand), else the memo is cleared.
pub(super) fn note_placement(scene: &str, anchor: (u8, u8), dry: bool, flags: &BTreeSet<u16>) {
    if !pad_props() {
        return;
    }
    DRY_PLACED.with(|d| {
        let mut d = d.borrow_mut();
        if dry {
            d.insert((scene.to_string(), anchor), flags.clone());
        } else {
            d.remove(&(scene.to_string(), anchor));
        }
    });
}

/// Forget every dry placement: a fresh run.
pub(super) fn reset_dry_placements() {
    DRY_PLACED.with(|d| d.borrow_mut().clear());
}

/// Count one use of the shortcut `name`, when the pad hand is driving.
#[track_caller]
pub(super) fn shortcut(name: &'static str) {
    if pad_hand() {
        TALLY.with(|t| *t.borrow_mut().entry(name).or_insert(0) += 1);
        if name.starts_with("examine_s")
            || name.starts_with("examine_far")
            || name.starts_with("touch_")
        {
            PLACED.with(|p| p.set(p.get() + 1));
        }
        if shortcut_sites_on() {
            let at = std::panic::Location::caller();
            let file = at.file().rsplit('/').next().unwrap_or(at.file());
            let site = format!("{file}:{}", at.line());
            SITES.with(|t| *t.borrow_mut().entry((name, site)).or_insert(0) += 1);
        }
    }
}

/// Whether part C prints each shortcut's firing lines
/// (`LEGAIA_FGL_SHORTCUT_SITES=1`).
pub(super) fn shortcut_sites_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("LEGAIA_FGL_SHORTCUT_SITES").is_some_and(|v| v != "0"))
}

/// The per-line tally so far, emptied: one `[shortcut-site]` line each.
pub(super) fn take_shortcut_sites() -> Vec<String> {
    SITES
        .with(|t| std::mem::take(&mut *t.borrow_mut()))
        .into_iter()
        .map(|((name, site), n)| format!("[shortcut-site] {name} {site} {n}"))
        .collect()
}

/// Whether the pad hand must not take a closable shortcut this run
/// (`LEGAIA_FGL_NO_SHORTCUTS=1`).
pub(super) fn shortcuts_refused() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    pad_hand()
        && *OFF
            .get_or_init(|| std::env::var_os("LEGAIA_FGL_NO_SHORTCUTS").is_some_and(|v| v != "0"))
}

/// Whether the pad hand plays examines, touches and box talks on the pad
/// (`LEGAIA_FGL_PAD_PROPS=1`, implied by `LEGAIA_FGL_NO_SHORTCUTS=1`), walking to each seat, leaning in and turning
/// to an interact door before its tap, and placing the player only where no
/// seat is in reach. Off, it places the player as the seated tier does, and
/// each placement is counted. Not yet the default: walked, the beats pass's
/// props answer by the side they are reached from, and the run's story
/// branches part from the measured one (see the ladder docs).
pub(super) fn pad_props() -> bool {
    pad_hand() && props_walked()
}

/// Whether this run's pad hand walks its props ([`pad_props`]), read off
/// the environment alone.
pub(super) fn props_walked() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        ["LEGAIA_FGL_PAD_PROPS", "LEGAIA_FGL_NO_SHORTCUTS"]
            .iter()
            .any(|k| std::env::var_os(k).is_some_and(|v| v != "0"))
    })
}

/// The baseline table part C's shortcut tally is held to, `placed` or
/// `walked` by how the run plays its props; `None` for a run whose
/// configuration is a probe (no fallbacks, retail decode or options, the
/// opening skipped, NPCs walked through, no lead-in heal), which keeps no
/// ratchet.
pub(super) fn shortcut_ratchet_mode() -> Option<&'static str> {
    let set = |k: &str| std::env::var_os(k).is_some_and(|v| v != "0");
    let unset = |k: &str| std::env::var_os(k).is_some_and(|v| v == "0");
    let probe = [
        "LEGAIA_FGL_NO_SHORTCUTS",
        "LEGAIA_FGL_RETAIL_DECODE",
        "LEGAIA_FGL_PIN_RETAIL",
        "LEGAIA_FGL_SKIP_OPENING",
        "LEGAIA_FGL_NO_PAD",
    ]
    .iter()
    .any(|k| set(k))
        || ["LEGAIA_FGL_SOLID_NPCS", "LEGAIA_FGL_LEAD_IN_HEAL"]
            .iter()
            .any(|k| unset(k));
    (!probe).then(|| if props_walked() { "walked" } else { "placed" })
}

/// The categories of `tally` that rose above `want`, the run's committed
/// row (a category the row does not name stands at zero), one
/// `name N > M` each. Empty when nothing rose; a category that fell is a
/// gain, raised into the row by a reviewed edit.
pub(super) fn shortcut_rises(
    tally: &BTreeMap<&'static str, u64>,
    want: &BTreeMap<String, u64>,
) -> Vec<String> {
    tally
        .iter()
        .filter_map(|(&k, &n)| {
            let m = want.get(k).copied().unwrap_or(0);
            (n > m).then(|| format!("{k} {n} > {m}"))
        })
        .collect()
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
fn the_ratchet_fails_only_a_rise() {
    let tally: BTreeMap<&'static str, u64> = [("restore_xz", 40), ("touch_seat", 3)].into();
    let row: BTreeMap<String, u64> = [
        ("restore_xz".to_string(), 41),
        ("touch_seat".to_string(), 3),
    ]
    .into();
    assert!(shortcut_rises(&tally, &row).is_empty());
    let row: BTreeMap<String, u64> = [("restore_xz".to_string(), 39)].into();
    assert_eq!(
        shortcut_rises(&tally, &row),
        vec![
            "restore_xz 40 > 39".to_string(),
            "touch_seat 3 > 0".to_string()
        ]
    );
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
