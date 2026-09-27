//! The staged-anim commit's **clip-tag ladder** and the reaction chain it
//! drives.
//!
//! Every path through the commit `FUN_8004AD80` converges at `0x8004BDD8`:
//! the new entry is installed at node `+0x4C` and `+0x1D9 = +0x1DA`, then the
//! routine tests the **new** entry's tag byte `+0x00` once
//! (`0x8004BE30..0x8004BF4C`, `see ghidra/scripts/funcs/8004ad80.txt`):
//!
//! | tag | condition | writes |
//! |---|---|---|
//! | `2` | first monster `0xB3` / `0xB5`, committing actor at HP `0` | the entry's tag becomes `4` in place (then the tag-`4` row runs); for `0xB3` also entry `+0x56 += 1` |
//! | `4` | HP not `0` | `+0x1DA = +0x1F2` (the get-up), `+0x1DC = 0` |
//! | `4` | HP `0`, party seat | `+0x1DA = 7`, `+0x1DC = 0` |
//! | `5` | - | `+0x1DC \|= 4` |
//! | `7` | - | `+0x1DA = 8` |
//! | `8` | - | `+0x1DC \|= 8` |
//!
//! `+0x1DA` is the next entry staged behind the playing one: the anim tick's
//! natural-end path (`FUN_80047430` `0x80047B30..0x80047B58`) replaces it with
//! `0` when `+0x1DC` bit `0x4` is set, masks `+0x1DC &= 0xF8` (bit `0x8`
//! survives) and calls the commit again. So a downed party member's chain is
//! knockdown, entry `7`, entry `8`, and entry `8` then re-commits itself at
//! every natural end with the root-motion latch (`+0x1DC` bit `0x8`,
//! `0x80047D20`) raised. Three catalogued battle states read a dead party
//! member at current = queued = `8`, `+0x1DC = 8`.
//!
//! The monster-death arm earlier in the commit (`0x8004B094..0x8004B6A0`)
//! fires when the **previous** entry carried tag `4`, the committing actor is
//! a monster seat and its HP is `0`: it re-installs that entry held on its
//! last frame, runs the death spoils (`battle_steal`), sets `+0x21C = 2` and
//! `+0x1DC |= 8`, and - with a Seru staged in `ctx[+0x269]` - overwrites
//! `+0x1DC = 4` and stages the get-up `+0x1F2` (`0x8004B688..0x8004B6A0`), so
//! the fallen monster rises for the absorb. Captures read both arms: a dead
//! monster held on its knockdown entry with `+0x1DC = 8`, and one on its
//! get-up entry with `+0x1DC = 4` while `ctx[+0x269] = 1`.
//!
//! ## One channel, one timing seam
//!
//! The engine plays hit reactions on its own channel
//! ([`crate::world::Actor::battle_reaction`] - the tag of the reaction-family
//! clip playing - plus [`crate::world::Actor::battle_reaction_next`], the
//! mirror of `+0x1DA` behind it) and keeps `+0x1DC` in
//! `battle.flag_bits`. The ladder runs at the reaction's commit
//! ([`World::commit_battle_reaction_entry`]) for every row except the tag-`4`
//! HP test: the port stages the reaction inside the hit, ahead of the combo
//! total's HP write (`World::apply_combo_total`), where retail's commit runs
//! on the next anim tick with that write already landed. So the knockdown's
//! row is evaluated at the clip's natural end on the live HP - the same value
//! retail's commit saw, as long as nothing revives the actor mid-fall.

use super::*;

use vm::battle_action::ActorFlags;

/// First monster ids whose flinch commit at HP `0` is rewritten into a
/// knockdown (`li a1,0xb3` / `li v0,0xb5` at `0x8004BE44` / `0x8004BE54`).
pub const TAG2_REWRITE_MONSTERS: [u8; 2] = [0xB3, 0xB5];

/// The party's downed-chain entries: the tag-`4` commit at HP `0` stages
/// entry `7` (`li v0,0x7` at `0x8004BED8`), whose own commit stages entry `8`
/// (`li v0,0x8` at `0x8004BF20`).
pub const PARTY_DOWNED_ENTRY: u8 = 7;
/// See [`PARTY_DOWNED_ENTRY`].
pub const PARTY_DOWNED_LOOP_ENTRY: u8 = 8;

/// `+0x1DC` bit the tag-`5` commit raises: return to idle at the natural end
/// (`ori v0,v0,0x4` at `0x8004BF04`; consumed at `0x80047B30`).
pub const ANIM_FLAG_IDLE_AT_END: u8 = ActorFlags::EXIT;
/// `+0x1DC` bit the tag-`8` commit and the monster-death arm raise: the
/// root-motion latch (`0x80047D20`) - the same bit gates the effect-script
/// walk (`FUN_801DEA50`).
pub const ANIM_FLAG_ROOT_LATCH: u8 = ActorFlags::FX_SUPPRESSED;

/// What the committing actor looks like to the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LadderInputs {
    /// The new entry's tag byte `+0x00`.
    pub tag: u8,
    /// The committing actor's HP `+0x14C` is `0`.
    pub hp_zero: bool,
    /// The committing actor is a party seat (`sltiu v0,s3,0x3`).
    pub party: bool,
    /// The battle's first monster id (`gp[+0x9F4]` = `0x8007BD0C`).
    pub first_monster: u8,
    /// The actor's get-up entry `+0x1F2`.
    pub getup_entry: u8,
}

/// How the ladder leaves `+0x1DC`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimFlagWrite {
    Keep,
    /// `sb zero,0x1dc` - the tag-`4` rows.
    Clear,
    /// `ori` of the given bits.
    Or(u8),
}

/// The ladder's writes for one commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LadderWrites {
    /// The entry's tag after the in-place tag-`2` rewrite.
    pub tag: u8,
    /// `entry[+0x56] += 1` (the `0xB3` half of the rewrite).
    pub bump_entry_56: bool,
    /// The new `+0x1DA`, when the ladder stages one.
    pub next: Option<u8>,
    pub flags: AnimFlagWrite,
}

/// The ladder `0x8004BE30..0x8004BF4C`, as a pure function of the committed
/// entry's tag and the committing actor.
///
/// PORT: FUN_8004AD80 (`0x8004BE30..0x8004BF4C`, the per-commit tag ladder)
pub fn commit_tag_ladder(i: LadderInputs) -> LadderWrites {
    let mut w = LadderWrites {
        tag: i.tag,
        bump_entry_56: false,
        next: None,
        flags: AnimFlagWrite::Keep,
    };
    if w.tag == 2 && TAG2_REWRITE_MONSTERS.contains(&i.first_monster) && i.hp_zero {
        w.tag = 4;
        w.bump_entry_56 = i.first_monster == TAG2_REWRITE_MONSTERS[0];
    }
    match w.tag {
        4 if !i.hp_zero => {
            w.next = Some(i.getup_entry);
            w.flags = AnimFlagWrite::Clear;
        }
        4 if i.party => {
            w.next = Some(PARTY_DOWNED_ENTRY);
            w.flags = AnimFlagWrite::Clear;
        }
        5 => w.flags = AnimFlagWrite::Or(ANIM_FLAG_IDLE_AT_END),
        7 => w.next = Some(PARTY_DOWNED_LOOP_ENTRY),
        8 => w.flags = AnimFlagWrite::Or(ANIM_FLAG_ROOT_LATCH),
        _ => {}
    }
    w
}

/// The party reaction map `FUN_80053CB8` hardcodes into `+0x1EF..+0x1F3`
/// (the player files store the family identity-ordered).
pub const PARTY_REACTION_MAP: [u8; 5] = [2, 3, 4, 5, 0x0B];

/// The reaction-family tags, in `+0x1EF..+0x1F3` order.
const REACTION_TAGS: [u8; 5] = [2, 3, 4, 5, 0x0B];

/// A monster's `+0x1EF..+0x1F3` map from its entry tags, the way
/// `FUN_80054CB0` builds it: one forward pass with no `break`, so a repeated
/// tag resolves to its **last** entry; an absent tag stays `0`; and a zero
/// knockdown slot takes the flinch slot (`0x80055428`).
pub fn monster_reaction_map(tags: &[u8]) -> [u8; 5] {
    let mut map = [0u8; 5];
    for (entry, &t) in tags.iter().enumerate() {
        for (k, &want) in REACTION_TAGS.iter().enumerate() {
            if t == want {
                map[k] = entry as u8;
            }
        }
    }
    if map[2] == 0 {
        map[2] = map[0];
    }
    map
}

impl World {
    /// The battle's first monster id as the one byte retail's
    /// `0x8007BD0C` holds (`0` when there is none).
    fn battle_first_monster_byte(&self) -> u8 {
        self.battle_monster_slots()
            .into_iter()
            .find(|&(_, _, slot)| slot == 0)
            .map_or(0, |(_, id, _)| id as u8)
    }

    /// Actor `slot`'s `+0x1EF..+0x1F3` reaction map; `None` without clips.
    pub(in crate::world) fn battle_reaction_map(&self, slot: usize) -> Option<[u8; 5]> {
        let actor = self.actors.get(slot)?;
        let clips = actor.battle_action_clips.as_ref()?;
        if actor.battle_monster_id.is_none() {
            return Some(PARTY_REACTION_MAP);
        }
        let tags: Vec<u8> = clips
            .iter()
            .map(|c| {
                c.as_ref()
                    .map_or(legaia_asset::monster_archive::NO_ACTION_ENTRY, |c| {
                        c.action_id
                    })
            })
            .collect();
        Some(monster_reaction_map(&tags))
    }

    /// Commit action entry `entry` onto actor `slot`'s reaction channel: the
    /// engine's half of the commit `FUN_8004AD80` for an entry the damage
    /// primitive or the ladder staged. Runs the ladder on the entry's tag
    /// (the tag-`4` HP row excepted - see the module docs). Returns `false`
    /// when the entry holds no playable clip.
    pub(in crate::world) fn commit_battle_reaction_entry(
        &mut self,
        slot: usize,
        entry: u8,
    ) -> bool {
        let Some(clip) = self
            .actors
            .get(slot)
            .and_then(|a| a.battle_action_clips.as_ref())
            .and_then(|c| c.get(usize::from(entry)))
            .and_then(|c| c.clone())
        else {
            return false;
        };
        let Some(player) = crate::battle_anim::MonsterAnimPlayer::new_one_shot(&clip) else {
            return false;
        };
        let first_monster = self.battle_first_monster_byte();
        let actor = &mut self.actors[slot];
        let inputs = LadderInputs {
            tag: clip.action_id,
            hp_zero: actor.battle.hp == 0,
            party: actor.battle_monster_id.is_none(),
            first_monster,
            getup_entry: 0,
        };
        let w = commit_tag_ladder(inputs);
        actor.battle_animation = Some(player);
        actor.battle_reaction = Some(w.tag);
        actor.battle_reaction_entry = Some(entry);
        actor.battle_reaction_next = None;
        actor.battle_pose = None;
        // Tag 4's staging waits for the natural end (module docs); every
        // other row lands now.
        if w.tag != 4 {
            actor.battle_reaction_next = w.next;
            match w.flags {
                AnimFlagWrite::Keep => {}
                AnimFlagWrite::Clear => actor.battle.flag_bits = ActorFlags::empty(),
                AnimFlagWrite::Or(bits) => actor.battle.flag_bits.set(bits),
            }
        }
        // Record committed: swap in its effect script + zero the cursor
        // (retail `sb zero,0x1f5` on every commit).
        actor.battle_effect_script = Some(clip.effect_script).filter(|s| !s.is_empty());
        actor.battle_effect_cursor = 0;
        true
    }

    /// Actor `slot`'s reaction clip reached its natural end: the
    /// `FUN_80047430` natural-end path followed by the next commit's two
    /// reaction-relevant arms (the monster-death arm and the tag ladder's
    /// staged `+0x1DA`). `playing` is the tag of the finished clip.
    pub(in crate::world) fn finish_battle_reaction(&mut self, slot: usize, playing: u8) {
        let party = usize::from(self.party.party_count);
        let first_monster = self.battle_first_monster_byte();
        let seru_staged = self.battle_ctx.multi_cast_gate != 0;
        let Some(map) = self.battle_reaction_map(slot) else {
            Self::end_battle_reaction(&mut self.actors[slot]);
            return;
        };
        let a = &mut self.actors[slot];
        let hp_zero = a.battle.hp == 0;
        let is_party = a.battle_monster_id.is_none();
        // The knockdown's own ladder row, evaluated on the landed HP.
        if playing == 4 {
            let w = commit_tag_ladder(LadderInputs {
                tag: 4,
                hp_zero,
                party: is_party,
                first_monster,
                getup_entry: map[3],
            });
            a.battle_reaction_next = w.next;
            if w.flags == AnimFlagWrite::Clear {
                a.battle.flag_bits = ActorFlags::empty();
            }
        }
        // Natural end (`0x80047B30..0x80047B58`).
        let idle = a.battle.flag_bits.has(ANIM_FLAG_IDLE_AT_END);
        a.battle.flag_bits = ActorFlags(a.battle.flag_bits.0 & 0xF8);
        // The monster-death arm: previous entry tagged 4, monster seat, HP 0.
        // It runs whatever bit 2 staged - it re-installs the entry itself.
        if playing == 4 && hp_zero && !is_party {
            a.battle.flag_bits.set(ANIM_FLAG_ROOT_LATCH);
            if slot >= party {
                self.resolve_monster_death_spoils(slot);
            }
            if seru_staged {
                // `0x8004B688..0x8004B6A0`: the fallen monster rises.
                self.actors[slot].battle.flag_bits = ActorFlags(ANIM_FLAG_IDLE_AT_END);
                if !self.commit_battle_reaction_entry(slot, map[3]) {
                    self.actors[slot].battle_reaction = Some(4);
                }
            }
            // Otherwise the knockdown holds its final keyframe.
            return;
        }
        if idle {
            Self::end_battle_reaction(&mut self.actors[slot]);
            return;
        }
        match self.actors[slot].battle_reaction_next.take() {
            // `+0x1DA = 0` is the idle loop (an actor whose `+0x1F2` names
            // no get-up entry): the reaction channel lets go.
            Some(0) => Self::end_battle_reaction(&mut self.actors[slot]),
            Some(next) => {
                if !self.commit_battle_reaction_entry(slot, next) {
                    // No playable clip (Terra's empty `7` / `8` streams): the
                    // downed actor keeps the finished frame it has.
                    if !(playing == 4 && hp_zero) {
                        Self::end_battle_reaction(&mut self.actors[slot]);
                    }
                }
            }
            // Entry 8 re-commits itself: `+0x1DA` still reads 8.
            None if playing == 8 => {
                self.commit_battle_reaction_entry(slot, PARTY_DOWNED_LOOP_ENTRY);
            }
            // A dead actor whose chain has nowhere to go holds its frame.
            None if playing == 4 && hp_zero => {}
            None => Self::end_battle_reaction(&mut self.actors[slot]),
        }
    }

    /// Release the reaction channel; the idle restore resumes the loop.
    fn end_battle_reaction(a: &mut Actor) {
        a.battle_reaction = None;
        a.battle_reaction_entry = None;
        a.battle_reaction_next = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(tag: u8) -> LadderInputs {
        LadderInputs {
            tag,
            hp_zero: false,
            party: true,
            first_monster: 0x10,
            getup_entry: 5,
        }
    }

    #[test]
    fn a_living_knockdown_stages_its_getup_and_clears_the_flags() {
        let w = commit_tag_ladder(inputs(4));
        assert_eq!(w.next, Some(5));
        assert_eq!(w.flags, AnimFlagWrite::Clear);
    }

    #[test]
    fn a_downed_party_member_chains_seven_then_eight_then_latches() {
        let w = commit_tag_ladder(LadderInputs {
            hp_zero: true,
            ..inputs(4)
        });
        assert_eq!(w.next, Some(7));
        assert_eq!(commit_tag_ladder(inputs(7)).next, Some(8));
        let eight = commit_tag_ladder(inputs(8));
        assert_eq!(eight.next, None);
        assert_eq!(eight.flags, AnimFlagWrite::Or(0x08));
    }

    #[test]
    fn a_dead_monster_knockdown_stages_nothing() {
        let w = commit_tag_ladder(LadderInputs {
            hp_zero: true,
            party: false,
            ..inputs(4)
        });
        assert_eq!(w.next, None);
        assert_eq!(w.flags, AnimFlagWrite::Keep);
    }

    #[test]
    fn the_songi_flinch_at_zero_hp_becomes_a_knockdown() {
        for (first, bump) in [(0xB3, true), (0xB5, false)] {
            let w = commit_tag_ladder(LadderInputs {
                hp_zero: true,
                first_monster: first,
                ..inputs(2)
            });
            assert_eq!(w.tag, 4);
            assert_eq!(w.bump_entry_56, bump);
            assert_eq!(w.next, Some(7));
        }
        // Any other formation, or a living actor, keeps the flinch.
        assert_eq!(
            commit_tag_ladder(LadderInputs {
                hp_zero: true,
                first_monster: 0xB4,
                ..inputs(2)
            })
            .tag,
            2
        );
        assert_eq!(
            commit_tag_ladder(LadderInputs {
                first_monster: 0xB3,
                ..inputs(2)
            })
            .tag,
            2
        );
    }

    #[test]
    fn the_getup_raises_idle_at_end() {
        assert_eq!(commit_tag_ladder(inputs(5)).flags, AnimFlagWrite::Or(0x04));
    }

    #[test]
    fn the_monster_map_takes_the_last_match_and_the_flinch_fallback() {
        // Knockdown absent: slot 2 takes the flinch entry.
        assert_eq!(monster_reaction_map(&[0, 1, 2, 5]), [2, 0, 2, 3, 0]);
        // Repeated get-up: the last one wins.
        assert_eq!(monster_reaction_map(&[0, 4, 5, 2, 5]), [3, 0, 1, 4, 0]);
    }
}
