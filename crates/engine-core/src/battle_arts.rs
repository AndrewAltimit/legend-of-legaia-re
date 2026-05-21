//! Player-driven battle Arts submenu.
//!
//! Sibling of [`crate::battle_magic::BattleSpellSession`] for the **Arts**
//! command. In Legaia an Art is a chain of directional inputs; this engine
//! stores them as named [`legaia_save::SavedChainRecord`]s in the per-character
//! chain library. The Arts submenu lists the caster's saved chains, opens a
//! single-enemy [`crate::target_picker::TargetPickerSession`], and resolves to
//! a `(art_index, target)` the live loop executes through the **real art-power
//! path** ([`crate::art_strike::apply_art_strike`]), then cycles the turn at
//! `EndOfAction`.
//!
//! Each [`ArtRow`] carries a per-strike **power profile** ([`PowerByte`]s) plus
//! the art's [`EnemyEffect`]. Two sources feed it:
//!
//! - **Real art record.** When the World has an [`ArtRecord`] whose command
//!   string the saved chain ends with ([`chain_matches_record`]), the row uses
//!   that record's damage power bytes + status effect ([`power_from_record`]).
//!   This is the disc-data path (art records live in PROT entry `0x05C4`); the
//!   live loop then deals real multiplier-tier, UDF/LDF-targeted damage.
//! - **Synthetic fallback.** With no matching record (the disc art tables
//!   aren't loaded), [`synthetic_power`] maps each directional command to a
//!   tier-0 (×12) hit - Down → LDF (low attack), everything else → UDF - so a
//!   saved chain is still playable through the same `apply_art_strike` kernel.
//!
//! The World owns the session because building the rows needs both the caster's
//! saved-chain library and the art-record catalog. The session is renderer- and
//! world-agnostic.

use crate::target_picker::{
    CursorRow, PickerInput, PickerOutcome, SlotState, TargetKind, TargetPickerSession,
};
use legaia_art::power::PowerByte;
use legaia_art::queue::Command;
use legaia_art::{ArtRecord, EnemyEffect};

/// Maximum hits one art resolves to in the live loop, so a pathological saved
/// chain (or art record) can't deal unbounded damage in a single turn.
pub const MAX_ART_HITS: u8 = 16;

/// Power byte used for a synthetic UDF (high) hit - tier 0, ×12. See
/// [`legaia_art::power`] for the byte encoding.
const SYNTH_UDF_X12: u8 = 0x16;
/// Power byte used for a synthetic LDF (low) hit - tier 0, ×12.
const SYNTH_LDF_X12: u8 = 0x1B;

/// One selectable art (saved chain) row in the battle Arts menu.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtRow {
    /// Display name of the saved chain.
    pub name: String,
    /// Per-strike damage power bytes the art deals, in strike order. Driven
    /// through [`crate::art_strike::apply_art_strike`] when the art runs.
    pub power: Vec<PowerByte>,
    /// Status effect the art inflicts on hit (if any).
    pub enemy_effect: EnemyEffect,
}

impl ArtRow {
    /// Number of damaging strikes this art deals (its damage-power-byte count,
    /// clamped to `1..=MAX_ART_HITS`). Used for the menu's hit-count display.
    pub fn hits(&self) -> u8 {
        let n = self.power.iter().filter(|p| p.is_damage()).count();
        (n as u8).clamp(1, MAX_ART_HITS)
    }
}

/// Sub-phase of the battle Arts submenu.
#[derive(Debug, Clone)]
pub enum ArtsPhase {
    /// Browsing the saved-chain list. `cursor` indexes
    /// [`BattleArtsSession::arts`].
    Select { cursor: u8 },
    /// An art is chosen; picking its target.
    Targeting {
        art_index: u8,
        picker: TargetPickerSession,
    },
    /// Resolved: the live loop should execute art `art_index` against the
    /// target.
    Confirmed {
        art_index: u8,
        target_row: CursorRow,
        target_slot: u8,
    },
    /// Backed out of the list (Circle, or no saved chain to run) - the live
    /// loop reopens the command menu.
    Aborted,
}

/// Per-frame, edge-triggered pad bundle for the arts session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BattleArtsInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// Confirm (Cross).
    pub cross: bool,
    /// Cancel / back (Circle).
    pub circle: bool,
}

/// One caster's art-selection session, driven a frame at a time.
#[derive(Debug, Clone)]
pub struct BattleArtsSession {
    /// Actor-table index of the casting party member.
    pub actor: u8,
    /// Party-row index (0..=2) of the caster.
    pub party_slot: u8,
    /// The caster's selectable arts, in library order.
    pub arts: Vec<ArtRow>,
    pub phase: ArtsPhase,
}

/// Outcome of a resolved [`BattleArtsSession`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtsResolution {
    /// The player confirmed an art execution.
    Confirmed {
        art_index: u8,
        target_row: CursorRow,
        target_slot: u8,
    },
    /// The player backed out; the live loop reopens the command menu.
    Aborted,
}

/// Count a packed chain's non-terminator commands, clamped to a usable hit
/// count. `0` bytes are terminators / empty slots.
pub fn chain_hit_count(sequence: &[u8]) -> u8 {
    let n = sequence.iter().filter(|&&b| b != 0).count();
    (n as u8).clamp(1, MAX_ART_HITS)
}

/// Build a synthetic per-strike power profile from a saved chain's command
/// bytes - the no-art-record fallback. Each non-terminator directional command
/// becomes one tier-0 (×12) hit: `Down` targets LDF (a low attack), every other
/// direction targets UDF (a high attack). Clamped to [`MAX_ART_HITS`]; always
/// at least one hit so an empty / all-terminator chain is still playable.
pub fn synthetic_power(sequence: &[u8]) -> Vec<PowerByte> {
    let mut out = Vec::new();
    for &b in sequence {
        if b == 0 {
            continue;
        }
        let byte = match Command::from_byte(b) {
            Some(Command::Down) => SYNTH_LDF_X12,
            Some(Command::Up) | Some(Command::Left) | Some(Command::Right) => SYNTH_UDF_X12,
            None => continue,
        };
        out.push(PowerByte::from_byte(byte));
        if out.len() >= MAX_ART_HITS as usize {
            break;
        }
    }
    if out.is_empty() {
        out.push(PowerByte::from_byte(SYNTH_UDF_X12));
    }
    out
}

/// Extract the damaging power bytes + status effect from a decoded
/// [`ArtRecord`]. Non-damage (terminator) power bytes are dropped; the result
/// is clamped to [`MAX_ART_HITS`] and floored at one hit.
pub fn power_from_record(rec: &ArtRecord) -> (Vec<PowerByte>, EnemyEffect) {
    let mut power: Vec<PowerByte> = rec
        .power
        .iter()
        .copied()
        .filter(|p| p.is_damage())
        .take(MAX_ART_HITS as usize)
        .collect();
    if power.is_empty() {
        power.push(PowerByte::from_byte(SYNTH_UDF_X12));
    }
    (power, rec.enemy_effect)
}

/// `true` iff the saved chain's command string ends with the record's command
/// string - the way a directional chain triggers an art in retail (the art
/// fires at the tail of the inputs). A record with no command string never
/// matches (Super / Miracle finishers are invoked by a different path).
pub fn chain_matches_record(sequence: &[u8], rec: &ArtRecord) -> bool {
    if rec.commands.is_empty() {
        return false;
    }
    let chain: Vec<u8> = sequence.iter().copied().filter(|&b| b != 0).collect();
    let want: Vec<u8> = rec.commands.iter().map(|c| c.as_byte()).collect();
    chain.ends_with(&want)
}

/// Build synthetic [`ArtRow`]s from a caster's saved chains (no art records) -
/// convenience for the no-disc-data path and tests.
pub fn rows_from_chains(actor: u8, chains: &[legaia_save::SavedChainRecord]) -> Vec<ArtRow> {
    chains
        .iter()
        .filter(|c| c.char_slot == actor)
        .map(|c| ArtRow {
            name: c.name.clone(),
            power: synthetic_power(&c.sequence),
            enemy_effect: EnemyEffect::None,
        })
        .collect()
}

impl BattleArtsSession {
    /// Build from prebuilt rows (the World resolves each saved chain's power
    /// profile from its art-record catalog before constructing the session).
    /// The cursor starts at row 0.
    pub fn new(actor: u8, party_slot: u8, arts: Vec<ArtRow>) -> Self {
        Self {
            actor,
            party_slot,
            arts,
            phase: ArtsPhase::Select { cursor: 0 },
        }
    }

    /// The art row currently under the select cursor, or `None` once the
    /// session has left the list.
    pub fn menu_art(&self) -> Option<&ArtRow> {
        match self.phase {
            ArtsPhase::Select { cursor } => self.arts.get(cursor as usize),
            _ => None,
        }
    }

    /// The active target picker, while one is open.
    pub fn picker(&self) -> Option<&TargetPickerSession> {
        match &self.phase {
            ArtsPhase::Targeting { picker, .. } => Some(picker),
            _ => None,
        }
    }

    /// The resolved execution / abort, or `None` while still selecting.
    pub fn resolved(&self) -> Option<ArtsResolution> {
        match &self.phase {
            ArtsPhase::Confirmed {
                art_index,
                target_row,
                target_slot,
            } => Some(ArtsResolution::Confirmed {
                art_index: *art_index,
                target_row: *target_row,
                target_slot: *target_slot,
            }),
            ArtsPhase::Aborted => Some(ArtsResolution::Aborted),
            _ => None,
        }
    }

    /// Advance one frame. `party` / `monsters` describe slot occupancy + alive
    /// state for the target picker. A no-op once the session has resolved.
    pub fn input(&mut self, ev: BattleArtsInput, party: [SlotState; 3], monsters: [SlotState; 5]) {
        match std::mem::replace(&mut self.phase, ArtsPhase::Aborted) {
            ArtsPhase::Select { cursor } => {
                self.phase = step_select(cursor, ev, self.party_slot, &self.arts, party, monsters);
            }
            ArtsPhase::Targeting {
                art_index,
                mut picker,
            } => {
                picker.input(PickerInput {
                    up: ev.up,
                    down: ev.down,
                    left: ev.left,
                    right: ev.right,
                    cross: ev.cross,
                    circle: ev.circle,
                });
                self.phase = match picker.outcome() {
                    Some(PickerOutcome::Single { slot, row }) => ArtsPhase::Confirmed {
                        art_index,
                        target_row: row,
                        target_slot: slot,
                    },
                    Some(PickerOutcome::Sweep { row }) => ArtsPhase::Confirmed {
                        art_index,
                        target_row: row,
                        target_slot: 0,
                    },
                    Some(PickerOutcome::Cancelled) => ArtsPhase::Select { cursor: art_index },
                    Some(PickerOutcome::NoCandidates) => ArtsPhase::Aborted,
                    None => ArtsPhase::Targeting { art_index, picker },
                };
            }
            other => self.phase = other,
        }
    }
}

/// One frame of the art-select list. Up/Down move the cursor (wrapping); Cross
/// opens a single-enemy target picker for the highlighted art; Circle backs
/// out. An empty list backs out on any confirm so the player isn't trapped.
fn step_select(
    cursor: u8,
    ev: BattleArtsInput,
    party_slot: u8,
    arts: &[ArtRow],
    party: [SlotState; 3],
    monsters: [SlotState; 5],
) -> ArtsPhase {
    let n = arts.len();
    if n == 0 {
        if ev.circle || ev.cross {
            return ArtsPhase::Aborted;
        }
        return ArtsPhase::Select { cursor: 0 };
    }
    let mut cursor = (cursor as usize).min(n - 1);
    if ev.up {
        cursor = (cursor + n - 1) % n;
    } else if ev.down {
        cursor = (cursor + 1) % n;
    }
    if ev.circle {
        return ArtsPhase::Aborted;
    }
    if ev.cross {
        let picker = TargetPickerSession::new(TargetKind::SingleEnemy, party_slot, party, monsters);
        if let Some(outcome) = picker.outcome() {
            return match outcome {
                PickerOutcome::Single { slot, row } => ArtsPhase::Confirmed {
                    art_index: cursor as u8,
                    target_row: row,
                    target_slot: slot,
                },
                PickerOutcome::Sweep { row } => ArtsPhase::Confirmed {
                    art_index: cursor as u8,
                    target_row: row,
                    target_slot: 0,
                },
                PickerOutcome::NoCandidates => ArtsPhase::Aborted,
                PickerOutcome::Cancelled => ArtsPhase::Select {
                    cursor: cursor as u8,
                },
            };
        }
        return ArtsPhase::Targeting {
            art_index: cursor as u8,
            picker,
        };
    }
    ArtsPhase::Select {
        cursor: cursor as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_art::power::{PowerByte, PowerTarget};
    use legaia_art::queue::ActionConstant;
    use legaia_save::SavedChainRecord;

    fn alive(present: bool) -> SlotState {
        SlotState::alive(present, true)
    }
    fn party3() -> [SlotState; 3] {
        [alive(true), alive(true), alive(true)]
    }
    fn one_monster() -> [SlotState; 5] {
        [
            alive(true),
            SlotState::default(),
            SlotState::default(),
            SlotState::default(),
            SlotState::default(),
        ]
    }
    fn press(b: &str) -> BattleArtsInput {
        BattleArtsInput {
            up: b == "U",
            down: b == "D",
            cross: b == "c",
            circle: b == "o",
            ..Default::default()
        }
    }
    fn chain(char_slot: u8, name: &str, seq: &[u8]) -> SavedChainRecord {
        SavedChainRecord {
            char_slot,
            name: name.into(),
            sequence: seq.to_vec(),
        }
    }
    fn target_of(pb: PowerByte) -> Option<PowerTarget> {
        match pb {
            PowerByte::Damage(p) => Some(p.target),
            PowerByte::NoDamage => None,
        }
    }

    #[test]
    fn hit_count_counts_nonterminator_commands() {
        assert_eq!(chain_hit_count(&[1, 2, 3]), 3);
        assert_eq!(chain_hit_count(&[1, 2, 0, 0]), 2);
        assert_eq!(chain_hit_count(&[]), 1, "floors at one hit");
        assert_eq!(chain_hit_count(&[0; 64]), 1);
        assert_eq!(chain_hit_count(&[7; 64]), MAX_ART_HITS, "clamped");
    }

    #[test]
    fn synthetic_power_maps_direction_to_udf_ldf() {
        // Left, Right, Up -> UDF; Down -> LDF. (1=Left,2=Right,3=Down,4=Up)
        let p = synthetic_power(&[4, 3, 1]);
        assert_eq!(p.len(), 3);
        assert_eq!(target_of(p[0]), Some(PowerTarget::Udf), "Up -> UDF");
        assert_eq!(target_of(p[1]), Some(PowerTarget::Ldf), "Down -> LDF");
        assert_eq!(target_of(p[2]), Some(PowerTarget::Udf), "Left -> UDF");
        // Terminators dropped; empty -> one hit floor.
        assert_eq!(synthetic_power(&[0, 0]).len(), 1);
        // Clamped to MAX_ART_HITS.
        assert_eq!(synthetic_power(&[2; 64]).len(), MAX_ART_HITS as usize);
    }

    #[test]
    fn power_from_record_keeps_damage_bytes_and_effect() {
        let rec = ArtRecord {
            action: ActionConstant::Art1B,
            commands: vec![Command::Up, Command::Up],
            anim_index: 0,
            anim_extra: vec![],
            name: None,
            // One damage byte (UDF x28) + one terminator (no-damage).
            power: vec![PowerByte::from_byte(0x1A), PowerByte::from_byte(0x00)],
            dmg_timing: vec![],
            effect_cues: Default::default(),
            hit_cues: vec![],
            identifier: 0,
            anim_speed: 0,
            enemy_effect: EnemyEffect::Burned,
            repeat_frames: Default::default(),
            background: 0,
            runtime_address: None,
        };
        let (power, effect) = power_from_record(&rec);
        assert_eq!(power.len(), 1, "no-damage byte dropped");
        assert_eq!(target_of(power[0]), Some(PowerTarget::Udf));
        assert_eq!(effect, EnemyEffect::Burned);
    }

    #[test]
    fn chain_matches_record_on_command_tail() {
        let rec = ArtRecord {
            action: ActionConstant::Art1B,
            commands: vec![Command::Up, Command::Up],
            anim_index: 0,
            anim_extra: vec![],
            name: None,
            power: vec![PowerByte::from_byte(0x1A)],
            dmg_timing: vec![],
            effect_cues: Default::default(),
            hit_cues: vec![],
            identifier: 0,
            anim_speed: 0,
            enemy_effect: EnemyEffect::None,
            repeat_frames: Default::default(),
            background: 0,
            runtime_address: None,
        };
        // Up=4. Chain "Left, Up, Up" ends with the record's "Up, Up".
        assert!(chain_matches_record(&[1, 4, 4], &rec));
        assert!(chain_matches_record(&[4, 4], &rec));
        assert!(!chain_matches_record(&[4, 4, 1], &rec), "tail must match");
        // Empty command string never matches.
        let mut empty = rec.clone();
        empty.commands = vec![];
        assert!(!chain_matches_record(&[4, 4], &empty));
    }

    #[test]
    fn rows_from_chains_lists_only_the_casters_chains() {
        let chains = [
            chain(0, "Vahn-A", &[1, 2]),
            chain(1, "Noa-A", &[3, 4, 1]),
            chain(0, "Vahn-B", &[1, 1, 1, 1]),
        ];
        let rows = rows_from_chains(0, &chains);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "Vahn-A");
        assert_eq!(rows[1].hits(), 4);
    }

    #[test]
    fn confirm_art_then_target_resolves() {
        let rows = rows_from_chains(0, &[chain(0, "Vahn-A", &[1, 2, 3])]);
        let mut s = BattleArtsSession::new(0, 0, rows);
        s.input(press("c"), party3(), one_monster());
        assert!(matches!(s.phase, ArtsPhase::Targeting { .. }));
        s.input(press("c"), party3(), one_monster());
        assert_eq!(
            s.resolved(),
            Some(ArtsResolution::Confirmed {
                art_index: 0,
                target_row: CursorRow::Enemy,
                target_slot: 0,
            })
        );
    }

    #[test]
    fn circle_aborts_and_empty_list_aborts_on_confirm() {
        let rows = rows_from_chains(0, &[chain(0, "Vahn-A", &[1])]);
        let mut s = BattleArtsSession::new(0, 0, rows);
        s.input(press("o"), party3(), one_monster());
        assert_eq!(s.resolved(), Some(ArtsResolution::Aborted));

        // A caster with no chains backs out on Cross.
        let mut empty = BattleArtsSession::new(2, 2, Vec::new());
        assert!(empty.arts.is_empty());
        empty.input(press("c"), party3(), one_monster());
        assert_eq!(empty.resolved(), Some(ArtsResolution::Aborted));
    }
}
