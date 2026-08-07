//! The field overlay's **op-`0x49` submode** family: opening a sub-screen,
//! spawning its driver actor, and laying out its list panel.
//!
//! PORT: FUN_801D9C3C, FUN_801DE478, FUN_801E6984, FUN_801D84B4
//!
//! A field script reaches a sub-screen through actor `+0x50` handler slots (see
//! [`crate::field_menu_dispatch`] and `docs/subsystems/script-vm.md`). The four
//! routines here are the entry side of that:
//!
//! - [`open_submode`] - `FUN_801D9C3C`: reset the submode's context block and
//!   spawn its driver actor, but only if one is not already running.
//! - [`scene_actor_initial_state`] - `FUN_801DE478`: the smaller sibling -
//!   spawn one actor from a fixed template and seed its state byte.
//! - [`submode_panel_rows`] - `FUN_801E6984`: the list panel's per-row layout +
//!   colour selection.
//! - [`request_card_mode`] - `FUN_801D84B4`: a six-store leaf that hands the
//!   master mode to CARD.
//!
//! Provenance: the `overlay_cutscene_dialogue_*` / `overlay_cutscene_mapview_*`
//! field captures, which agree byte for byte at all four addresses. **Do not
//! use the `overlay_0897_*` dumps for three of them** - `overlay_0897_801d9c3c`
//! resolves `entry=801d9c0c` and `overlay_0897_801de478` resolves
//! `entry=801de468` (the dumper names files after the *requested* address while
//! resolving the *containing* function), and `overlay_0897_801ddc20` reports
//! zero instructions outright.
//!
//! [`open_submode`] and [`scene_actor_initial_state`] are **live**, on the same
//! chain retail puts them on: the MAN loader `FUN_8003AEB0` calls
//! `FUN_801D9C3C()` at `0x8003B444` and `FUN_801DE478(0xF)` at `0x8003B9B0`, so
//! the engine runs both from `SceneHost::load_scene` →
//! [`World::man_load_actor_reset`]. What unblocked them is the actor `+0x0C`
//! handler ([`crate::actor_handler::ActorHandler`]): the open searches the pool
//! for a live [`SUBMODE_DRIVER_HANDLER`] through the port of `FUN_8003CF04`,
//! and the spawn installs a handler on the slot it takes.
//!
//! [`World::man_load_actor_reset`]: crate::world::World::man_load_actor_reset
//!
//! The other two stay disclosed and their notes say why - [`submode_panel_rows`]
//! wants a GPU-primitive channel, [`request_card_mode`] wants a retail
//! master-mode word.
//!
//! REF: FUN_80020DE0 (actor spawn), FUN_8003CF04 (actor-by-handler search),
//! FUN_8003AEB0 (the MAN loader that calls the first two),
//! FUN_8002B994 / FUN_8002C488 / FUN_80036888 / FUN_8002C69C (the draw leaves)

/// Submode context words the open routine seeds, as `(offset_from_0x801F2734,
/// value)` pairs.
///
/// The block is a run of `u32`s based at `0x801F2734`; the open routine writes
/// ten of them explicitly and then clears a 16-word tail. Offsets are kept
/// rather than named because only three have confirmed roles:
/// `+0x00` is the submode **state** (`1` = open - the same word the resume /
/// close handler `FUN_801F159C` gates on, admitting `{1, 4, 7}`), `+0x0C` is a
/// mode selector seeded to `3`, and `+0x20` a second flag seeded to `1`.
pub const SUBMODE_CONTEXT_SEEDS: [(u16, u32); 10] = [
    (0x00, 1),  // 0x801F2734 - submode state: open
    (0x04, 0),  // 0x801F2738
    (0x08, 0),  // 0x801F273C
    (0x0C, 3),  // 0x801F2740 - mode selector
    (0x14, 0),  // 0x801F2748
    (0x18, 0),  // 0x801F274C
    (0x20, 1),  // 0x801F2754
    (0x24, 0),  // 0x801F2758
    (0x28, 0),  // 0x801F275C
    (0xE04, 0), // 0x801F3538
];

/// Words in the tail block the open routine clears
/// (`0x801F3540..=0x801F357C`, written high-to-low).
pub const SUBMODE_TAIL_WORDS: usize = 16;

/// The submode state value meaning "open".
pub const SUBMODE_STATE_OPEN: u32 = 1;

/// Driver-actor template the open routine spawns from.
pub const SUBMODE_DRIVER_TEMPLATE: u32 = 0x801F_2760;

/// Per-frame handler the open routine searches the actor list for before
/// spawning. An actor already running it means the submode is live and the
/// spawn is skipped.
pub const SUBMODE_DRIVER_HANDLER: u32 = 0x801D_84D0;

/// Outcome of opening a submode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmodeOpen {
    /// No actor was running [`SUBMODE_DRIVER_HANDLER`], so a driver actor is
    /// spawned from [`SUBMODE_DRIVER_TEMPLATE`] with `+0x50` and `+0x54` both
    /// cleared.
    Spawned,
    /// An actor is already running the handler; retail returns `0` and spawns
    /// nothing. The context reset **still happened** - it precedes the search.
    AlreadyOpen,
}

/// Open (or re-open) the field submode.
///
/// PORT: FUN_801D9C3C (`0x801d9c3c..0x801d9d2c`).
///
/// Order matters and is easy to get backwards: the context block is reset
/// **unconditionally, before** the already-open check. So calling this on a
/// live submode re-seeds its state to [`SUBMODE_STATE_OPEN`] and wipes the tail
/// block while leaving the existing driver actor running.
///
/// `handler_present` reports whether any actor in the scene list already runs
/// [`SUBMODE_DRIVER_HANDLER`] (retail's `FUN_8003CF04` search over the list head
/// at the scene control block's `+0x4`).
///
/// Returns the seeds the caller should apply plus what to do about the actor.
///
/// Live: `SceneHost::load_scene` → [`World::man_load_actor_reset`] calls this
/// on every MAN load, exactly where `FUN_8003AEB0` calls `FUN_801D9C3C()`
/// (`0x8003B444`). `handler_present` comes from
/// [`World::find_actor_by_handler`] (the port of `FUN_8003CF04`), and a
/// [`SubmodeOpen::Spawned`] result takes a pool slot with the driver handler
/// installed. Host roots: `SceneHost::enter_field_scene` → `BootSession` /
/// `legaia-engine run` / `play-window`.
///
/// [`World::man_load_actor_reset`]: crate::world::World::man_load_actor_reset
/// [`World::find_actor_by_handler`]: crate::world::World::find_actor_by_handler
pub fn open_submode(handler_present: bool) -> (&'static [(u16, u32); 10], SubmodeOpen) {
    (
        &SUBMODE_CONTEXT_SEEDS,
        if handler_present {
            SubmodeOpen::AlreadyOpen
        } else {
            SubmodeOpen::Spawned
        },
    )
}

/// Template the scene-actor spawner uses.
pub const SCENE_ACTOR_TEMPLATE: u32 = 0x801F_2810;

/// The state word the MAN loader asks for (`FUN_8003AEB0` at `0x8003B9B0`
/// passes `a0 = 0xF`). Every retail call site of `FUN_801DE478` found so far
/// is this one, so the "requested" argument has exactly one live value - the
/// interesting half is the `field_mode_flags` override in
/// [`scene_actor_initial_state`].
pub const SCENE_ACTOR_REQUESTED_STATE: u16 = 0x0F;

/// Spawn one scene actor and pick its initial state byte.
///
/// PORT: FUN_801DE478 (`0x801de478..0x801de4c4`).
///
/// Spawns from [`SCENE_ACTOR_TEMPLATE`] against the scene actor list, then
/// writes the new actor's `+0x54` state word: normally the caller's argument,
/// but **forced to `1`** when the field mode-flags word `_DAT_8007B868` is
/// non-zero. That word is the same one the window-rebuild sweep tests bit `1`
/// of ([`crate::field_regions::window_rebuild_spawns`]) and the field
/// initialiser branches the primitive-buffer size on, so a non-zero value means
/// "not an ordinary field frame" - and this actor then starts one state along.
///
/// Live: same chain as [`open_submode`], one call later - `FUN_8003AEB0`
/// issues `FUN_801DE478(0xF)` at `0x8003B9B0`, and
/// [`World::man_load_actor_reset`] mirrors it with
/// [`SCENE_ACTOR_REQUESTED_STATE`], seating the state word this returns on the
/// spawned slot.
///
/// [`World::man_load_actor_reset`]: crate::world::World::man_load_actor_reset
pub fn scene_actor_initial_state(requested: u16, field_mode_flags: u32) -> u16 {
    if field_mode_flags != 0 { 1 } else { requested }
}

/// Row pitch of the submode list panel, in pixels (the panel walks **upwards**,
/// so each row's Y is `0x10` less than the previous).
pub const PANEL_ROW_PITCH: i16 = 0x10;

/// Glyph colour for an ordinary row.
pub const PANEL_INK_NORMAL: i16 = 0x4F;

/// Glyph colour for the row matching the secondary cursor.
pub const PANEL_INK_MARKED: i16 = 0x58;

/// X offset of a row's highlight bar from the panel origin.
pub const PANEL_HIGHLIGHT_DX: i16 = 0x0C;

/// X offset of a row's primary glyph run.
pub const PANEL_GLYPH_DX: i16 = 0x24;

/// X offset of a row's secondary glyph run (drawn only for non-zero entries).
pub const PANEL_GLYPH2_DX: i16 = 0x30;

/// One laid-out row of the submode list panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelRow {
    /// Zero-based row index within the panel.
    pub index: u8,
    /// The entry id this row shows (`row + ctx[+0x02]`, the scroll base).
    pub entry: u8,
    /// Row baseline Y.
    pub y: i16,
    /// `true` when this row carries the selection highlight bar
    /// (`entry == _DAT_8007BB88`).
    pub highlighted: bool,
    /// Glyph-run **base id**, not a colour: retail adds it to the row's own
    /// entry index and hands the sum to the glyph-run draw leaf as the code
    /// (`(x + 0x24, y, entry + base)`), picking [`PANEL_INK_MARKED`] when
    /// `entry == _DAT_8007BB9C` and [`PANEL_INK_NORMAL`] otherwise
    /// (`0x801e6a0c..0x801e6a2c`). The field name predates that read; the
    /// values are the retail ones either way.
    pub ink: i16,
    /// `true` when the row draws its second glyph run (retail skips it for
    /// `entry == 0`).
    pub second_run: bool,
}

/// Lay out the submode list panel.
///
/// PORT: FUN_801E6984 (`0x801e6984..0x801e6a70`, the row loop).
///
/// `origin` is the panel struct's `(+0xA, +0xC)` pair; `count` and `scroll` are
/// the submode context's `+0x03` row count and `+0x02` scroll base
/// (`_DAT_8007B450`); `cursor` and `marked` are `_DAT_8007BB88` and
/// `_DAT_8007BB9C`.
///
/// The first row sits at `origin.1 + (count - 1) * 0x10` and each subsequent
/// row is `0x10` **lower-numbered**, i.e. retail fills the panel bottom-up. A
/// `count` of zero draws no rows at all (the loop is entered only after a
/// `count != 0` test), which is why the frame and labels are emitted separately
/// from this pass.
///
/// NOT WIRED: this is the layout half of a render-track routine, and the
/// blocker is on the render side only - the inputs are all live.
///
/// The context clause an earlier reading carried ("the context block it reads
/// is the same one [`open_submode`] cannot reach") is false, and contradicts
/// this module's own header: `open_submode` **is** live, its ten seeds land in
/// `World::submode_context`, and `World::submode_env` reads that block every
/// frame. The row inputs are live with it - the cursor and entry count this
/// pass wants are already projected into `HubEnv`.
///
/// What has no consumer is the output. The submode screen's own draw list
/// (`World::submode_screen.draws()`, built by the ported panel-window
/// painters) is not read by any host: neither `engine-shell` nor the browser
/// page renders the op-`0x49` screen at all. So a `PanelRow` would join a
/// draw list nothing paints. The prerequisite is a host consumer for that
/// list; the four draw leaves it would then need (`FUN_8002B994` highlight
/// bar, `FUN_8002C488` glyph run, `FUN_80036888` label, `FUN_8002C69C` frame)
/// are themselves ported, in `engine-render`, `engine-ui` and `legaia-font`.
pub fn submode_panel_rows(
    origin: (i16, i16),
    count: u8,
    scroll: u8,
    cursor: u8,
    marked: u8,
) -> Vec<PanelRow> {
    if count == 0 {
        return Vec::new();
    }
    let mut y = origin
        .1
        .wrapping_add((i16::from(count) - 1).wrapping_mul(PANEL_ROW_PITCH));
    let mut out = Vec::with_capacity(usize::from(count));
    for i in 0..count {
        let entry = i.wrapping_add(scroll);
        out.push(PanelRow {
            index: i,
            entry,
            y,
            highlighted: entry == cursor,
            ink: if entry == marked {
                PANEL_INK_MARKED
            } else {
                PANEL_INK_NORMAL
            },
            second_run: entry != 0,
        });
        y = y.wrapping_sub(PANEL_ROW_PITCH);
    }
    let _ = origin.0;
    out
}

/// Master game mode the CARD request leaf installs (`0x16` = 22, the
/// memory-card / menu overlay pair's init half - [`crate::mode::GameMode::CardInit`]).
pub const CARD_REQUEST_MODE: i16 = 0x16;

/// What the CARD request leaf writes.
///
/// REF: FUN_801D84B4 (`0x801d84b4..0x801d84cc`) - the `PORT:` for this address
/// sits on [`request_card_mode`], the free function that builds this record.
/// This is a plain data struct and the file declares no `impl` block at all, so
/// `port-catalog.py` widens the type anchor to the whole module - which is live
/// through [`open_submode`], and that made a correct `NOT WIRED:` here read as
/// a stale disclosure. The wiring verdict lives on the function instead.
///
/// Seven instructions, no `jal` at all - two stores and `jr ra` with the second
/// store in the delay slot. It sets the master game mode word `_DAT_8007B83C`
/// to [`CARD_REQUEST_MODE`] and raises `_DAT_8007BB00`.
///
/// It is worth knowing that this leaf is the other half of a gate already
/// ported: the field initialiser's BGM wait barrier spins until the audio
/// driver acknowledges the track **unless** the master mode is `0x16`
/// ([`crate::mode_entry_init::FIELD_BGM_WAIT_ABORT_MODE`]). So a CARD request
/// raised while a scene is loading is what lets that barrier give up.
///
/// This function was previously in the port catalog's **ignore list** under a
/// "PADDING" reason. That reason holds for four minigame images that carry
/// filler at this VA; it does not hold for field (897), where the address is a
/// real entry with a real body. A six-store leaf is still a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardRequest {
    /// Value written to the master game mode word `_DAT_8007B83C`.
    pub game_mode: i16,
    /// Value written to `_DAT_8007BB00`.
    pub flag: u32,
}

/// The CARD request leaf's two stores.
///
/// PORT: FUN_801D84B4.
///
/// NOT WIRED: nothing in `engine-core` requests a master-mode transition by
/// writing the retail mode word - [`crate::mode::GameMode`] changes go through
/// typed scene-host calls. Two ports do carry a `_DAT_8007B83C` mirror
/// (`scene_transition_actor`'s `EnterGameMode` step and
/// `ProtCdDmaHost::set_overlay_mode_state`), so the word is not unmodelled;
/// neither is a live master-mode driver a request could be routed through.
/// The companion flag `_DAT_8007BB00` has no engine counterpart at all; its
/// consumer is not in the ported set, which is the narrower blocker of the two.
///
/// The `(mode, warp sub-id)` bridge ([`crate::mode::GameMode::scene_mode_with_warp`])
/// is unrelated to both blockers: it decides which `SceneMode` a *running*
/// mode maps to, and this leaf is a *request* to change the mode. The
/// legitimate host is the same one `mode_init_bare` names - a production owner
/// of [`crate::mode::ModeDriver`] - plus a port of `_DAT_8007BB00`'s consumer,
/// without which the second store here has nowhere to land.
pub const fn request_card_mode() -> CardRequest {
    CardRequest {
        game_mode: CARD_REQUEST_MODE,
        flag: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_resets_the_context_before_checking_for_a_live_driver() {
        // Both outcomes hand back the same seed set - the reset is
        // unconditional and precedes the search.
        let (a, oa) = open_submode(false);
        let (b, ob) = open_submode(true);
        assert_eq!(a, b);
        assert_eq!(oa, SubmodeOpen::Spawned);
        assert_eq!(ob, SubmodeOpen::AlreadyOpen);
    }

    #[test]
    fn the_submode_state_word_is_seeded_open() {
        let state = SUBMODE_CONTEXT_SEEDS
            .iter()
            .find(|(off, _)| *off == 0)
            .map(|(_, v)| *v);
        assert_eq!(state, Some(SUBMODE_STATE_OPEN));
        // The mode selector is 3 and the second flag is 1; everything else
        // clears.
        assert_eq!(
            SUBMODE_CONTEXT_SEEDS
                .iter()
                .filter(|(_, v)| *v != 0)
                .count(),
            3
        );
    }

    #[test]
    fn scene_actor_state_is_forced_when_the_mode_flags_are_set() {
        assert_eq!(scene_actor_initial_state(0, 0), 0);
        assert_eq!(scene_actor_initial_state(7, 0), 7);
        // Any non-zero flags word forces state 1, whatever was asked for.
        assert_eq!(scene_actor_initial_state(0, 2), 1);
        assert_eq!(scene_actor_initial_state(7, 1), 1);
    }

    #[test]
    fn panel_rows_fill_bottom_up_from_the_origin() {
        let rows = submode_panel_rows((40, 100), 4, 0, 0, 0);
        assert_eq!(rows.len(), 4);
        // First row is (count-1) pitches above the origin, then descending.
        assert_eq!(rows[0].y, 100 + 3 * PANEL_ROW_PITCH);
        assert_eq!(rows[1].y, 100 + 2 * PANEL_ROW_PITCH);
        assert_eq!(rows[3].y, 100);
        assert!(rows.windows(2).all(|w| w[0].y > w[1].y));
    }

    #[test]
    fn a_zero_count_panel_draws_no_rows() {
        assert!(submode_panel_rows((0, 0), 0, 0, 0, 0).is_empty());
    }

    #[test]
    fn scroll_offsets_the_entry_ids_and_the_cursor_matches_on_entry() {
        let rows = submode_panel_rows((0, 0), 3, 10, 11, 12);
        assert_eq!(
            rows.iter().map(|r| r.entry).collect::<Vec<_>>(),
            [10, 11, 12]
        );
        // The highlight follows the entry id, not the row index.
        assert_eq!(
            rows.iter().map(|r| r.highlighted).collect::<Vec<_>>(),
            [false, true, false]
        );
        assert_eq!(
            rows.iter().map(|r| r.ink).collect::<Vec<_>>(),
            [PANEL_INK_NORMAL, PANEL_INK_NORMAL, PANEL_INK_MARKED]
        );
    }

    #[test]
    fn entry_zero_skips_its_second_glyph_run() {
        let rows = submode_panel_rows((0, 0), 2, 0, 9, 9);
        assert!(!rows[0].second_run, "entry 0 draws one run");
        assert!(rows[1].second_run);
    }

    #[test]
    fn the_card_request_matches_the_bgm_barrier_abort_mode() {
        let r = request_card_mode();
        assert_eq!(r.game_mode, CARD_REQUEST_MODE);
        assert_eq!(r.flag, 1);
        // The leaf writes exactly the mode the field initialiser's BGM wait
        // barrier bails out on - the two are the same gate seen from each end.
        assert_eq!(
            r.game_mode,
            crate::mode_entry_init::FIELD_BGM_WAIT_ABORT_MODE
        );
        // And it is mode 22, the CARD init half.
        assert_eq!(
            crate::mode::GameMode::from_index(r.game_mode as usize),
            Some(crate::mode::GameMode::CardInit)
        );
    }
}
