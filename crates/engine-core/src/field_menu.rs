//! Field menu (pause menu) state machine.
//!
//! The retail "Start in field" pause menu: a vertical list of seven rows
//! in the retail order **Items / Magic / Equip / Status / Options / Load /
//! Save** (the id-50 command-list renderer `FUN_801CFD68` draws exactly
//! these labels at `WY + n*0xe`) plus a return-to-game cancel path. Each
//! row hands off to a sub-session already shipped in this crate (or via
//! the boot-UI dispatch in the shell):
//!
//! - **Items** → [`crate::inventory_use::InventoryUseSession`] in field context.
//! - **Magic** → [`crate::spell_menu::SpellMenuSession`].
//! - **Equip** → [`crate::equip_session::EquipSession`].
//! - **Status** → [`crate::status_screen::StatusScreenSession`].
//! - **Options** → [`crate::options::OptionsSession`].
//! - **Load** → [`crate::save_select::SaveSelectSession`] in Load mode.
//! - **Save** → [`crate::save_select::SaveSelectSession`] in Save mode.
//!
//! The last two rows are **conditional**, and the condition is disc data, not
//! host policy: Load is blocked while an op-`0x49` entry context of kind
//! `0x0D` is parked, and Save is blocked in any scene whose MAN header clears
//! the save-allow bit - which on the retail disc is every field scene, the
//! three kingdom world maps being the only ones that permit it. Hosts sample
//! both into a [`FieldMenuGate`] at menu-open; the session then runs retail's
//! own [`root_menu_confirm_route`] per row for both the ink and the confirm,
//! so a row cannot draw white and then buzz.
//!
//! The engine's Tactical Arts chain editor
//! ([`crate::tactical_arts_editor::ChainEditor`]) is an engine extension
//! with no retail pause-menu row; it stays reachable through the
//! dedicated arts session commands.
//!
//! Renderer-agnostic. Engines drive [`FieldMenuSession::tick`] each frame
//! with a [`FieldMenuInput`] bundle and consume the returned
//! [`FieldMenuEvent`] stream. The session emits an [`FieldMenuOutcome`] on
//! Done - the shell's job is to push the matching sub-session, then call
//! [`FieldMenuSession::resume`] when control returns.

use crate::pause_screens::{
    ROOT_MENU_ROUTES, ROOT_MENU_ROWS, RootMenuRoute, root_menu_confirm_route,
};

/// One menu row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldMenuRow {
    Items,
    Magic,
    Equip,
    Status,
    Options,
    Load,
    Save,
}

/// The engine row list and the retail route table describe the same picker,
/// so they must stay the same length.
const _: () = assert!(FieldMenuRow::ALL.len() == ROOT_MENU_ROWS as usize);

impl FieldMenuRow {
    /// Retail row order (`FUN_801CFD68` draw order, top to bottom).
    pub const ALL: [Self; 7] = [
        Self::Items,
        Self::Magic,
        Self::Equip,
        Self::Status,
        Self::Options,
        Self::Load,
        Self::Save,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Items => "Items",
            Self::Magic => "Magic",
            Self::Equip => "Equip",
            Self::Status => "Status",
            Self::Options => "Options",
            Self::Load => "Load",
            Self::Save => "Save",
        }
    }

    pub fn from_index(idx: u8) -> Option<Self> {
        Self::ALL.get(idx as usize).copied()
    }

    pub fn index(self) -> u8 {
        Self::ALL.iter().position(|r| *r == self).unwrap() as u8
    }

    /// The retail sub-screen id this row hands off to
    /// ([`crate::pause_screens::ROOT_MENU_ROUTES`]): Items `0x05`, Magic
    /// `0x0E`, Equip `0x12`, Status `0x15`, Options `0x17`, Load `0x18`,
    /// Save `0x19`.
    pub fn retail_subscreen(self) -> u8 {
        ROOT_MENU_ROUTES[self.index() as usize]
    }

    /// Inverse of [`Self::retail_subscreen`] - the row a retail sub-screen id
    /// names. The seven ids are distinct, so this round-trips.
    pub fn from_retail_subscreen(sub: u8) -> Option<Self> {
        ROOT_MENU_ROUTES
            .iter()
            .position(|id| *id == sub)
            .and_then(|i| Self::from_index(i as u8))
    }
}

/// Per-row enable/disable mask. Engines that have a save-blocked overlay
/// (e.g. cutscene playback) flip the matching row off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldMenuRowMask(u8);

impl FieldMenuRowMask {
    pub const ALL_ENABLED: Self = Self(0x7F);

    pub fn new() -> Self {
        Self::ALL_ENABLED
    }

    pub fn enable(&mut self, row: FieldMenuRow) {
        self.0 |= 1 << row.index();
    }

    pub fn disable(&mut self, row: FieldMenuRow) {
        self.0 &= !(1 << row.index());
    }

    pub fn is_enabled(&self, row: FieldMenuRow) -> bool {
        (self.0 >> row.index()) & 1 == 1
    }
}

impl Default for FieldMenuRowMask {
    fn default() -> Self {
        Self::ALL_ENABLED
    }
}

/// The two runtime inputs retail's root command picker gates its last two rows
/// on, carried together because one function reads both.
///
/// Retail keeps them as globals the picker and its row renderer each sample
/// directly: the entry-context pointer `_DAT_8007B450` (whose kind byte blocks
/// **Load**) and the per-scene save-allow byte `_DAT_8007B6A8` (which blocks
/// **Save**). The port has no globals, so a host samples both at menu-open and
/// hands them over - see `BootSession::open_field_menu`, which reads
/// [`crate::world::World::menu_entry_context_kind`] and
/// [`crate::world::World::scene_save_allowed`].
///
/// Both the greying and the buzz come from one call to
/// [`root_menu_confirm_route`] per row, which is what keeps them from
/// disagreeing - retail's row renderer `FUN_801CFD68` re-tests the same two
/// globals in the same order for exactly that reason.
///
/// The default is "unblocked": no entry context, saving permitted. That is the
/// right default for a session a host builds without a world behind it (the
/// menu suite's fixtures), not a claim about a scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldMenuGate {
    /// Kind byte of the armed op-`0x49` entry context (`*_DAT_8007B450`), or
    /// `None` when no script is parked. Only
    /// [`crate::pause_screens::ROOT_MENU_CONTEXT_LOCKED`] blocks Load.
    pub entry_context_kind: Option<u8>,
    /// The scene's save permission (`_DAT_8007B6A8`, seeded from the MAN
    /// header bit). `false` greys Save and buzzes its confirm.
    pub save_allowed: bool,
}

impl Default for FieldMenuGate {
    fn default() -> Self {
        Self {
            entry_context_kind: None,
            save_allowed: true,
        }
    }
}

/// Phase of the field-menu SM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldMenuPhase {
    /// Player is browsing the row list.
    Browsing { cursor: u8 },
    /// Player confirmed a row; shell pushes the sub-session and engines call
    /// [`FieldMenuSession::resume`] when control returns.
    Suspended { row: FieldMenuRow },
    /// Player cancelled out (Circle on Browsing) - shell closes the menu.
    Done(FieldMenuOutcome),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldMenuOutcome {
    /// Player closed the menu without picking anything (or after the
    /// pushed sub-session finished). Shell returns to field tick.
    Closed,
    /// A row was confirmed and the shell wants the resolved row.
    Confirmed(FieldMenuRow),
}

/// Per-frame input bundle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldMenuInput {
    pub up: bool,
    pub down: bool,
    pub cross: bool,
    pub circle: bool,
    pub start: bool,
}

/// Events emitted on `tick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldMenuEvent {
    CursorMoved {
        row: u8,
    },
    Confirmed {
        row: FieldMenuRow,
    },
    InvalidConfirm {
        row: FieldMenuRow,
    },
    Cancelled,
    /// Shell is about to push the matching sub-session.
    EnteringSub {
        row: FieldMenuRow,
    },
    /// Sub-session finished and shell handed control back.
    Resumed {
        row: FieldMenuRow,
    },
}

/// Renderer-agnostic field-menu state machine.
#[derive(Debug, Clone)]
pub struct FieldMenuSession {
    phase: FieldMenuPhase,
    mask: FieldMenuRowMask,
    gate: FieldMenuGate,
    /// Optional gold count to display in the corner. Plain data - engines
    /// pass it through and the renderer view uses it.
    pub money: u32,
    /// Optional play-time-seconds for the corner badge.
    pub play_time_seconds: u32,
}

impl Default for FieldMenuSession {
    fn default() -> Self {
        Self::new()
    }
}

impl FieldMenuSession {
    pub fn new() -> Self {
        Self {
            phase: FieldMenuPhase::Browsing { cursor: 0 },
            mask: FieldMenuRowMask::ALL_ENABLED,
            gate: FieldMenuGate::default(),
            money: 0,
            play_time_seconds: 0,
        }
    }

    pub fn with_mask(mask: FieldMenuRowMask) -> Self {
        let mut s = Self::new();
        s.mask = mask;
        // Make sure cursor lands on an enabled row.
        let first = s.first_enabled_row().index();
        if let FieldMenuPhase::Browsing { cursor } = &mut s.phase {
            *cursor = first;
        }
        s
    }

    pub fn phase(&self) -> FieldMenuPhase {
        self.phase
    }

    pub fn outcome(&self) -> Option<FieldMenuOutcome> {
        match self.phase {
            FieldMenuPhase::Done(o) => Some(o),
            _ => None,
        }
    }

    pub fn cursor(&self) -> u8 {
        match self.phase {
            FieldMenuPhase::Browsing { cursor } => cursor,
            _ => 0,
        }
    }

    pub fn mask(&self) -> &FieldMenuRowMask {
        &self.mask
    }

    pub fn set_mask(&mut self, mask: FieldMenuRowMask) {
        self.mask = mask;
    }

    /// The gate inputs this session was opened with.
    pub fn gate(&self) -> FieldMenuGate {
        self.gate
    }

    /// Install the entry-context / save-permission gate. Hosts call this at
    /// menu-open with the values sampled off the world; a session left at the
    /// default offers every row.
    ///
    /// Unlike [`Self::set_mask`], this does **not** take a row out of the
    /// browse order: retail's picker runs `FUN_801D688C(&DAT_801E46BC, 7, 1)`
    /// over all seven rows and lets the cursor sit on a blocked one, which
    /// then draws grey and buzzes. The mask is the engine's separate "this row
    /// does not exist here" concept and does skip.
    pub fn set_gate(&mut self, gate: FieldMenuGate) {
        self.gate = gate;
    }

    /// Retail's confirm routing for `row` under this session's gate - the
    /// single decision behind both the row's ink and its confirm.
    ///
    /// PORT: FUN_801d6b20 (live wiring; kernel =
    /// [`root_menu_confirm_route`])
    pub fn route_for(&self, row: FieldMenuRow) -> RootMenuRoute {
        root_menu_confirm_route(
            u16::from(row.index()),
            self.gate.entry_context_kind,
            self.gate.save_allowed,
        )
    }

    /// Is `row` offerable: present in the mask **and** not blocked by the
    /// gate. This is the bit the renderer inks - a blocked row draws in the
    /// retail grey (string CLUT `0`) instead of white.
    pub fn row_is_available(&self, row: FieldMenuRow) -> bool {
        self.mask.is_enabled(row) && !matches!(self.route_for(row), RootMenuRoute::Buzz)
    }

    /// The row a confirm on `row` actually enters, resolved **through** the
    /// retail sub-screen id the route table names, or `None` when the row
    /// buzzes. Round-tripping through the id is what makes
    /// [`ROOT_MENU_ROUTES`] load-bearing here rather than decorative.
    fn confirm_target(&self, row: FieldMenuRow) -> Option<FieldMenuRow> {
        if !self.mask.is_enabled(row) {
            return None;
        }
        match self.route_for(row) {
            RootMenuRoute::Sub(sub) => FieldMenuRow::from_retail_subscreen(sub),
            RootMenuRoute::Buzz | RootMenuRoute::None => None,
        }
    }

    pub fn is_done(&self) -> bool {
        matches!(self.phase, FieldMenuPhase::Done(_))
    }

    pub fn is_suspended(&self) -> bool {
        matches!(self.phase, FieldMenuPhase::Suspended { .. })
    }

    fn first_enabled_row(&self) -> FieldMenuRow {
        FieldMenuRow::ALL
            .iter()
            .copied()
            .find(|r| self.mask.is_enabled(*r))
            .unwrap_or(FieldMenuRow::Items)
    }

    fn next_enabled(&self, from: u8, dir: i8) -> u8 {
        let n = FieldMenuRow::ALL.len() as i8;
        let mut i = from as i8;
        for _ in 0..n {
            i = (i + dir).rem_euclid(n);
            if let Some(r) = FieldMenuRow::from_index(i as u8)
                && self.mask.is_enabled(r)
            {
                return i as u8;
            }
        }
        from
    }

    pub fn tick(&mut self, input: FieldMenuInput) -> Vec<FieldMenuEvent> {
        let mut events = Vec::new();
        match self.phase {
            FieldMenuPhase::Browsing { cursor } => {
                if input.circle || input.start {
                    self.phase = FieldMenuPhase::Done(FieldMenuOutcome::Closed);
                    events.push(FieldMenuEvent::Cancelled);
                    return events;
                }
                let mut new_cursor = cursor;
                if input.up {
                    new_cursor = self.next_enabled(cursor, -1);
                } else if input.down {
                    new_cursor = self.next_enabled(cursor, 1);
                }
                if new_cursor != cursor {
                    self.phase = FieldMenuPhase::Browsing { cursor: new_cursor };
                    events.push(FieldMenuEvent::CursorMoved { row: new_cursor });
                }
                if input.cross
                    && let Some(row) = FieldMenuRow::from_index(new_cursor)
                {
                    // Retail's own confirm arm decides: an accepted row plays
                    // cue 0x20 and advances to the routed sub-screen, a
                    // gated one plays the reject cue 0x23 and stays. The
                    // engine's InvalidConfirm is that buzz.
                    match self.confirm_target(row) {
                        Some(target) => {
                            self.phase = FieldMenuPhase::Suspended { row: target };
                            events.push(FieldMenuEvent::Confirmed { row: target });
                            events.push(FieldMenuEvent::EnteringSub { row: target });
                        }
                        None => events.push(FieldMenuEvent::InvalidConfirm { row }),
                    }
                }
            }
            FieldMenuPhase::Suspended { .. } => {
                // Wait for explicit `resume`/`finish`. Input drained.
            }
            FieldMenuPhase::Done(_) => {}
        }
        events
    }

    /// Sub-session finished and shell hands control back. The caller chooses
    /// whether to drop back into Browsing (the default - most sub-sessions
    /// are "do a thing then return to the menu") or close the menu entirely
    /// (e.g. Save → Continue, where the shell wants the field gameplay back).
    pub fn resume(&mut self, close: bool) -> Vec<FieldMenuEvent> {
        let mut events = Vec::new();
        if let FieldMenuPhase::Suspended { row } = self.phase {
            events.push(FieldMenuEvent::Resumed { row });
            if close {
                self.phase = FieldMenuPhase::Done(FieldMenuOutcome::Confirmed(row));
            } else {
                self.phase = FieldMenuPhase::Browsing {
                    cursor: row.index(),
                };
            }
        }
        events
    }
}

/// Plain-data view for the renderer. Engines call [`FieldMenuSession::view`]
/// once per frame and feed the result into `engine-render::field_menu_draws_for`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldMenuView {
    pub rows: [FieldMenuRowView; 7],
    pub cursor: u8,
    pub money: u32,
    pub play_time_seconds: u32,
    pub suspended: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldMenuRowView {
    pub row: FieldMenuRow,
    pub label: &'static str,
    pub enabled: bool,
}

impl FieldMenuSession {
    pub fn view(&self) -> FieldMenuView {
        let mut rows = [FieldMenuRowView {
            row: FieldMenuRow::Items,
            label: "",
            enabled: false,
        }; 7];
        for (i, r) in FieldMenuRow::ALL.iter().enumerate() {
            rows[i] = FieldMenuRowView {
                row: *r,
                label: r.label(),
                enabled: self.row_is_available(*r),
            };
        }
        FieldMenuView {
            rows,
            cursor: self.cursor(),
            money: self.money,
            play_time_seconds: self.play_time_seconds,
            suspended: self.is_suspended(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> FieldMenuInput {
        FieldMenuInput::default()
    }

    #[test]
    fn cursor_moves_on_down_with_event() {
        let mut s = FieldMenuSession::new();
        let evs = s.tick(FieldMenuInput {
            down: true,
            ..input()
        });
        assert_eq!(s.cursor(), 1);
        assert_eq!(evs, vec![FieldMenuEvent::CursorMoved { row: 1 }]);
    }

    #[test]
    fn up_wraps_to_last_row() {
        let mut s = FieldMenuSession::new();
        let _ = s.tick(FieldMenuInput {
            up: true,
            ..input()
        });
        assert_eq!(s.cursor(), 6);
    }

    #[test]
    fn cross_confirms_to_suspended_with_row() {
        let mut s = FieldMenuSession::new();
        let evs = s.tick(FieldMenuInput {
            cross: true,
            ..input()
        });
        assert!(s.is_suspended());
        assert!(matches!(s.phase, FieldMenuPhase::Suspended { row } if row == FieldMenuRow::Items));
        assert!(evs.contains(&FieldMenuEvent::Confirmed {
            row: FieldMenuRow::Items
        }));
        assert!(evs.contains(&FieldMenuEvent::EnteringSub {
            row: FieldMenuRow::Items
        }));
    }

    #[test]
    fn circle_closes_menu() {
        let mut s = FieldMenuSession::new();
        let evs = s.tick(FieldMenuInput {
            circle: true,
            ..input()
        });
        assert!(s.is_done());
        assert_eq!(s.outcome(), Some(FieldMenuOutcome::Closed));
        assert!(evs.contains(&FieldMenuEvent::Cancelled));
    }

    #[test]
    fn disabled_row_skipped_on_cursor_move() {
        let mut mask = FieldMenuRowMask::ALL_ENABLED;
        mask.disable(FieldMenuRow::Magic);
        let mut s = FieldMenuSession::with_mask(mask);
        let _ = s.tick(FieldMenuInput {
            down: true,
            ..input()
        });
        assert_eq!(s.cursor(), FieldMenuRow::Equip.index());
    }

    #[test]
    fn invalid_confirm_on_disabled_does_not_change_phase() {
        // Manually craft an impossible state where cursor sits on a
        // disabled row to make sure InvalidConfirm fires in defence.
        let mut mask = FieldMenuRowMask::ALL_ENABLED;
        mask.disable(FieldMenuRow::Save);
        let mut s = FieldMenuSession::with_mask(mask);
        s.phase = FieldMenuPhase::Browsing {
            cursor: FieldMenuRow::Save.index(),
        };
        let evs = s.tick(FieldMenuInput {
            cross: true,
            ..input()
        });
        assert!(!s.is_suspended());
        assert!(evs.contains(&FieldMenuEvent::InvalidConfirm {
            row: FieldMenuRow::Save
        }));
    }

    #[test]
    fn resume_returns_to_browsing_by_default() {
        let mut s = FieldMenuSession::new();
        let _ = s.tick(FieldMenuInput {
            cross: true,
            ..input()
        });
        let evs = s.resume(false);
        assert!(matches!(s.phase, FieldMenuPhase::Browsing { cursor } if cursor == 0));
        assert!(evs.contains(&FieldMenuEvent::Resumed {
            row: FieldMenuRow::Items
        }));
    }

    #[test]
    fn resume_with_close_closes_menu() {
        let mut s = FieldMenuSession::new();
        let _ = s.tick(FieldMenuInput {
            cross: true,
            ..input()
        });
        let _ = s.resume(true);
        assert_eq!(
            s.outcome(),
            Some(FieldMenuOutcome::Confirmed(FieldMenuRow::Items))
        );
    }

    #[test]
    fn view_reflects_mask_and_cursor() {
        let mut mask = FieldMenuRowMask::ALL_ENABLED;
        mask.disable(FieldMenuRow::Save);
        let mut s = FieldMenuSession::with_mask(mask);
        s.money = 1234;
        s.play_time_seconds = 60;
        let v = s.view();
        assert_eq!(v.money, 1234);
        assert_eq!(v.play_time_seconds, 60);
        assert!(!v.rows[FieldMenuRow::Save.index() as usize].enabled);
        assert!(v.rows[FieldMenuRow::Items.index() as usize].enabled);
    }

    #[test]
    fn first_enabled_row_for_with_mask() {
        let mut mask = FieldMenuRowMask::ALL_ENABLED;
        mask.disable(FieldMenuRow::Items);
        let s = FieldMenuSession::with_mask(mask);
        assert_eq!(s.cursor(), FieldMenuRow::Magic.index());
    }

    /// Retail's list is Items / Magic / Equip / Status / Options / **Load** /
    /// **Save** - `FUN_801CFD68` draws `@Load` at `+0x46` and `@Save` at
    /// `+0x54`, and `FUN_801D6B20` routes row 5 to sub-screen `0x18` (the
    /// load driver) and row 6 to `0x19` (the save driver). Pinning both ends
    /// so the pair cannot silently invert again.
    #[test]
    fn row_five_is_load_and_row_six_is_save() {
        assert_eq!(FieldMenuRow::from_index(5), Some(FieldMenuRow::Load));
        assert_eq!(FieldMenuRow::from_index(6), Some(FieldMenuRow::Save));
        assert_eq!(FieldMenuRow::Load.retail_subscreen(), 0x18);
        assert_eq!(FieldMenuRow::Save.retail_subscreen(), 0x19);
        for r in FieldMenuRow::ALL {
            assert_eq!(
                FieldMenuRow::from_retail_subscreen(r.retail_subscreen()),
                Some(r)
            );
        }
    }

    /// A scene whose MAN clears the save-allow bit greys the Save row and
    /// buzzes its confirm - and leaves the other six rows alone.
    #[test]
    fn no_save_scene_greys_the_save_row_and_buzzes_its_confirm() {
        let mut s = FieldMenuSession::new();
        s.set_gate(FieldMenuGate {
            entry_context_kind: None,
            save_allowed: false,
        });

        let v = s.view();
        assert!(!v.rows[FieldMenuRow::Save.index() as usize].enabled);
        for r in FieldMenuRow::ALL {
            if r != FieldMenuRow::Save {
                assert!(v.rows[r.index() as usize].enabled, "{r:?} must stay white");
            }
        }

        s.phase = FieldMenuPhase::Browsing {
            cursor: FieldMenuRow::Save.index(),
        };
        let evs = s.tick(FieldMenuInput {
            cross: true,
            ..input()
        });
        assert!(
            !s.is_suspended(),
            "a blocked Save must not enter the driver"
        );
        assert_eq!(
            evs,
            vec![FieldMenuEvent::InvalidConfirm {
                row: FieldMenuRow::Save
            }]
        );
    }

    /// The same session with the bit set offers Save normally.
    #[test]
    fn save_allowed_scene_confirms_the_save_row() {
        let mut s = FieldMenuSession::new();
        s.set_gate(FieldMenuGate {
            entry_context_kind: None,
            save_allowed: true,
        });
        s.phase = FieldMenuPhase::Browsing {
            cursor: FieldMenuRow::Save.index(),
        };
        let evs = s.tick(FieldMenuInput {
            cross: true,
            ..input()
        });
        assert!(s.view().rows[FieldMenuRow::Save.index() as usize].enabled);
        assert!(evs.contains(&FieldMenuEvent::Confirmed {
            row: FieldMenuRow::Save
        }));
        assert!(matches!(s.phase, FieldMenuPhase::Suspended { row } if row == FieldMenuRow::Save));
    }

    /// Retail's picker navigates all seven rows unconditionally
    /// (`FUN_801D688C(&DAT_801E46BC, 7, 1)`); a gated row draws grey but the
    /// cursor still lands on it. Only the engine's own row mask skips.
    #[test]
    fn a_gate_blocked_row_stays_navigable() {
        let mut s = FieldMenuSession::new();
        s.set_gate(FieldMenuGate {
            entry_context_kind: None,
            save_allowed: false,
        });
        // Up from Items wraps onto the last row, which is the blocked Save.
        let _ = s.tick(FieldMenuInput {
            up: true,
            ..input()
        });
        assert_eq!(s.cursor(), FieldMenuRow::Save.index());
        assert!(!s.row_is_available(FieldMenuRow::Save));
    }

    /// The Load half of the same gate: an entry context of kind `0x0D` blocks
    /// it, any other kind (and no context at all) does not.
    #[test]
    fn locked_entry_context_greys_only_the_load_row() {
        let mut s = FieldMenuSession::new();
        s.set_gate(FieldMenuGate {
            entry_context_kind: Some(crate::pause_screens::ROOT_MENU_CONTEXT_LOCKED),
            save_allowed: true,
        });
        let v = s.view();
        assert!(!v.rows[FieldMenuRow::Load.index() as usize].enabled);
        assert!(v.rows[FieldMenuRow::Save.index() as usize].enabled);

        // Sub-op 0 (an armed inline shop) is not the blocking kind.
        s.set_gate(FieldMenuGate {
            entry_context_kind: Some(0),
            save_allowed: true,
        });
        assert!(s.row_is_available(FieldMenuRow::Load));
    }

    /// A row the engine's mask removes stays removed even when the retail
    /// gate would allow it - the two concepts compose rather than override.
    #[test]
    fn the_mask_and_the_gate_compose() {
        let mut mask = FieldMenuRowMask::ALL_ENABLED;
        mask.disable(FieldMenuRow::Magic);
        let mut s = FieldMenuSession::with_mask(mask);
        s.set_gate(FieldMenuGate {
            entry_context_kind: None,
            save_allowed: false,
        });
        assert!(!s.row_is_available(FieldMenuRow::Magic));
        assert!(!s.row_is_available(FieldMenuRow::Save));
        assert!(s.row_is_available(FieldMenuRow::Items));
    }

    #[test]
    fn start_also_closes_like_circle() {
        let mut s = FieldMenuSession::new();
        let _ = s.tick(FieldMenuInput {
            start: true,
            ..input()
        });
        assert!(s.is_done());
    }
}
