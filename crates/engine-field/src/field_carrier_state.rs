//! Field-scene carrier entities: the per-entity FUN_801DA51C state machines ticked in field scenes and their battle / engage handoffs.
//!
//! One subsystem's state, held as a unit on the composite `World`.
//! Fields keep their retail provenance notes.

use crate::man_field_scripts::FieldCarrierConfig;

/// Field-scene carrier entities: the per-entity FUN_801DA51C state machines ticked in field scenes and their battle / engage handoffs.
pub struct FieldCarrierState {
    /// Per-entity **field** state machines - the same `FUN_801DA51C` SM the
    /// overworld uses ([`crate::world_map_entity`]), but ticked in [`legaia_engine_mode::mode::SceneMode::Field`]
    /// for the scene's MAN-placed actors. A scripted-encounter carrier (the
    /// Rim Elm Tetsu fight) sits Idle until `World::engage_field_carrier`
    /// (the dialogue-accept) advances it to `Activating`; the next
    /// `Self::tick_field_carriers` then copies its formation and launches the
    /// battle, mirroring retail's state-1 `entity[+0x94]` copy + `case 2/3`
    /// fall-through battle handoff. Empty unless
    /// `World::install_field_carriers` seeded them.
    pub entities: Vec<crate::world_map_entity::WorldMapEntityCtx>,
    /// Per-carrier role config, paired by index with [`FieldCarrierState::entities`].
    pub configs: Vec<FieldCarrierConfig>,
    /// Field carrier battle pending resolution: the MAN `formation_id` a
    /// carrier SM latched on its scene-transition this frame. Drained at the
    /// end of `Self::tick_field_carriers` to flip Field -> Battle. `None`
    /// between transitions.
    pub pending_battle: Option<u16>,
    /// Field-interact `slot` -> [`FieldCarrierState::entities`] index, for the
    /// **scripted-encounter** carriers only. Built by
    /// `World::install_field_carriers_from_man` so a field-interact on the
    /// sparring partner's placement can find its carrier and auto-arm the fight
    /// (the dialogue-accept drives the engage instead of the manual API). Plain
    /// talk NPCs are deliberately absent - interacting with them never launches
    /// a battle.
    pub slots: std::collections::HashMap<u8, usize>,
    /// A scripted-encounter carrier whose dialogue the player opened via a
    /// field-interact and which engages when that dialogue is dismissed (the
    /// accept). Set in `World::trigger_field_interact`, consumed
    /// by the dialog-advance dismiss (`op 0x4C n5 sub-4`). `None` when no
    /// scripted carrier's prompt is up.
    ///
    /// This any-accept path is used for a carrier whose dialogue has **no
    /// picker**. The Rim Elm spar's dialogue *does* (a 4-option menu whose
    /// index-2 entry "I want to practice with you." arms the fight), so it takes
    /// the faithful [`FieldCarrierState::menu`] path instead - the engage there fires
    /// only on the fight option, matching retail (live-pinned by
    /// `autorun_tetsu_confirm.lua`: a dialog-SM inline picker, cursor at
    /// `*(0x801C6EA4)+0x0C`, confirming index 2 drives `0x03 -> 0x09 -> 0x15`).
    pub pending_engage: Option<usize>,
    /// The faithful counterpart to [`FieldCarrierState::pending_engage`]: when the
    /// opened carrier dialogue carries a 4-option picker (the Rim Elm spar menu),
    /// this holds the live menu so the engage fires **only** on the fight option
    /// ("I want to practice with you.", picker index 2 - RE-pinned live by
    /// `autorun_tetsu_confirm.lua`), not on any accept. `None` when the carrier
    /// has no picker (then `pending_carrier_engage` keeps the any-accept path).
    pub menu: Option<CarrierMenu>,
}

impl FieldCarrierState {
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
            configs: Vec::new(),
            pending_battle: None,
            slots: std::collections::HashMap::new(),
            pending_engage: None,
            menu: None,
        }
    }
}

impl Default for FieldCarrierState {
    fn default() -> Self {
        Self::new()
    }
}

/// A live scripted-encounter carrier menu: the 4-option picker its dialogue
/// presents (the Rim Elm spar's "hear about Biron / secrets of fighting /
/// **practice with you** / nothing"). Navigated with Up/Down; confirming the
/// `fight_option` index engages the carrier (-> Battle), any other option just
/// closes (its talk branch). Mirrors the retail inline-picker cursor
/// `*(0x801C6EA4)+0x0C`; the engine can't run the option's field-VM branch, so it
/// gates the engage on `fight_option` directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CarrierMenu {
    /// Index into `FieldCarrierState::entities` this menu engages.
    pub carrier_idx: usize,
    /// Number of options in the picker (4 for the spar).
    pub n: usize,
    /// The picker option whose confirm engages the carrier (the fight choice).
    pub fight_option: usize,
    /// Highlighted option `0..n`.
    pub cursor: usize,
}

/// The field-VM scripted-battle install prefix (`3E FF <formation-row>`): the
/// op `FUN_801DA51C` uses to arm a formation-row fight (the Rim Elm spar's
/// `3E FF 04` = row 4 lone Tetsu; garmel's Zeto `3E FF 09`; rikuroa's Caruban
/// `3E FF 11` - see `rim_elm_sparring_carrier`). Matching the two-byte prefix
/// is row-agnostic, so it recognises any scripted-battle branch.
const SCRIPTED_BATTLE_INSTALL: [u8; 2] = [0x3E, 0xFF];

/// Bytes to scan from an option's branch target when hunting the install, if no
/// nearer branch bounds the window first. A spar reply branch is short; a span
/// this size can't reach into an unrelated later region.
const SPAR_BRANCH_SCAN_SPAN: usize = 256;

/// If `dialogue` presents the spar's 4-option picker, return `(n_options,
/// fight_option_index)`. The fight option is disc-derived: the choice whose
/// branch (at its [`legaia_mes::Picker::jump_target`]) installs a scripted
/// battle (`3E FF _`, the exact op the RE pinned for the Rim Elm training
/// fight). Because that ties the choice to a disc op rather than an English
/// label, it holds under translation packs / PAL discs. The `"practice"` label
/// match is kept only as a last-resort fallback for a buffer whose branch
/// bytes were truncated away (so no install is present to key on). `None` when
/// the dialogue carries no such picker, so a carrier without a menu keeps the
/// any-accept engage path.
pub fn spar_menu_of(dialogue: &[u8]) -> Option<(usize, usize)> {
    for p in legaia_mes::scan_pickers(dialogue) {
        if p.n != 4 {
            continue;
        }
        // Resolve each option's branch entry offset into this same buffer.
        // `jump_target` shares `dialogue`'s coordinate space (both `open` and
        // the returned offset index it), so an out-of-range target is dropped.
        let targets: Vec<usize> = (0..p.n)
            .map(|i| p.jump_target(i).unwrap_or(usize::MAX))
            .collect();
        // Disc-derived fight option: its branch carries the scripted-battle
        // install. Bound each option's scan to the start of the nearest later
        // branch (so a lower option's window can't run into a higher option's
        // install) capped by a fixed span and the buffer end.
        let fight = (0..p.n).find(|&i| {
            let t = targets[i];
            if t >= dialogue.len() {
                return false;
            }
            let next = targets
                .iter()
                .copied()
                .filter(|&o| o > t && o <= dialogue.len())
                .min()
                .unwrap_or(dialogue.len());
            let end = next
                .min(t.saturating_add(SPAR_BRANCH_SCAN_SPAN))
                .min(dialogue.len());
            dialogue[t..end]
                .windows(2)
                .any(|w| w == SCRIPTED_BATTLE_INSTALL)
        });
        if let Some(f) = fight {
            return Some((p.n, f));
        }
        // Last-resort fallback: the English "practice" label.
        if let Some(f) = p.options.iter().position(|o| {
            o.label
                .windows(8)
                .any(|w| w.eq_ignore_ascii_case(b"practice"))
        }) {
            return Some((p.n, f));
        }
    }
    None
}
