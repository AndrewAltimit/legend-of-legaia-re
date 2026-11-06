//! The generic decision core of the monster AI picker `FUN_801E9FD4`: the
//! physical-or-magic gate, the magic pick's tag-`0x23` walk and the
//! target-class arms, from `0x801EA058` to the loop exit at `0x801EA974`
//! (`ghidra/scripts/funcs/overlay_0898_801e9fd4.txt`).
//!
//! Retail seat numbering throughout: party seats `0..3`, monster seats from
//! `3` (`3 + index`), `8` / `9` the party / monster row codes. The caller
//! re-keys them onto its own seating.
//!
//! What runs after the core - the flee checkpoint `FUN_801EC0DC` and the
//! per-monster-id `switch` - is [`super::decide`] and the caller's.
//!
//! ## One pass
//!
//! 1. **The gate** (`0x801EA058..0x801EA1A0`). `s1 = 1 +` the count of
//!    **non-zero** magic slots `+0x21..=+0x23`, forced to `1` for monster
//!    `0x89` and to `2` for `0x88` on an odd round (`ctx[+0x28A] & 1`).
//!    `r = rand() % s1`; a non-zero `r` whose slot `+0x20 + r` is below `2`
//!    falls back to `0`, and monster `0xB4` always takes `r = 1`. `r == 0` is
//!    the physical arm.
//! 2. **The physical arm** (`0x801EA1A8..`): the target is `rand() % ctx[+0]`,
//!    the seated party count, with no liveness test (the turn picker's
//!    redirect `FUN_801DB124` re-rolls a fallen one). The swing-budget loop
//!    that follows is the caller's
//!    ([`crate::monster_catalog::MonsterDef::action_costs`]).
//! 3. **The magic arm** (`0x801EA3D0..`): `s1 =` the count of slots `>= 2`,
//!    `k = rand() % s1` - the pass's **second** draw - and `s1` forced to `1`
//!    for `0x88` on an odd round. The walk then steps every archive entry
//!    from `2` while the index is below `+0x4A`. A tag-`0x23` entry with
//!    `k != 0` counts `k` and `s1` down (`0x801EA940`); with `k == 0` it
//!    **matches**: the spell is the slot byte `+0x21 + s1 - 1`, an empty slot
//!    or one the caster's MP `+0x150` cannot pay (`spell +3`) fails the pass
//!    (`0x801EA8B4`), and otherwise the spell id, the clip (the entry index,
//!    into `+0x1E0` and `+0x1E7`) and the target class arm's `+0x1DD` are
//!    stored.
//! 4. **A match does not end the walk**: the stores fall through to the loop
//!    bottom (`0x801EA948`), so every later tag-`0x23` entry matches again,
//!    with everything that implies - the affordability test on the slot it
//!    reads, the stores, and the target-class arm, whose draws are taken
//!    again. The heal (`0x00`) and buff (`0x20`) arms count their monster
//!    loop in `s1`, which they leave at the seated monster count `ctx[+1]`,
//!    so the next match reads slot `+0x21 + ctx[+1] - 1` rather than the
//!    `k`-th one.
//! 5. **A failed pass** (`s8 -= 1; s2 = 0xFFFE`) leaves the walk and the
//!    loop exit sends the picker back to the gate (`blez` at `0x801EA9B8`):
//!    the whole pass is redrawn, gate roll first.
//!
//! The target-class arms key on the spell record's `+2 & 0x60`:
//!
//! | `& 0x60` | `+0` | `+0x1DD` |
//! |---|---|---|
//! | `0x40` | - | `rand() % ctx[+0]`; `0` when party seat `1` holds character `4` (`0x801EA8CC..0x801EA930`) |
//! | `0x60` | - | `8` |
//! | `0x00` | `< 2` | the weakest monster below half HP, each candidate past the HP tests on a `rand() & 3 == 0`; none fails the pass. Monsters `0x53..=0x55` re-point the clip at the entry before (`0x801EA5E4..0x801EA6FC`) |
//! | `0x00` | `0x63` | the caster |
//! | `0x00` | other | the caster, then the healthiest living monster on the same per-candidate draw (`0x801EA700..0x801EA7E4`) |
//! | `0x20` | `< 2` | `9` when any living monster below half HP passes its `rand() & 3 == 0`; none fails the pass (`0x801EA7E8..0x801EA8B0`) |
//! | `0x20` | other | `9` |

/// The spell-table bytes the core reads (`DAT_800754C8 + id * 0xC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellBytes {
    /// `+0` - the cast class (`< 2` / `0x63` select a target arm).
    pub class: u8,
    /// `+2` - the target-shape byte; the core reads `& 0x60`.
    pub target: u8,
    /// `+3` - the MP cost.
    pub mp: u8,
}

/// One monster seat the target-class arms walk, by monster index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MonsterSeat {
    /// Live HP `+0x14C`.
    pub hp: u16,
    /// Max HP `+0x14E`.
    pub max_hp: u16,
    /// The seat's monster id (`DAT_8007BD0C[index]`).
    pub monster_id: u8,
}

/// What the core reads.
#[derive(Debug, Clone, Copy)]
pub struct CoreInput<'a> {
    /// The caster's monster index (`s7`; its seat is `3 + index`).
    pub caster_index: u8,
    /// The caster's monster id (`DAT_8007BD0C[s7]`).
    pub monster_id: u8,
    /// The round counter `ctx[+0x28A]`.
    pub round: u8,
    /// The record bytes `+0x20..+0x28`.
    pub ai_bytes: [u8; 8],
    /// Each archive entry's tag (entry `+0`), `+0x4A` long.
    pub entry_tags: &'a [u8],
    /// The caster's live MP `+0x150`.
    pub mp: u16,
    /// `ctx[+0]`, the seated party count.
    pub party_count: u8,
    /// The seated monster seats, `ctx[+1]` long.
    pub monsters: &'a [MonsterSeat],
    /// `DAT_8007BD10[1] == 4`: party seat `1` holds character `4`.
    pub companion_in_seat_1: bool,
}

/// The core's pick, in retail numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePick {
    /// The physical arm: `+0x1DE = 3`, target `+0x1DD`.
    Physical { target: u8 },
    /// A magic match: `+0x1DE = 2`, spell `+0x1DF`, clip `+0x1E0` and the
    /// Spirit band's clip `+0x1E7`, target `+0x1DD` - as the **last** match
    /// of the walk left them.
    Magic {
        spell: u8,
        clip: u8,
        spirit_clip: u8,
        target: u8,
    },
    /// The magic walk reached no tag-`0x23` entry with `k` spent and matched
    /// nothing, and no failed pass stored a cast: retail leaves the actor's
    /// command bytes as its last action left them.
    Unmatched,
}

/// A pass's outcome.
enum Pass {
    Done(CorePick),
    Failed(Option<CorePick>),
}

/// Passes the port runs before it gives up on a pick that keeps failing.
/// Retail's retry is unbounded; a gate that can roll the physical arm ends
/// it with probability one, and only a forced magic arm (monster `0xB4`)
/// whose every match fails would spin.
pub const MAX_PASSES: usize = 64;

/// A [`CorePick::Magic`] target that means "the actor's `+0x1DD` as it stands":
/// the pick of a pass that failed before its target store.
pub const KEEP_TARGET: u8 = 0xFE;

/// The tag the cast walk counts.
const CAST_TAG: u8 = 0x23;

/// Run the core: passes until one does not fail. `spell` resolves a spell
/// id to its table bytes (`None` past the table); `rand` is the battle's
/// `rand()`. Returns the pick and the number of passes run.
pub fn pick(
    input: &CoreInput<'_>,
    spell: &dyn Fn(u8) -> Option<SpellBytes>,
    rand: &mut dyn FnMut() -> u32,
) -> (CorePick, usize) {
    // A pass that matched a cast and then failed on its target has already stored
    // the spell, clip and category: they stand if the retry stores nothing over
    // them (`0x801EA528..0x801EA570` ahead of the `LAB_801EA8B8` failures).
    let mut stored = None;
    for n in 1..=MAX_PASSES {
        match one_pass(input, spell, rand) {
            Pass::Done(CorePick::Unmatched) => {
                return (stored.unwrap_or(CorePick::Unmatched), n);
            }
            Pass::Done(p) => return (p, n),
            Pass::Failed(Some(s)) => stored = Some(s),
            Pass::Failed(None) => {}
        }
    }
    (stored.unwrap_or(CorePick::Unmatched), MAX_PASSES)
}

fn odd_round(input: &CoreInput<'_>) -> bool {
    input.round & 1 != 0
}

fn one_pass(
    input: &CoreInput<'_>,
    spell: &dyn Fn(u8) -> Option<SpellBytes>,
    rand: &mut dyn FnMut() -> u32,
) -> Pass {
    let slots = &input.ai_bytes[1..4];
    // The gate.
    let mut s1 = 1 + slots.iter().filter(|&&b| b != 0).count() as u32;
    if input.monster_id == 0x89 && odd_round(input) {
        s1 = 1;
    }
    if input.monster_id == 0x88 && odd_round(input) {
        s1 = 2;
    }
    let mut r = rand() % s1;
    if r != 0 && input.ai_bytes[r as usize] < 2 {
        r = 0;
    }
    if input.monster_id == 0xB4 {
        r = 1;
    }
    let pc = u32::from(input.party_count.max(1));
    if r == 0 {
        return Pass::Done(CorePick::Physical {
            target: (rand() % pc) as u8,
        });
    }
    // The magic arm.
    let count = slots.iter().filter(|&&b| b >= 2).count() as u32;
    if count == 0 {
        // Retail divides by zero here (`break 0x1c00`); only the forced
        // `0xB4` gate can reach it, and that record carries magic.
        return Pass::Done(CorePick::Unmatched);
    }
    let mut k = (rand() % count) as u16;
    let mut s1 = count as u16;
    if input.monster_id == 0x88 && odd_round(input) {
        s1 = 1;
    }
    let mc = input.monsters.len() as u16;
    let mut picked = None;
    let mut e = 2usize;
    while e < input.entry_tags.len() {
        if input.entry_tags[e] != CAST_TAG {
            e += 1;
            continue;
        }
        if k != 0 {
            k -= 1;
            s1 = s1.wrapping_sub(1);
            e += 1;
            continue;
        }
        // `+0x21 + s1 - 1`, the window `+0x20..+0x28`; a counter that ran
        // off it reads as an empty slot.
        let at = 1 + i32::from(s1) - 1;
        let id = usize::try_from(at)
            .ok()
            .and_then(|i| input.ai_bytes.get(i))
            .copied()
            .unwrap_or(0);
        // A failure here comes ahead of this entry's stores (the MP test at
        // `0x801EA51C` sits before `sb v0,0x1de(s4)` at `0x801EA528`), but an
        // earlier match in the same walk has already stored its spell,
        // clip and category, and they stand into the retry.
        let Some(bytes) = (id != 0).then(|| spell(id)).flatten() else {
            return Pass::Failed(picked);
        };
        if input.mp < u16::from(bytes.mp) {
            return Pass::Failed(picked);
        }
        let stored = CorePick::Magic {
            spell: id,
            clip: e as u8,
            spirit_clip: e as u8,
            target: KEEP_TARGET,
        };
        let mut clip = e as u8;
        let target = match bytes.target & 0x60 {
            0x40 => {
                let t = (rand() % pc) as u8;
                if input.companion_in_seat_1 { 0 } else { t }
            }
            0x60 => 8,
            0x00 if bytes.class < 2 => {
                let mut best = 0xFFFFu16;
                let mut target = None;
                for (i, m) in input.monsters.iter().enumerate() {
                    if m.hp != 0 && m.hp < best && m.hp < m.max_hp >> 1 && rand() & 3 == 0 {
                        best = m.hp;
                        target = Some(3 + i as u8);
                        if (0x53..=0x55).contains(&m.monster_id) {
                            clip = (e as u8).wrapping_sub(1);
                        }
                    }
                }
                s1 = mc;
                match target {
                    Some(t) => t,
                    None => return Pass::Failed(Some(stored)),
                }
            }
            0x00 => {
                let mut target = 3 + input.caster_index;
                if bytes.class != 0x63 {
                    let mut best = 0u16;
                    for (i, m) in input.monsters.iter().enumerate() {
                        if m.hp != 0 && best < m.hp && rand() & 3 == 0 {
                            best = m.hp;
                            target = 3 + i as u8;
                        }
                    }
                    s1 = mc;
                }
                target
            }
            _ if bytes.class < 2 => {
                let mut weak = 0u16;
                for m in input.monsters {
                    if m.hp != 0 && m.hp < m.max_hp >> 1 && rand() & 3 == 0 {
                        weak += 1;
                    }
                }
                s1 = mc;
                if weak == 0 {
                    return Pass::Failed(Some(stored));
                }
                9
            }
            _ => 9,
        };
        picked = Some(CorePick::Magic {
            spell: id,
            clip,
            spirit_clip: clip,
            target,
        });
        e += 1;
    }
    Pass::Done(picked.unwrap_or(CorePick::Unmatched))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(id: u8) -> Option<SpellBytes> {
        Some(match id {
            // A single-target nuke.
            0x30 => SpellBytes {
                class: 0x14,
                target: 0x44,
                mp: 10,
            },
            // A row nuke.
            0x31 => SpellBytes {
                class: 0x14,
                target: 0x64,
                mp: 10,
            },
            // A heal.
            0x32 => SpellBytes {
                class: 0x01,
                target: 0x02,
                mp: 4,
            },
            _ => return None,
        })
    }

    fn input<'a>(ai: [u8; 8], tags: &'a [u8], monsters: &'a [MonsterSeat]) -> CoreInput<'a> {
        CoreInput {
            caster_index: 0,
            monster_id: 0x10,
            round: 0,
            ai_bytes: ai,
            entry_tags: tags,
            mp: 100,
            party_count: 3,
            monsters,
            companion_in_seat_1: false,
        }
    }

    fn draws(seq: &[u32]) -> impl FnMut() -> u32 + '_ {
        let mut i = 0;
        move || {
            let v = seq[i % seq.len()];
            i += 1;
            v
        }
    }

    #[test]
    fn the_magic_pick_draws_the_gate_then_k() {
        // Two live slots; gate 1 (magic), k 0, then the single target draw.
        let tags = [0, 0, 0x23, 0x0C, 0x23];
        let mon = [MonsterSeat {
            hp: 50,
            max_hp: 50,
            monster_id: 0x10,
        }];
        let i = input([0, 0x30, 0x31, 0, 0, 0, 0, 0], &tags, &mon);
        let mut taken = 0;
        let seq = [1u32, 2, 7, 5];
        let mut r = || {
            let v = seq[taken];
            taken += 1;
            v
        };
        let (p, passes) = pick(&i, &table, &mut r);
        // k = 2 % 2 = 0: the first tag-0x23 entry reads slot `s1 - 1 = 1`
        // (0x31, the row nuke), and the second re-matches it.
        assert_eq!(passes, 1);
        assert_eq!(
            p,
            CorePick::Magic {
                spell: 0x31,
                clip: 4,
                spirit_clip: 4,
                target: 8
            }
        );
        assert_eq!(taken, 2, "gate and k only: the row arm draws nothing");
    }

    #[test]
    fn a_single_target_re_match_redraws_its_target() {
        let tags = [0, 0, 0x23, 0x23];
        let mon = [MonsterSeat::default()];
        let i = input([0, 0x30, 0, 0, 0, 0, 0, 0], &tags, &mon);
        let mut seq = vec![1u32, 0, 1, 2];
        seq.reverse();
        let mut r = || seq.pop().unwrap();
        let (p, _) = pick(&i, &table, &mut r);
        // Gate 1 % 2, k 0 % 1, then one target draw per match: 1, then 2.
        assert_eq!(
            p,
            CorePick::Magic {
                spell: 0x30,
                clip: 3,
                spirit_clip: 3,
                target: 2
            }
        );
        assert!(seq.is_empty());
    }

    #[test]
    fn the_heal_arm_leaves_s1_at_the_monster_count() {
        // Slots [heal, nuke]; k = 1 walks past entry 2, matches entry 3 on
        // slot 0 (the heal), whose arm leaves s1 = 2 monsters: the re-match
        // on entry 4 reads slot 1, the nuke.
        let tags = [0, 0, 0x23, 0x23, 0x23];
        let mon = [
            MonsterSeat {
                hp: 10,
                max_hp: 100,
                monster_id: 0x10,
            },
            MonsterSeat {
                hp: 90,
                max_hp: 100,
                monster_id: 0x10,
            },
        ];
        let i = input([0, 0x32, 0x30, 0, 0, 0, 0, 0], &tags, &mon);
        // gate 1, k 1, heal candidate draw 0 (passes), nuke target 1.
        let mut seq = vec![1u32, 1, 0, 1];
        seq.reverse();
        let mut r = || seq.pop().unwrap();
        let (p, _) = pick(&i, &table, &mut r);
        assert_eq!(
            p,
            CorePick::Magic {
                spell: 0x30,
                clip: 4,
                spirit_clip: 4,
                target: 1
            }
        );
        assert!(seq.is_empty());
    }

    #[test]
    fn a_heal_with_no_patient_redraws_the_pass() {
        let tags = [0, 0, 0x23];
        let mon = [MonsterSeat {
            hp: 90,
            max_hp: 100,
            monster_id: 0x10,
        }];
        let i = input([0, 0x32, 0, 0, 0, 0, 0, 0], &tags, &mon);
        // Pass 1: gate 1, k 0, the heal finds nobody -> retry. Pass 2:
        // gate 0 -> physical, target 2.
        let mut seq = vec![1u32, 0, 0, 2];
        seq.reverse();
        let mut r = || seq.pop().unwrap();
        let (p, passes) = pick(&i, &table, &mut r);
        assert_eq!(passes, 2);
        assert_eq!(p, CorePick::Physical { target: 2 });
        assert!(seq.is_empty());
    }

    #[test]
    fn a_marker_slot_counts_in_the_gate_but_falls_back_to_physical() {
        // Slot value 1 is non-zero (gate denominator 2) but `< 2`.
        let tags = [0, 0, 0x23];
        let i = input([0, 1, 0, 0, 0, 0, 0, 0], &tags, &[]);
        let mut r = draws(&[1, 1]);
        assert_eq!(pick(&i, &table, &mut r).0, CorePick::Physical { target: 1 });
    }
}
