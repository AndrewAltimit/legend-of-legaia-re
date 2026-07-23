//! Out-of-battle spell menu.
//!
//! Field-side spell casting flow: pick caster → pick spell → pick target
//! → resolve via [`crate::spells::cast_spell`]. Only spells whose effect
//! is field-meaningful (Heal / HealAll / Cure / Revive) are admissible
//! in this context - damage / capture / escape spells filter out.
//!
//! ## States
//!
//! `CharSelect → SpellSelect → TargetSelect → Done(SpellOutcome | Cancelled)`
//!
//! Each state honours Circle as "back one step"; Circle from CharSelect
//! cancels out (Done(Cancelled)). The session never mutates the world -
//! it returns a typed [`SpellMenuOutcome`] and engines apply it.
//!
//! ## Retail sub-screen mapping
//!
//! Retail runs the same chain as four menu-overlay sub-screens off the
//! `0x801E4F40` pointer table:
//!
//! - `0x0E` caster picker (`FUN_801D8F10`): `FUN_801D688C` over the
//!   roster; confirm gated on the spell count `record[0x13C]` **and** the
//!   Ra-Seru equip slot, buzz `0x23` on either, SFX `0x20` + sub-screen
//!   `0x0F` on pass.
//! - `0x0F` spell list (`FUN_801D9110`): the kind-4 list window (content
//!   id `5`); cancel parks the list (mode 4) back to `0x0E`; confirm
//!   routes on the spell-stat flag ([`spell_targets_group`]) to `0x10`
//!   or `0x11`.
//! - `0x10` group-cast flow (`FUN_801D9280`): no target rows
//!   (`FUN_801D688C(count = 0)` - confirm/cancel only), SFX `0x25` on
//!   commit, cancel returns to `0x0F` with the editing bit raised.
//! - `0x11` single-target pick + apply (`FUN_801D9594`): target rows over
//!   the party count; the commit revalidates through the target-relevance
//!   probe (`FUN_8003FB10`, buzz on nobody-affected), costs MP through
//!   the per-caster kernel (`FUN_80035394`) and applies through the
//!   shared field applier (`FUN_800402F4`).
//!
//! PORT: FUN_801d8f10 (CharSelect phase incl. both confirm gates)
//! PORT: FUN_801d9110 (SpellSelect phase + [`spell_targets_group`])
//! PORT: FUN_801d9280 (group-target confirm/cancel shape)
//! PORT: FUN_801d9594 (TargetSelect + resolve; the engine folds the two
//! retail target flows into one `TargetSelect` phase and resolves group
//! spells against every live row - see `dispatch` on the outcome side)
//! REF: FUN_801d688c (cursor navigator) / FUN_8003fb10 (relevance probe)
//! / FUN_80035394 (MP-cost kernel) / FUN_800402f4 (field applier)

use crate::input::PadButton;
use crate::spells::{SpellCatalog, SpellEffect, SpellOutcome};

/// Rows per page of the retail Magic-screen spell list (the capture-pinned
/// list-page layout, shared with the Items list - see
/// `docs/subsystems/field-menu.md`).
pub const SPELL_LIST_PAGE_ROWS: usize = 12;

/// Per-character roster row. Engines feed in the active party with each
/// character's current MP / HP so the menu can grey out casters who are
/// dead or out of MP for the cheapest spell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CasterSlot {
    pub slot: u8,
    pub name: String,
    pub hp: u16,
    pub mp: u16,
    /// Max HP (retail record `+0x104`; the HP/MP/AP pairs are (max, cur)
    /// order - `legaia_save::HpMpSp`).
    pub hp_max: u16,
    /// Max MP (record `+0x108`) - the Magic screen's caster window draws
    /// `cur / max` through the MP tier ink (`FUN_801D2C98`).
    pub mp_max: u16,
    /// Displayed level (record `+0x130`, [`legaia_save::CharacterRecord::magic_rank`]).
    pub level: u8,
    /// Spell ids the caster has access to. Engines build this from the
    /// per-character `learned_spells` list.
    pub spells: Vec<u8>,
    /// Learned spell level per entry of `spells` (record `+0x161` list,
    /// parallel to `+0x13D` ids - the "Lv n" the spell info window shows).
    /// May be empty (level defaults to 1).
    pub spell_levels: Vec<u8>,
    /// Caster's active-abilities bitfield word (record `+0xF4`, mirrored in
    /// `World::character_ability_bits`). The Magic screen discounts each
    /// displayed MP cost through the per-caster MP-cost kernel
    /// (`FUN_80035394`): bit `0x20` = half cost, bit `0x10` = quarter off,
    /// Half winning when both are set. Defaults to `0` (no discount).
    pub ability_bits: u32,
    /// `true` when the caster's Ra-Seru equip slot (`record[0x196 +
    /// *(i16*)(0x8007B424 + char*2)]`) is empty - retail refuses such a
    /// caster with a buzz (`InvalidReason::NoRaSeru`). Default `false`
    /// (equipped), matching the retail party's normal state.
    pub ra_seru_missing: bool,
}

impl CasterSlot {
    pub fn alive(&self) -> bool {
        self.hp > 0
    }

    /// Learned level of the `idx`-th spell (1 when the level list is
    /// absent / short - a freshly-learned spell is level 1).
    pub fn spell_level(&self, idx: usize) -> u8 {
        match self.spell_levels.get(idx).copied() {
            Some(l) if l > 0 => l,
            _ => 1,
        }
    }
}

/// Per-target-row data for the in-menu target picker. Engines feed live
/// HP so the renderer can render greyed-out dead targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRow {
    pub slot: u8,
    pub name: String,
    pub hp: u16,
    pub hp_max: u16,
}

impl TargetRow {
    pub fn alive(&self) -> bool {
        self.hp > 0
    }
}

/// Phase of the field spell menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpellMenuPhase {
    CharSelect {
        cursor: u8,
    },
    SpellSelect {
        caster: u8,
        cursor: u8,
    },
    TargetSelect {
        caster: u8,
        spell_id: u8,
        cursor: u8,
    },
    Done(SpellMenuOutcome),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpellMenuOutcome {
    Cast {
        caster_slot: u8,
        spell_id: u8,
        target_slot: u8,
        outcome: SpellOutcome,
    },
    Cancelled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpellMenuInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub cross: bool,
    pub circle: bool,
    pub start: bool,
}

impl SpellMenuInput {
    pub fn from_pad_edge(pressed: u16) -> Self {
        Self {
            up: pressed & PadButton::Up.mask() != 0,
            down: pressed & PadButton::Down.mask() != 0,
            left: pressed & PadButton::Left.mask() != 0,
            right: pressed & PadButton::Right.mask() != 0,
            cross: pressed & PadButton::Cross.mask() != 0,
            circle: pressed & PadButton::Circle.mask() != 0,
            start: pressed & PadButton::Start.mask() != 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpellMenuEvent {
    CursorMoved {
        cursor: u8,
    },
    EnteredSpellSelect {
        caster: u8,
    },
    EnteredTargetSelect {
        caster: u8,
        spell_id: u8,
    },
    Cast {
        caster_slot: u8,
        spell_id: u8,
        target_slot: u8,
    },
    InvalidConfirm {
        reason: InvalidReason,
    },
    Backed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidReason {
    DeadCaster,
    EmptySpellList,
    NotEnoughMp,
    NotFieldUsable,
    DeadTarget,
    InvalidTarget,
    UnknownSpell,
    /// Caster's Ra-Seru equip slot is empty - retail's second confirm
    /// gate on the Magic caster picker (`record[0x196 +
    /// *(i16*)(0x8007B424 + char*2)] == 0` buzzes `0x23` at
    /// `0x801d908c..0x801d90b8`).
    NoRaSeru,
}

/// Spell-table flag deciding which target flow a confirmed spell opens:
/// stats byte `+2` bit `0x20` set routes to the no-pick **group** flow
/// (retail sub-screen `0x10`), clear to the per-member **target picker**
/// (sub-screen `0x11`).
///
/// PORT: FUN_801d9110 (the state-2 confirm dispatch,
/// `0x801d9220..0x801d9260`: `lbu 0x2(spell_stats); andi 0x20`)
///
/// NOT WIRED: the session has no group flow to route into.
/// [`SpellMenuOutcome::Cast`] names exactly one `target_slot` and
/// [`crate::spells::cast_spell`] resolves a group heal one ally at a time
/// (its `HealAll` arm asks the caller to re-run it per ally), so a
/// confirmed spell always enters the per-member picker. Wiring the retail
/// split (sub-screen `0x10` no-pick vs `0x11` picker) needs a multi-target
/// outcome shape the field host can apply, which is a change to the
/// outcome contract rather than to this predicate. The catalog already
/// carries the flag as [`crate::spells::SpellTarget`] - see
/// `crate::retail_magic` for the `0x02` ally / `0x20` all bit decode.
pub fn spell_targets_group(stats_flag_byte: u8) -> bool {
    stats_flag_byte & 0x20 != 0
}

/// Returns true if the spell's effect kind is meaningful in the field.
/// Damage / Capture / Escape / Buff are battle-only.
pub fn is_field_usable(eff: &SpellEffect) -> bool {
    matches!(
        eff,
        SpellEffect::Heal { .. }
            | SpellEffect::HealAll { .. }
            | SpellEffect::Cure(_)
            | SpellEffect::CureAll
            | SpellEffect::Revive { .. }
    )
}

/// What the renderer needs per row of the spell list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellRowView {
    pub spell_id: u8,
    pub name: String,
    pub mp_cost: u8,
    pub admissible: bool,
}

#[derive(Debug, Clone)]
pub struct SpellMenuSession {
    party: Vec<CasterSlot>,
    targets: Vec<TargetRow>,
    catalog: SpellCatalog,
    phase: SpellMenuPhase,
}

impl SpellMenuSession {
    pub fn new(party: Vec<CasterSlot>, targets: Vec<TargetRow>, catalog: SpellCatalog) -> Self {
        Self {
            party,
            targets,
            catalog,
            phase: SpellMenuPhase::CharSelect { cursor: 0 },
        }
    }

    pub fn party(&self) -> &[CasterSlot] {
        &self.party
    }

    pub fn targets(&self) -> &[TargetRow] {
        &self.targets
    }

    pub fn catalog(&self) -> &SpellCatalog {
        &self.catalog
    }

    pub fn phase(&self) -> &SpellMenuPhase {
        &self.phase
    }

    pub fn outcome(&self) -> Option<&SpellMenuOutcome> {
        match &self.phase {
            SpellMenuPhase::Done(o) => Some(o),
            _ => None,
        }
    }

    pub fn is_done(&self) -> bool {
        matches!(self.phase, SpellMenuPhase::Done(_))
    }

    /// Build the spell-row views for the currently-active caster.
    pub fn current_spell_rows(&self) -> Vec<SpellRowView> {
        let caster_idx = match self.phase {
            SpellMenuPhase::SpellSelect { caster, .. } => caster as usize,
            SpellMenuPhase::TargetSelect { caster, .. } => caster as usize,
            _ => return Vec::new(),
        };
        let Some(c) = self.party.get(caster_idx) else {
            return Vec::new();
        };
        c.spells
            .iter()
            .map(|id| {
                let def = self.catalog.get(*id);
                let name = def
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| format!("Spell {id}"));
                let cost = def.map(|d| d.mp_cost).unwrap_or(0);
                let admissible = match def {
                    Some(d) => is_field_usable(&d.effect) && c.mp >= d.mp_cost as u16,
                    None => false,
                };
                SpellRowView {
                    spell_id: *id,
                    name,
                    mp_cost: cost,
                    admissible,
                }
            })
            .collect()
    }

    pub fn cursor(&self) -> u8 {
        match self.phase {
            SpellMenuPhase::CharSelect { cursor } => cursor,
            SpellMenuPhase::SpellSelect { cursor, .. } => cursor,
            SpellMenuPhase::TargetSelect { cursor, .. } => cursor,
            SpellMenuPhase::Done(_) => 0,
        }
    }

    fn step(cursor: u8, dir: i8, n: usize) -> u8 {
        if n == 0 {
            return 0;
        }
        ((cursor as i8 + dir).rem_euclid(n as i8)) as u8
    }

    pub fn tick(&mut self, input: SpellMenuInput) -> Vec<SpellMenuEvent> {
        let mut events = Vec::new();
        match self.phase.clone() {
            SpellMenuPhase::CharSelect { cursor } => {
                if input.circle || input.start {
                    self.phase = SpellMenuPhase::Done(SpellMenuOutcome::Cancelled);
                    events.push(SpellMenuEvent::Cancelled);
                    return events;
                }
                let n = self.party.len();
                let mut new_cursor = cursor;
                if input.up || input.left {
                    new_cursor = Self::step(cursor, -1, n);
                } else if input.down || input.right {
                    new_cursor = Self::step(cursor, 1, n);
                }
                if new_cursor != cursor {
                    self.phase = SpellMenuPhase::CharSelect { cursor: new_cursor };
                    events.push(SpellMenuEvent::CursorMoved { cursor: new_cursor });
                }
                if input.cross {
                    let Some(c) = self.party.get(new_cursor as usize) else {
                        return events;
                    };
                    if !c.alive() {
                        events.push(SpellMenuEvent::InvalidConfirm {
                            reason: InvalidReason::DeadCaster,
                        });
                        return events;
                    }
                    if c.spells.is_empty() {
                        events.push(SpellMenuEvent::InvalidConfirm {
                            reason: InvalidReason::EmptySpellList,
                        });
                        return events;
                    }
                    // Retail's second gate: an empty Ra-Seru slot buzzes
                    // (FUN_801d8f10 at 0x801d908c..0x801d90b8).
                    if c.ra_seru_missing {
                        events.push(SpellMenuEvent::InvalidConfirm {
                            reason: InvalidReason::NoRaSeru,
                        });
                        return events;
                    }
                    self.phase = SpellMenuPhase::SpellSelect {
                        caster: new_cursor,
                        cursor: 0,
                    };
                    events.push(SpellMenuEvent::EnteredSpellSelect { caster: new_cursor });
                }
            }
            SpellMenuPhase::SpellSelect { caster, cursor } => {
                if input.circle {
                    self.phase = SpellMenuPhase::CharSelect { cursor: caster };
                    events.push(SpellMenuEvent::Backed);
                    return events;
                }
                let rows = self.current_spell_rows();
                let n = rows.len();
                let mut new_cursor = cursor;
                if input.up {
                    new_cursor = Self::step(cursor, -1, n);
                } else if input.down {
                    new_cursor = Self::step(cursor, 1, n);
                } else if n > SPELL_LIST_PAGE_ROWS {
                    // Left/Right flip list pages (12 rows per page - the
                    // retail list-page layout; clamped at the ends).
                    if input.left {
                        new_cursor = cursor.saturating_sub(SPELL_LIST_PAGE_ROWS as u8);
                    } else if input.right {
                        new_cursor = (cursor as usize + SPELL_LIST_PAGE_ROWS).min(n - 1) as u8;
                    }
                } else if input.left {
                    new_cursor = Self::step(cursor, -1, n);
                } else if input.right {
                    new_cursor = Self::step(cursor, 1, n);
                }
                if new_cursor != cursor {
                    self.phase = SpellMenuPhase::SpellSelect {
                        caster,
                        cursor: new_cursor,
                    };
                    events.push(SpellMenuEvent::CursorMoved { cursor: new_cursor });
                }
                if input.cross {
                    let Some(row) = rows.get(new_cursor as usize) else {
                        events.push(SpellMenuEvent::InvalidConfirm {
                            reason: InvalidReason::EmptySpellList,
                        });
                        return events;
                    };
                    let Some(def) = self.catalog.get(row.spell_id) else {
                        events.push(SpellMenuEvent::InvalidConfirm {
                            reason: InvalidReason::UnknownSpell,
                        });
                        return events;
                    };
                    if !is_field_usable(&def.effect) {
                        events.push(SpellMenuEvent::InvalidConfirm {
                            reason: InvalidReason::NotFieldUsable,
                        });
                        return events;
                    }
                    let caster_mp = self.party.get(caster as usize).map(|c| c.mp).unwrap_or(0);
                    if caster_mp < def.mp_cost as u16 {
                        events.push(SpellMenuEvent::InvalidConfirm {
                            reason: InvalidReason::NotEnoughMp,
                        });
                        return events;
                    }
                    self.phase = SpellMenuPhase::TargetSelect {
                        caster,
                        spell_id: row.spell_id,
                        cursor: 0,
                    };
                    events.push(SpellMenuEvent::EnteredTargetSelect {
                        caster,
                        spell_id: row.spell_id,
                    });
                }
            }
            SpellMenuPhase::TargetSelect {
                caster,
                spell_id,
                cursor,
            } => {
                if input.circle {
                    self.phase = SpellMenuPhase::SpellSelect { caster, cursor: 0 };
                    events.push(SpellMenuEvent::Backed);
                    return events;
                }
                let n = self.targets.len();
                let mut new_cursor = cursor;
                if input.up || input.left {
                    new_cursor = Self::step(cursor, -1, n);
                } else if input.down || input.right {
                    new_cursor = Self::step(cursor, 1, n);
                }
                if new_cursor != cursor {
                    self.phase = SpellMenuPhase::TargetSelect {
                        caster,
                        spell_id,
                        cursor: new_cursor,
                    };
                    events.push(SpellMenuEvent::CursorMoved { cursor: new_cursor });
                }
                if input.cross {
                    let Some(target) = self.targets.get(new_cursor as usize) else {
                        events.push(SpellMenuEvent::InvalidConfirm {
                            reason: InvalidReason::InvalidTarget,
                        });
                        return events;
                    };
                    let Some(def) = self.catalog.get(spell_id) else {
                        events.push(SpellMenuEvent::InvalidConfirm {
                            reason: InvalidReason::UnknownSpell,
                        });
                        return events;
                    };
                    let needs_dead = matches!(def.effect, SpellEffect::Revive { .. });
                    if needs_dead && target.alive() {
                        events.push(SpellMenuEvent::InvalidConfirm {
                            reason: InvalidReason::InvalidTarget,
                        });
                        return events;
                    }
                    if !needs_dead && !target.alive() {
                        events.push(SpellMenuEvent::InvalidConfirm {
                            reason: InvalidReason::DeadTarget,
                        });
                        return events;
                    }
                    let target_slot = target.slot;
                    let Some(c) = self.party.get(caster as usize) else {
                        return events;
                    };
                    let snap = crate::spells::SpellSnapshot {
                        caster_mp: c.mp,
                        target_hp: target.hp,
                        target_hp_max: target.hp_max,
                        target_alive: target.alive(),
                        ..Default::default()
                    };
                    let outcome = crate::spells::cast_spell(def, target_slot, &snap);
                    let resolved = SpellMenuOutcome::Cast {
                        caster_slot: c.slot,
                        spell_id,
                        target_slot,
                        outcome,
                    };
                    self.phase = SpellMenuPhase::Done(resolved);
                    events.push(SpellMenuEvent::Cast {
                        caster_slot: c.slot,
                        spell_id,
                        target_slot,
                    });
                }
            }
            SpellMenuPhase::Done(_) => {}
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spells::SpellEffect;

    fn party() -> Vec<CasterSlot> {
        vec![
            CasterSlot {
                slot: 0,
                name: "Vahn".into(),
                hp: 60,
                mp: 30,
                spells: vec![],
                ..Default::default()
            },
            CasterSlot {
                slot: 1,
                name: "Noa".into(),
                hp: 50,
                mp: 30,
                spells: vec![0x10, 0x40], // Heal (field-usable), Reseal (battle-only Capture)
                ..Default::default()
            },
        ]
    }

    fn targets() -> Vec<TargetRow> {
        vec![
            TargetRow {
                slot: 0,
                name: "Vahn".into(),
                hp: 30,
                hp_max: 60,
            },
            TargetRow {
                slot: 1,
                name: "Noa".into(),
                hp: 50,
                hp_max: 50,
            },
        ]
    }

    #[test]
    fn dead_caster_invalid_confirm() {
        let mut p = party();
        p[0].hp = 0;
        let mut s = SpellMenuSession::new(p, targets(), SpellCatalog::vanilla());
        let evs = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        assert!(matches!(
            s.phase(),
            SpellMenuPhase::CharSelect { cursor: 0 }
        ));
        assert!(evs.contains(&SpellMenuEvent::InvalidConfirm {
            reason: InvalidReason::DeadCaster,
        }));
    }

    #[test]
    fn empty_spell_list_invalid_confirm() {
        let s_party = party();
        let mut s = SpellMenuSession::new(s_party, targets(), SpellCatalog::vanilla());
        // First caster (Vahn) has empty spell list.
        let evs = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        assert!(evs.contains(&SpellMenuEvent::InvalidConfirm {
            reason: InvalidReason::EmptySpellList,
        }));
    }

    #[test]
    fn enter_spell_select_with_alive_caster_with_spells() {
        let mut s = SpellMenuSession::new(party(), targets(), SpellCatalog::vanilla());
        // Move cursor to Noa (idx 1).
        let _ = s.tick(SpellMenuInput {
            down: true,
            ..Default::default()
        });
        let _ = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        assert!(matches!(
            s.phase(),
            SpellMenuPhase::SpellSelect {
                caster: 1,
                cursor: 0
            }
        ));
    }

    #[test]
    fn cancel_from_charselect() {
        let mut s = SpellMenuSession::new(party(), targets(), SpellCatalog::vanilla());
        let evs = s.tick(SpellMenuInput {
            circle: true,
            ..Default::default()
        });
        assert!(s.is_done());
        assert!(evs.contains(&SpellMenuEvent::Cancelled));
        assert_eq!(s.outcome(), Some(&SpellMenuOutcome::Cancelled));
    }

    #[test]
    fn back_from_spellselect_returns_to_char() {
        let mut s = SpellMenuSession::new(party(), targets(), SpellCatalog::vanilla());
        let _ = s.tick(SpellMenuInput {
            down: true,
            ..Default::default()
        });
        let _ = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        let _ = s.tick(SpellMenuInput {
            circle: true,
            ..Default::default()
        });
        assert!(matches!(
            s.phase(),
            SpellMenuPhase::CharSelect { cursor: 1 }
        ));
    }

    #[test]
    fn confirm_battle_only_spell_filters_to_invalid() {
        let mut s = SpellMenuSession::new(party(), targets(), SpellCatalog::vanilla());
        // Manually drop into SpellSelect for Noa with spell slot index 1
        // (Reseal - Capture, battle-only).
        let _ = s.tick(SpellMenuInput {
            down: true,
            ..Default::default()
        });
        let _ = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        // Move cursor to Reseal (index 1 in Noa's spell list).
        let _ = s.tick(SpellMenuInput {
            down: true,
            ..Default::default()
        });
        let evs = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        assert!(evs.iter().any(|e| matches!(
            e,
            SpellMenuEvent::InvalidConfirm {
                reason: InvalidReason::NotFieldUsable
            }
        )));
    }

    #[test]
    fn cast_heal_resolves_outcome() {
        let mut s = SpellMenuSession::new(party(), targets(), SpellCatalog::vanilla());
        // Move to Noa.
        let _ = s.tick(SpellMenuInput {
            down: true,
            ..Default::default()
        });
        let _ = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        // Confirm Heal (slot 0).
        let _ = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        assert!(matches!(s.phase(), SpellMenuPhase::TargetSelect { .. }));
        // Confirm Vahn target (slot 0).
        let _ = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        assert!(s.is_done());
        match s.outcome() {
            Some(SpellMenuOutcome::Cast {
                caster_slot,
                target_slot,
                outcome,
                ..
            }) => {
                assert_eq!(*caster_slot, 1);
                assert_eq!(*target_slot, 0);
                match outcome {
                    SpellOutcome::Heal { .. } => {}
                    other => panic!("expected Heal, got {other:?}"),
                }
            }
            other => panic!("expected Cast, got {other:?}"),
        }
    }

    #[test]
    fn pad_edge_decoder_round_trip() {
        let m = PadButton::Cross.mask() | PadButton::Down.mask();
        let i = SpellMenuInput::from_pad_edge(m);
        assert!(i.cross && i.down);
    }

    #[test]
    fn current_spell_rows_marks_admissibility() {
        let mut p = party();
        p[1].mp = 0;
        let mut s = SpellMenuSession::new(p, targets(), SpellCatalog::vanilla());
        let _ = s.tick(SpellMenuInput {
            down: true,
            ..Default::default()
        });
        let _ = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        let rows = s.current_spell_rows();
        assert!(rows.iter().all(|r| !r.admissible));
    }

    #[test]
    fn is_field_usable_filters_damage_and_buff() {
        assert!(!is_field_usable(&SpellEffect::Damage {
            base_power: 50,
            element: crate::spells::SpellElement::Fire
        }));
        assert!(is_field_usable(&SpellEffect::HealAll { amount: 60 }));
        assert!(is_field_usable(&SpellEffect::Heal { amount: 24 }));
    }

    #[test]
    fn target_dead_blocks_for_non_revive() {
        let mut t = targets();
        t[0].hp = 0;
        let mut s = SpellMenuSession::new(party(), t, SpellCatalog::vanilla());
        let _ = s.tick(SpellMenuInput {
            down: true,
            ..Default::default()
        });
        let _ = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        let _ = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        let evs = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        assert!(evs.iter().any(|e| matches!(
            e,
            SpellMenuEvent::InvalidConfirm {
                reason: InvalidReason::DeadTarget
            }
        )));
    }

    #[test]
    fn missing_ra_seru_buzzes_the_caster_confirm() {
        // FUN_801d8f10's second gate: spell list present but the Ra-Seru
        // slot empty -> refuse with a buzz, stay on the picker.
        let mut p = party();
        p[1].ra_seru_missing = true; // Noa: has spells, Ra-Seru unequipped
        let mut s = SpellMenuSession::new(p, targets(), SpellCatalog::vanilla());
        let _ = s.tick(SpellMenuInput {
            down: true,
            ..Default::default()
        });
        let evs = s.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        assert!(evs.iter().any(|e| matches!(
            e,
            SpellMenuEvent::InvalidConfirm {
                reason: InvalidReason::NoRaSeru
            }
        )));
        assert!(matches!(
            s.phase(),
            SpellMenuPhase::CharSelect { cursor: 1 }
        ));
    }

    #[test]
    fn spell_stat_flag_0x20_routes_to_the_group_flow() {
        // FUN_801d9110 state-2 dispatch: stats byte +2 bit 0x20 set ->
        // sub-screen 0x10 (group), clear -> 0x11 (target picker).
        assert!(spell_targets_group(0x20));
        assert!(spell_targets_group(0xFF));
        assert!(!spell_targets_group(0x00));
        assert!(!spell_targets_group(0xDF));
    }
}
