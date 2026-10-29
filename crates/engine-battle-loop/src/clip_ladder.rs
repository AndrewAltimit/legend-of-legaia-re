//! The staged-anim commit's clip-tag ladder (`FUN_8004AD80`, `0x8004BDD8..`)
//! as a kernel over its inputs: the rows, the party reaction map and the
//! re-commit cue cursor. `engine-core`'s `world::battle::clip_ladder` runs it
//! at the reaction's commit and carries the full account of the chain.

use legaia_engine_battle_vm::battle_action::ActorFlags;

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

/// The lowest first-cue id for which a monster's re-committed block entry
/// skips its first cue (`lhu v0,0x56(v0)` / `sltiu v0,v0,0x64` at
/// `0x8004B048..0x8004B050`).
pub const BLOCK_RECOMMIT_CUE_FLOOR: u16 = 0x64;

/// The cue-track cursor `+0x1F6` a commit seeds (`0x8004AFC0..0x8004B05C`).
///
/// Every commit stores `0` (`sb zero,0x1f6(s1)` in the delay slot at
/// `0x8004AFD0`) and then stores `1` (`0x8004B05C`) when the staged entry
/// `+0x1DA` is the clip already playing (`+0x1D9`, `bne` at `0x8004AFCC`)
/// **and** the juggle byte `+0x1F7` is up (`0x8004AFD4`), for these entries:
///
/// - the light-flinch slots `+0x1EF` / `+0x1F0` (`0x8004AFE4..0x8004AFFC`);
/// - the block slot `+0x1F3` (`0x8004B004`), on a party seat
///   (`sltiu v0,s3,0x3` at `0x8004B010`) or when the entry's first cue id
///   (`entry[+0x56]`, slot 0 of the `+0x54` track) is at least
///   [`BLOCK_RECOMMIT_CUE_FLOOR`].
///
/// So a hit that re-stages the reaction inside its own juggle window
/// restarts the clip without its first sound cue: the impact sound is not
/// layered on top of the one still ringing. The knockdown `+0x1F1` and
/// get-up `+0x1F2` always restart at `0`.
///
/// PORT: FUN_8004AD80 (`0x8004AFC0..0x8004B05C`, the cue-cursor seed)
pub fn recommit_cue_cursor(
    map: [u8; 5],
    current: u8,
    entry: u8,
    window_open: bool,
    party: bool,
    first_cue: u16,
) -> u8 {
    if entry != current || !window_open {
        return 0;
    }
    let skip = entry == map[0]
        || entry == map[1]
        || (entry == map[4] && (party || first_cue >= BLOCK_RECOMMIT_CUE_FLOOR));
    u8::from(skip)
}

/// The party reaction map `FUN_80053CB8` hardcodes into `+0x1EF..+0x1F3`
/// (the player files store the family identity-ordered).
pub const PARTY_REACTION_MAP: [u8; 5] = [2, 3, 4, 5, 0x0B];

/// The reaction-family tags, in `+0x1EF..+0x1F3` order.
pub const REACTION_TAGS: [u8; 5] = [2, 3, 4, 5, 0x0B];

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

    /// `0x8004AFC0..0x8004B05C`: only a same-entry re-commit inside the
    /// juggle window of a flinch, or of a block under the party / first-cue
    /// gate, skips the first cue.
    #[test]
    fn a_juggled_flinch_recommit_skips_its_first_cue() {
        let map = [2, 3, 4, 5, 0x0B];
        // Light flinches re-staged inside the window.
        assert_eq!(recommit_cue_cursor(map, 2, 2, true, false, 0), 1);
        assert_eq!(recommit_cue_cursor(map, 3, 3, true, false, 0), 1);
        // Window down, or a different clip playing: the cursor starts at 0.
        assert_eq!(recommit_cue_cursor(map, 2, 2, false, false, 0), 0);
        assert_eq!(recommit_cue_cursor(map, 0, 2, true, false, 0), 0);
        // Knockdown and get-up always restart at 0.
        assert_eq!(recommit_cue_cursor(map, 4, 4, true, false, 0x200), 0);
        assert_eq!(recommit_cue_cursor(map, 5, 5, true, true, 0x200), 0);
        // Block: every party seat, a monster only from cue 0x64 up.
        assert_eq!(recommit_cue_cursor(map, 0x0B, 0x0B, true, true, 0), 1);
        assert_eq!(recommit_cue_cursor(map, 0x0B, 0x0B, true, false, 0x63), 0);
        assert_eq!(recommit_cue_cursor(map, 0x0B, 0x0B, true, false, 0x64), 1);
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
