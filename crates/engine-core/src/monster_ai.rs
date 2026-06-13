//! Per-monster-id battle-AI script - clean-room port of the per-monster
//! `switch` in the action picker `FUN_801E9FD4`
//! (`ghidra/scripts/funcs/overlay_battle_action_801e9fd4.txt`).
//!
//! The picker has two layers. The **generic decision core** (ported inline in
//! `crate::world::World::pick_monster_action`) rolls physical-vs-cast over the
//! monster's own `+0x21` magic ids. After it, a large `switch` keyed on
//! `DAT_8007BD0C[slot]` can **override** the choice with a bespoke scripted
//! cast. `DAT_8007BD0C[slot]` is the **per-slot monster id** - `FUN_801DA51C`
//! fills it from the encounter record's `[+4 + slot]` ids - so each case is AI
//! for a specific monster (e.g. low-HP self-heal, MP-gated nukes, multi-phase
//! boss scripts), gated on HP / MP / per-monster cooldowns / battle-mode flags.
//!
//! This module ports that `switch` as the pure [`decide`] function plus the
//! post-switch **recent-target anti-repeat ring** ([`apply_recent_target_ring`])
//! and the battle-scoped state the cases read/write across turns
//! ([`MonsterAiState`]).
//!
//! ## Faithful, with documented gaps
//!
//! - Scripted casts emit retail spell ids (`0x50..=0xBA`); they fold only when
//!   the active spell catalog knows the id (the disc spell table, or the
//!   clean-room monster block added to [`crate::spells::SpellCatalog::vanilla`]).
//!   Otherwise the engine falls back to a physical strike, matching the retail
//!   shape (the picked action is simply unaffordable / unknown).
//! - `ctx+0x28a` battle-mode flags ([`MonsterAiState::mode_flags`]) gate the
//!   multi-phase boss cases (`0xA8`, `0xB4`, `0xB5`, `0xB6`, `0xA2..=0xA4`, …).
//!   The retail writer is the battle-action SM's `case 0xFF`
//!   (`_DAT_8007BD24[0x28A] += 1`); it is ported as
//!   [`crate::world::World::advance_battle_mode`], which a boss phase-transition
//!   action calls so the next turn's [`decide`] reads the bumped mode and the
//!   phase's scripted casts come alive.
//! - The `actor+0x16e & 0x380` AI flag is **not** a missing monster writer.
//!   `FUN_80047430` sets it only on **party** slots (slot `< 3`) whose character
//!   ability bitfield `+0xF8` has bit `0x2000` - accessory passive `0x2D`,
//!   the Evil Medallion's Rage - delegating that party member
//!   to the AI; the target resolver `FUN_801E7320` (`monster_setup`) is reached
//!   only when it is set. A normal monster keeps `0x380` **clear**, so its
//!   `!ai380` scripted-cast cases fire and the resolver stays dormant - which is
//!   exactly what the engine does (monster actors carry `field_flags == 0`). So
//!   the `!ai380` gates are faithful as-is; the path behind a set `0x380`
//!   (AI-driven party members) is a separate status-effect feature, not a flag
//!   the monster AI flips.
//! - The per-monster cooldown latches (`DAT_801C8FE0`) are **battle-scoped**, not
//!   per-round. Retail clears the whole latch array exactly once, at battle init
//!   (`FUN_80055b6c`, the same sweep that zeroes `flag_bd84` / `_DAT_8007BD84`), so
//!   the ability cooldowns the cases arm (`dat[pi+4]`) stay set for the rest of the
//!   fight - a boss self-heals at most **once per battle**. There is no
//!   between-rounds clear: the only other writer of `DAT_801C8FE0` is the steal /
//!   spoils handler `FUN_8004ad80`, which reuses the same scratch region for
//!   stolen-item bookkeeping (`[monster_index]` = stolen id, `[monster_index+8]` =
//!   recovered flag), unrelated to the AI cooldowns. [`MonsterAiState::reset`]
//!   mirrors the battle-init clear and is the only re-arm point.
//! - Monster `0x8A` reads its own spirit-art charge gauge (`actor+0x170`,
//!   modelled as [`MonsterAiCtx::spirit_gauge`]) as a cast gate; the override
//!   returns an [`AiCast::spirit_gauge_writeback`] the caller applies.
//! - Two retail tail blocks that run *after* the per-monster `switch` are not
//!   modelled, both because they touch host state the engine has no consumer for:
//!   (1) the `'O'` (`0x4F`) boss post-amble (`LAB_801EBDF0`) - when the band-lead
//!   monster id is `0x4F` it pokes a *second* actor's action queue (revive + disarm
//!   at battle-mode `1`, a three-cast chain `8/9/0xB` at mode `3`); it is a
//!   cross-actor write keyed on runtime-pinned actor-identity globals, with no
//!   `0x4F` encounter in the playable slice. (2) the capture-archive preload
//!   (`FUN_8003eae4(0,0x21)`) the picker fires when the chosen action is a
//!   category-2 cast of spell id `0x2E`/`0x2F` - host-side asset streaming.

// [`decide`] is a line-by-line transcription of the retail per-monster `switch`.
// Its literal shape - discrete case labels (not ranges), `rand() % k == 0` gates
// (the retail `iVar == (iVar / k) * k` idiom), and sole-`if` arm bodies - is
// kept deliberately so each arm maps to the Ghidra dump; the structural clippy
// lints that would rewrite that shape are allowed for this module.
#![allow(
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::manual_is_multiple_of,
    clippy::manual_range_patterns
)]

/// Battle-scoped monster-AI state - the globals the `FUN_801E9FD4` `switch`
/// reads and writes across turns. Reset at battle start
/// ([`MonsterAiState::reset`]).
#[derive(Debug, Clone)]
pub struct MonsterAiState {
    /// `DAT_801C8FE0[..]` - a flat i32 array the cases index two ways:
    /// `[monster_index]` (short cooldown) and `[monster_index + 4]` (ability
    /// cooldown). `DAT_801C8FE4` (the boss phase counter used by ids
    /// `0x89/0xA8/0xB5`) aliases index `1`, exactly as in retail; expose it via
    /// [`MonsterAiState::counter`].
    pub dat: [i32; 16],
    /// `DAT_8007BD84` - a global one-shot gate (boss intro cast, id `0xB4`).
    pub flag_bd84: u8,
    /// `DAT_8007BDBC[0..4]` - the recent-target ring (anti-repeat targeting).
    pub recent_targets: [u8; 4],
    /// `ctx+0x28a` - battle-mode counter. Bit 0 (and `% 3`, exact value) gate
    /// the multi-phase boss cases. Advanced by
    /// [`crate::world::World::advance_battle_mode`] (the battle-action SM's
    /// `case 0xFF`); `0` until a boss phase transition fires.
    pub mode_flags: u8,
    /// Per-slot scratch for monster `0xB3`'s `record+0x1C` queue-armed byte
    /// (`0x16`/`0x17`); modelled here rather than on the record.
    pub b3_armed: [u8; 8],
}

impl Default for MonsterAiState {
    fn default() -> Self {
        Self {
            dat: [0; 16],
            flag_bd84: 0,
            recent_targets: [0xFF; 4],
            mode_flags: 0,
            b3_armed: [0; 8],
        }
    }
}

impl MonsterAiState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset for a fresh battle (clear cooldowns / counters / ring).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// `DAT_801C8FE4` - aliases `dat[1]`.
    pub fn counter(&self) -> i32 {
        self.dat[1]
    }
    fn set_counter(&mut self, v: i32) {
        self.dat[1] = v;
    }
}

/// Inputs the AI script reads for one monster's turn. Field names cite the
/// retail offsets (`actor+0xNN`, `ctx[+0xNN]`).
#[derive(Debug, Clone, Copy)]
pub struct MonsterAiCtx {
    /// `DAT_8007BD0C[slot]` - the per-slot monster id the `switch` keys on.
    pub monster_id: u8,
    /// `param_1` - 0-based index within the monster band.
    pub monster_index: u8,
    /// Absolute actor-table slot of the caster (for self-target casts).
    pub caster_slot: u8,
    /// `actor+0x14C` current HP.
    pub hp: u16,
    /// `actor+0x14E` max HP.
    pub max_hp: u16,
    /// `actor+0x150` current MP.
    pub mp: u16,
    /// `ctx[+0]` party count.
    pub party_count: u8,
    /// `ctx[+1]` monster count.
    pub monster_count: u8,
    /// `actor+0x16E` field flags at entry (`local_40`). Bit set `0x380` =
    /// "this turn is delegated to the AI target resolver" (set by `FUN_80047430`
    /// only on party members whose ability bitfield carries the Evil
    /// Medallion's Rage passive). A normal monster keeps it clear,
    /// so its `!ai380` scripted-cast cases fire - `0` in the current engine.
    pub field_flags: u16,
    /// Count of living party members with non-zero MP (for monster `0xA7`).
    pub allies_with_mp: u8,
    /// `actor+0x170` - the per-actor spirit-art charge gauge (0..=100), filled
    /// on the defender of every damaging hit by the shared finisher
    /// (`crate::world::World::accrue_spirit_gauge`, the port of
    /// `FUN_801DDB30`'s spirit stage). Monster `0x8A` reads it as a charge gate:
    /// once it passes `0x31` the monster fires its big all-enemies cast and the
    /// gauge is clamped back to `0x32` (see the `0x8a` case + the returned
    /// [`AiCast::spirit_gauge_writeback`]).
    pub spirit_gauge: u16,
}

/// One AI-script override: the monster casts `spell_id` (action category `2` =
/// magic, or `3` = physical chain) at `target_class` - a `+0x1DD` targeting
/// code the resolver / cast path expands (`< party_count` = that party slot,
/// `>= party_count` = that monster slot / self, `8` = all enemies, `9` = all
/// allies).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiCast {
    pub spell_id: u8,
    pub target_class: u8,
    pub category: u8,
    /// When `Some(v)`, the caller writes `v` back to the caster's spirit-art
    /// gauge (`actor+0x170`) as part of committing this action - retail's
    /// `*(ushort *)(actor + 0x170) = 0x32` clamp in the monster `0x8A` case
    /// (`FUN_801E9FD4`). `None` for every other override (they don't touch the
    /// gauge). Draws no RNG, so it never perturbs the determinism stream.
    pub spirit_gauge_writeback: Option<u16>,
}

impl AiCast {
    fn magic(spell_id: u8, target_class: u8) -> Self {
        Self {
            spell_id,
            target_class,
            category: 2,
            spirit_gauge_writeback: None,
        }
    }
}

/// `LAB_801EB6E0` + `LAB_801EB6E4`: cast `spell` at a random party slot
/// (`rand % party_count`).
fn cast_party(spell: u8, party_count: u8, rng: &mut dyn FnMut() -> u32) -> AiCast {
    let pc = party_count.max(1) as u32;
    AiCast::magic(spell, (rng() % pc) as u8)
}

/// `LAB_801EB9A8` / `LAB_801EB1B0`: cast `spell` at self (the caster slot).
fn cast_self(spell: u8, caster_slot: u8) -> AiCast {
    AiCast::magic(spell, caster_slot)
}

/// `LAB_801EBD20` + `LAB_801EBD24`: cast `spell` at all enemies (class `8`).
fn cast_all_enemies(spell: u8) -> AiCast {
    AiCast::magic(spell, 8)
}

/// Decide a monster's scripted-cast override for this turn, or `None` to keep
/// the generic-core choice. Pure port of the `FUN_801E9FD4` per-monster-id
/// `switch`; `rng` is the deterministic battle RNG (each draw mirrors a retail
/// `rand()` call, in the same order).
pub fn decide(
    ctx: &MonsterAiCtx,
    state: &mut MonsterAiState,
    rng: &mut dyn FnMut() -> u32,
) -> Option<AiCast> {
    let id = ctx.monster_id;
    let pi = ctx.monster_index as usize;
    let ai380 = ctx.field_flags & 0x380 != 0;
    let mode_flags = state.mode_flags;
    let mode1 = mode_flags & 1 != 0;
    let pc = ctx.party_count.max(1);
    let half = ctx.max_hp >> 1;
    let third = ctx.max_hp / 3;
    let quarter = ctx.max_hp >> 2;
    let self_slot = ctx.caster_slot;

    match id {
        // Low-tier nukers: occasional single-target cast off a short cooldown.
        0x04 | 0x43 | 0x44 | 0x45 | 0x92 => {
            if state.dat[pi] == 0 && rng().is_multiple_of(5) {
                return Some(cast_party(0x51, pc, rng));
            }
        }
        0x05 => {
            if state.dat[pi] == 0 && rng() & 3 == 0 {
                return Some(cast_party(0x51, pc, rng));
            }
        }
        0x06 => {
            if !ai380 && ctx.hp <= half && state.dat[pi + 4] == 0 {
                state.dat[pi + 4] += 1;
                return Some(cast_self(0x52, self_slot));
            }
            if state.dat[pi] == 0 && rng().is_multiple_of(3) {
                return Some(cast_party(0x51, pc, rng));
            }
        }
        0x07 => {
            if ctx.monster_count != 4 && rng().is_multiple_of(6) && !ai380 {
                return Some(cast_self(0x50, self_slot));
            }
        }
        0x08 => {
            if ctx.monster_count != 4 && rng().is_multiple_of(5) && !ai380 {
                return Some(cast_self(0x50, self_slot));
            }
        }
        0x09 => {
            if ctx.monster_count != 4 && rng() & 3 == 0 && !ai380 {
                return Some(cast_self(0x50, self_slot));
            }
            if rng() & 3 == 0 && ctx.mp > 0x18 && !ai380 {
                return Some(cast_party(0x6f, pc, rng));
            }
        }
        // Single-target nuke vs the picked party member, MP- and accuracy-gated.
        0x0E => {
            let target = (rng() % pc as u32) as u8;
            // The retail extra gates (target not behind 0x1000, rand%100, MP)
            // collapse to: cast 0x40 when affordable and the roll lands.
            if !ai380 && rng() % 100 <= 0xc && ctx.mp >= 99 {
                return Some(AiCast::magic(0x40, target));
            }
        }
        0x0F => {
            let _target = (rng() % pc as u32) as u8;
            if !ai380 && rng() % 100 < 0xd && ctx.mp > 0x81 {
                return Some(cast_all_enemies(0x53));
            }
        }
        // Multi-hit physical chain when wounded (queue of "7" entries).
        0x25 | 0x26 | 0x27 => {
            if rng().is_multiple_of(3) && ctx.hp <= half {
                // Physical chain: category 3, target a random party member.
                return Some(AiCast {
                    spell_id: 0,
                    target_class: (rng() % pc as u32) as u8,
                    category: 3,
                    spirit_gauge_writeback: None,
                });
            }
        }
        // Status-shot vs a status-eligible party member (denominator varies by
        // id; collapses to an MP-gated cast).
        0x28 | 0x29 | 0x2a | 0x7f | 0x85 => {
            let denom = if id == 0x28 { 5 } else { 3 };
            let target = (rng() % pc as u32) as u8;
            if rng() % denom == 0 && !ai380 && ctx.mp > 0x27 {
                return Some(AiCast::magic(0x70, target));
            }
        }
        0x39 => {
            if !ai380 && rng() & 1 == 0 && ctx.mp > 0xbd {
                return Some(cast_all_enemies(0x71));
            }
        }
        // Self-heal when badly wounded, on an ability cooldown.
        0x47 | 0x48 | 0x68 | 0x69 | 0x6a => {
            if !ai380 && ctx.hp <= third && state.dat[pi + 4] == 0 {
                state.dat[pi + 4] += 1;
                return Some(cast_self(0x52, self_slot));
            }
        }
        0x4b => {
            if state.dat[pi + 4] != 0 {
                return Some(cast_all_enemies(0x56));
            }
            if rng() % 10 < 4 && ctx.mp > 99 {
                return Some(cast_self(0x55, self_slot));
            }
        }
        0x4d | 0xad | 0xae => {
            if mode1 && rng() & 1 == 0 && ctx.mp > 99 && ctx.hp <= half {
                return Some(cast_all_enemies(0xb9));
            }
        }
        0x54 | 0x55 | 0x6f | 0x70 | 0x93 | 0x94 | 0x95 => {
            if !ai380 && ctx.hp <= quarter && state.dat[pi + 4] == 0 && ctx.mp > 9 {
                state.dat[pi + 4] += 1;
                return Some(cast_self(0x60, self_slot));
            }
        }
        0x59 | 0x5a | 0x5b => {
            if !ai380 {
                if ctx.hp <= quarter && state.dat[pi + 4] == 0 {
                    state.dat[pi + 4] += 1;
                    return Some(cast_self(0x60, self_slot));
                }
                if ctx.mp > 0x54 && rng().is_multiple_of(5) {
                    return Some(cast_party(0x73, pc, rng));
                }
            }
        }
        0x62 | 0x63 | 0x64 => {
            if !ai380 {
                let denom = (0x66u32).saturating_sub(id as u32).max(1);
                if rng() % denom == 0 && ctx.mp > 0x59 {
                    return Some(cast_party(0x5a, pc, rng));
                }
            }
        }
        0x6b | 0x6c | 0x6d => {
            if rng().is_multiple_of(6) && !ai380 && ctx.mp > 0x22 {
                return Some(cast_self(0x72, self_slot));
            }
        }
        0x89 => {
            if state.counter() == 0 && ctx.mp > 99 && ctx.hp <= third {
                state.set_counter(state.counter() + 1);
                return Some(cast_self(0xba, self_slot));
            }
        }
        0x8a => {
            // Charge gate: once the monster's own spirit-art gauge passes 0x31
            // it fires its big all-enemies cast (0x4E) and the gauge is clamped
            // back to 0x32. The gauge fills as the monster takes damage
            // (`accrue_spirit_gauge`), so this arms after it has been worn down.
            if ctx.spirit_gauge > 0x31 {
                let mut cast = cast_all_enemies(0x4e);
                cast.spirit_gauge_writeback = Some(0x32);
                return Some(cast);
            }
        }
        0x8b => {
            if state.dat[pi + 4] != 0 {
                return Some(cast_all_enemies(0x5d));
            }
            if rng().is_multiple_of(3) {
                return Some(cast_self(0x5e, self_slot));
            }
        }
        0x97 | 0x98 => {
            // Arms its ability cooldown each turn (no scripted cast for the
            // non-AI-flagged path); the generic core's choice stands.
            state.dat[pi + 4] = 1;
        }
        0x99 | 0x9a | 0x9b => {
            if !ai380 {
                let denom = (0x9du32).saturating_sub(id as u32).max(1);
                if rng() % denom == 0 && ctx.mp > 199 {
                    return Some(cast_party(0x75, pc, rng));
                }
            }
        }
        0x9c | 0x9d | 0x9e => {
            if !ai380 {
                let denom = (id as u32).saturating_sub(0x99).max(1);
                if rng() % denom != 0 && ctx.mp > 199 {
                    return Some(cast_party(0x76, pc, rng));
                }
            }
        }
        0x9f | 0xa0 | 0xa1 => {
            if !ai380 {
                let denom = (0xa3u32).saturating_sub(id as u32).max(1);
                if rng() % denom == 0 && ctx.mp > 199 {
                    return Some(cast_all_enemies(0x77));
                }
            }
        }
        0xa2 | 0xa3 | 0xa4 => {
            if mode_flags % 3 == 2 {
                return Some(AiCast::magic(id.wrapping_sub(0x29), self_slot));
            }
        }
        0xa6 => {
            if mode1 && !rng().is_multiple_of(3) && ctx.mp > 199 {
                return Some(cast_all_enemies(0xa6));
            }
        }
        0xa7 => {
            if ctx.allies_with_mp != 0 && ctx.mp > 0xfe {
                return Some(cast_all_enemies(0xb5));
            }
        }
        0xa8 => {
            if !mode1 {
                return Some(cast_self(0xaf, self_slot));
            }
            // Boss multi-phase: cast (counter - 0x50) at all enemies.
            return Some(AiCast::magic((state.counter() as u8).wrapping_sub(0x50), 8));
        }
        0xa9 | 0xaa => {
            let nuke = !mode1 || rng() & 3 == 0 || ctx.mp < 200;
            if nuke {
                if id == 0xaa && ctx.monster_count != 3 && rng().is_multiple_of(3) && ctx.mp != 0 {
                    return Some(cast_self(0xae, self_slot));
                }
            } else {
                return Some(AiCast::magic(id.wrapping_add(1), 8));
            }
        }
        // 0xB3: needs actor SP / record+0x1C interplay - the queue-armed half is
        // modelled via `b3_armed`; the SP scan is not.
        0xb3 => {
            let i = ctx.caster_slot as usize % state.b3_armed.len();
            if state.b3_armed[i] == 0x17 {
                return Some(cast_all_enemies(0xb3));
            }
            if rng() & 1 == 0 && ctx.mp > 0xfe && ctx.hp <= half {
                state.b3_armed[i] = 0x17;
            } else {
                state.b3_armed[i] = 0x16;
            }
        }
        0xb4 => {
            if mode_flags != 0 {
                if state.flag_bd84 == 0 && mode1 && rng().is_multiple_of(3) && ctx.mp > 0xfe {
                    return Some(cast_all_enemies(0xad));
                }
                if !rng().is_multiple_of(3) || ctx.mp < 100 {
                    return None;
                }
                return Some(cast_all_enemies(0xb7));
            }
            return Some(cast_self(0xac, self_slot));
        }
        0xb5 => {
            if state.counter() == 0 {
                if rng() & 3 == 0 && ctx.mp > 199 {
                    state.set_counter(state.counter() + 1);
                    return Some(cast_self(0xa5, self_slot));
                }
                if mode1 && rng() & 1 == 0 && ctx.mp > 0xfe && ctx.hp <= half {
                    return Some(cast_all_enemies(0xb6));
                }
            } else {
                state.set_counter(state.counter() - 1);
                return Some(AiCast::magic(0xb4, 8));
            }
        }
        0xb6 => {
            return Some(match mode_flags {
                0 => AiCast::magic(0xa2, self_slot),
                1 => AiCast::magic(0xa3, self_slot),
                2 => AiCast::magic(0xa4, self_slot),
                3 => AiCast::magic(0xa5, 3),
                _ => cast_all_enemies(0xa1),
            });
        }
        _ => {}
    }
    None
}

/// Post-switch recent-target anti-repeat ring (`DAT_8007BDBC[0..4]`). When the
/// chosen target is a party slot (`< party_count`), the party has more than one
/// member, and the spell is not the special id `0x70`, a target that appears in
/// the last-4 ring is re-rolled to a different living-ish slot (the retail logic
/// nudges off recently-hit members); then the ring is shifted and the final
/// target recorded at the head. Returns the (possibly adjusted) target.
pub fn apply_recent_target_ring(
    target_class: u8,
    spell_id: u8,
    party_count: u8,
    state: &mut MonsterAiState,
    rng: &mut dyn FnMut() -> u32,
) -> u8 {
    let pc = party_count.max(1);
    let mut target = target_class;
    if target_class < 3 && pc > 1 && spell_id != 0x70 {
        // Find the target in the recent ring.
        let idx = state.recent_targets.iter().position(|&t| t == target_class);
        if let Some(idx) = idx {
            let span = (4 - idx) as u32;
            if span != 0 && rng() % span != 0 {
                // Nudge to an adjacent slot (retail: +/-1 then wrap), or re-roll.
                let bump = if rng() & 1 == 0 {
                    target.wrapping_sub(1)
                } else {
                    target.wrapping_add(1)
                };
                target = bump % pc;
            }
        }
    }
    // Shift the ring and record the chosen target at the head.
    state.recent_targets[3] = state.recent_targets[2];
    state.recent_targets[2] = state.recent_targets[1];
    state.recent_targets[1] = state.recent_targets[0];
    state.recent_targets[0] = target;
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(id: u8, hp: u16, max_hp: u16, mp: u16) -> MonsterAiCtx {
        MonsterAiCtx {
            monster_id: id,
            monster_index: 0,
            caster_slot: 3,
            hp,
            max_hp,
            mp,
            party_count: 3,
            monster_count: 1,
            field_flags: 0,
            allies_with_mp: 0,
            spirit_gauge: 0,
        }
    }

    /// As [`ctx`] but with the spirit-art gauge pre-set (for monster `0x8A`).
    fn ctx_gauge(id: u8, gauge: u16) -> MonsterAiCtx {
        MonsterAiCtx {
            spirit_gauge: gauge,
            ..ctx(id, 100, 100, 100)
        }
    }

    #[test]
    fn low_hp_monster_self_heals_off_cooldown() {
        // Monster 0x47 self-heals (spell 0x52) when HP <= maxHP/3 and the
        // ability cooldown is clear. Targets self (caster slot 3).
        let mut state = MonsterAiState::new();
        let mut rng = || 0u32;
        let c = ctx(0x47, 20, 100, 50); // 20 <= 33
        let cast = decide(&c, &mut state, &mut rng).expect("self-heal");
        assert_eq!(cast.spell_id, 0x52);
        assert_eq!(cast.category, 2);
        assert_eq!(cast.target_class, 3, "targets self");
        assert_eq!(state.dat[4], 1, "ability cooldown armed");

        // Second turn: cooldown is set, so no scripted heal this turn.
        let again = decide(&c, &mut state, &mut rng);
        assert!(again.is_none(), "cooldown gates the re-heal");
    }

    #[test]
    fn ability_cooldown_is_battle_scoped_and_rearms_only_on_reset() {
        // The ability cooldown a scripted case arms (`dat[pi+4]`) is BATTLE-scoped,
        // not per-round: retail clears the latch array only at battle init
        // (`FUN_80055b6c`), so a wounded monster self-heals once and is gated for
        // the rest of the fight. There is no between-rounds clear - the next battle
        // (a fresh `reset`, which mirrors that battle-init sweep) is what re-arms it.
        let mut state = MonsterAiState::new();
        let mut rng = || 0u32;
        let c = ctx(0x47, 20, 100, 50); // 20 <= maxHP/3 -> heal eligible

        // Turn 1: heals, arms the ability cooldown.
        assert_eq!(decide(&c, &mut state, &mut rng).unwrap().spell_id, 0x52);
        assert_eq!(state.dat[4], 1, "ability cooldown armed");

        // Many later turns in the SAME battle: the latch persists, no re-heal.
        for _ in 0..8 {
            assert!(decide(&c, &mut state, &mut rng).is_none());
        }
        assert_eq!(state.dat[4], 1, "no per-round clear within the battle");

        // A fresh battle (`reset` = the battle-init sweep) re-arms it.
        state.reset();
        assert_eq!(state.dat[4], 0, "reset clears the latch");
        assert_eq!(
            decide(&c, &mut state, &mut rng).unwrap().spell_id,
            0x52,
            "heals again next battle"
        );
    }

    #[test]
    fn healthy_monster_does_not_force_a_heal() {
        let mut state = MonsterAiState::new();
        let mut rng = || 0u32;
        let c = ctx(0x47, 100, 100, 50); // full HP
        assert!(decide(&c, &mut state, &mut rng).is_none());
    }

    #[test]
    fn boss_phase_counter_cycles() {
        // Monster 0xB5: first eligible turn arms the counter + casts 0xA5 at
        // self; while the counter is non-zero it casts 0xB4 at all enemies and
        // decrements.
        let mut state = MonsterAiState::new();
        let mut rng = || 0u32; // 0 & 3 == 0 -> first branch fires
        let c = ctx(0xb5, 200, 200, 250);
        let first = decide(&c, &mut state, &mut rng).expect("arm");
        assert_eq!(first.spell_id, 0xa5);
        assert_eq!(state.counter(), 1);
        let second = decide(&c, &mut state, &mut rng).expect("phase");
        assert_eq!(second.spell_id, 0xb4);
        assert_eq!(second.target_class, 8, "all enemies");
        assert_eq!(state.counter(), 0, "counter decremented");
    }

    #[test]
    fn battle_mode_selects_boss_phase_spell() {
        // Monster 0xB6 is a pure boss-phase case: it always casts, but which
        // spell depends on the battle-mode counter (`ctx+0x28a`). Advancing the
        // mode (the SM's `case 0xFF`) walks it through its phase spells.
        let c = ctx(0xb6, 200, 200, 250);
        let phase = |mode: u8| {
            let mut s = MonsterAiState::new();
            s.mode_flags = mode;
            decide(&c, &mut s, &mut || 0u32)
                .expect("0xB6 always casts")
                .spell_id
        };
        assert_eq!(phase(0), 0xa2, "mode 0 -> phase I");
        assert_eq!(phase(1), 0xa3, "mode 1 -> phase II");
        assert_eq!(phase(2), 0xa4, "mode 2 -> phase III");
        assert_eq!(phase(3), 0xa5, "mode 3 -> smite a party slot");
        assert_eq!(phase(4), 0xa1, "mode 4+ -> all-enemy nova");
    }

    #[test]
    fn boss_case_dormant_until_mode_advances() {
        // Monster 0xA8 keeps to its plain self-buff while the mode is even
        // (`!mode1`); once a phase transition flips bit 0 it switches to the
        // counter-driven all-enemy cast.
        let mut state = MonsterAiState::new();
        let mut rng = || 0u32;
        let c = ctx(0xa8, 200, 200, 250);
        assert_eq!(
            decide(&c, &mut state, &mut rng).unwrap().spell_id,
            0xaf,
            "mode 0 -> self buff"
        );
        state.mode_flags = 1;
        let phased = decide(&c, &mut state, &mut rng).unwrap();
        assert_eq!(phased.target_class, 8, "phased -> all enemies");
    }

    #[test]
    fn recent_target_ring_records_targets() {
        let mut state = MonsterAiState::new();
        let mut rng = || 1u32;
        let t = apply_recent_target_ring(0, 0x51, 3, &mut state, &mut rng);
        assert!(t < 3);
        assert_eq!(state.recent_targets[0], t, "head records the target");
    }

    #[test]
    fn all_enemy_target_class_is_not_ring_filtered() {
        // target_class 8 (all enemies) is >= 3, so the ring leaves it intact.
        let mut state = MonsterAiState::new();
        let mut rng = || 0u32;
        let t = apply_recent_target_ring(8, 0x53, 3, &mut state, &mut rng);
        assert_eq!(t, 8);
    }

    #[test]
    fn monster_8a_fires_all_enemies_cast_once_charged() {
        // 0x8A reads its own spirit-art gauge: below the 0x31 threshold it keeps
        // the generic-core choice; at/above it fires the 0x4E all-enemies cast
        // and asks the caller to clamp the gauge to 0x32. Draws no RNG.
        let mut state = MonsterAiState::new();
        let mut rng = || 0u32;

        // Gauge 0x31 (49) is NOT strictly greater than 0x31 -> no override.
        let below = ctx_gauge(0x8a, 0x31);
        assert!(decide(&below, &mut state, &mut rng).is_none());

        // Gauge 0x32 (50) trips the gate.
        let at = ctx_gauge(0x8a, 0x32);
        let cast = decide(&at, &mut state, &mut rng).expect("charged cast");
        assert_eq!(cast.spell_id, 0x4e);
        assert_eq!(cast.category, 2);
        assert_eq!(cast.target_class, 8, "all enemies");
        assert_eq!(cast.spirit_gauge_writeback, Some(0x32));

        // A full gauge still clamps back to 0x32, not zero.
        let full = ctx_gauge(0x8a, 100);
        let cast = decide(&full, &mut state, &mut rng).expect("charged cast");
        assert_eq!(cast.spirit_gauge_writeback, Some(0x32));
    }
}
