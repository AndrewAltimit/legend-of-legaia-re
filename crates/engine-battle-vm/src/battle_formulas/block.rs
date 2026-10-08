//! The melee **block roll** - whether a defender blocks one physical hit.
//!
//! PORT: FUN_801EC3E4 (`0x801EC5A8..0x801EC878`, the roll; the verdict is
//! `sltu s0,s1` at `0x801EC874`)
//!
//! Read off the disassembly (`see
//! ghidra/scripts/funcs/overlay_battle_action_801ec3e4.txt`). The roll runs
//! only when the defender has a block entry (`+0x1F3 != 0`, `0x801EC5BC`) and
//! still stands on the accumulated total (`target[+0x0] < +0x14C`,
//! `0x801EC5CC..0x801EC5DC`) - both gates sit before the two `rand()` draws,
//! so a defender with no block clip consumes no randomness here.
//!
//! ```text
//! s0 = A.SPD + A.ATK*4/5 + ctx[+0x6D2]          ; attacker (+0x164, +0x158)
//!                                               ; 0x6D2 in 0..=0x800
//! s1 = D.SPD + D.ATK*4/5 + ctx[+0x6D4]          ; defender
//! s0 = max(s0, s1)
//! s0 += (rand() % s0) * BLOCK_SCALARS[(pb - 0x0C) % 5] >> 1
//! s1 += rand() % s1
//! D chose Spirit (+0x1DE == 4)         -> s1 = s1 * 3 / 2
//! A committed art slot 0x11            -> s0 = s0 * 3 / 2
//! A status +0x16E & 0x1000             -> s0 = s0 * 8 / 10
//! D status +0x16E & 0x1000             -> s1 = s1 * 8 / 10
//! A party, ability +0xF4 & 0x80000     -> s0 <<= 1
//! A party, ability +0xF4 & 0x200000    -> s0 = s1
//! D party, ability +0xF4 & 0x100000    -> s1 = s1 * 3 / 2
//!   else ability +0xF4 & 0x200000      -> s0 = s1
//! D status +0x16E & 0x400              -> s0 = s1
//! blocked = s0 < s1
//! ```
//!
//! All arithmetic is unsigned 32-bit (`divu`, `multu`, `srl`). `ATK*4/5` is the
//! `0x66666667` reciprocal (`sra 1` of the high word), `*8/10` the
//! `0xCCCCCCCD` one (`srl 3`). The ATK read is the **unfolded** `+0x158`: the
//! per-command equipment fold runs later in the same routine (`0x801ECB84`).

/// `0x801F64E4[(pb - 0x0C) % 5]` in PROT 0898 (file `0x27CCC`) - the
/// attacker's per-command block-roll scalar, eight bytes below the damage
/// scalars at `0x801F64EC`.
pub const BLOCK_SCALARS: [u8; 5] = [6, 4, 4, 4, 2];

/// Ability bit (`+0xF4`) that doubles the attacker's roll.
pub const ABILITY_BLOCK_ATTACK_X2: u32 = 0x8_0000;
/// Ability bit (`+0xF4`) that triples-halves the defender's roll.
pub const ABILITY_BLOCK_GUARD_X15: u32 = 0x10_0000;
/// Ability bit (`+0xF4`) that pins the attacker's roll to the defender's.
pub const ABILITY_BLOCK_EVEN: u32 = 0x20_0000;
/// Ability bit (`+0xF4`) on a party attacker that cancels a defender's block
/// after the verdict (`0x801ECB44..0x801ECB5C`).
pub const ABILITY_BLOCK_BREAK: u32 = 0x4000;
/// Status bit (`+0x16E`) that scales a roll by 8/10.
pub const STATUS_BLOCK_DAMPEN: u16 = 0x1000;
/// Status bit (`+0x16E`) on the defender that disables blocking.
pub const STATUS_GUARD_DISABLED: u16 = 0x400;

/// One side of the roll.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlockSide {
    /// `+0x164`, SPD working.
    pub spd: u16,
    /// `+0x158`, ATK working (unfolded).
    pub atk: u16,
    /// `+0x16E` status word.
    pub status: u16,
    /// The character record's `+0xF4` ability word; `None` for a monster
    /// (the party tests are `sltiu 3` on the slot).
    pub ability: Option<u32>,
}

/// Inputs of [`block_roll`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlockRoll {
    pub attacker: BlockSide,
    pub defender: BlockSide,
    /// `ctx[+0x6D2]` - the attack-angle term, `0..=0x800` (distance from
    /// face-on).
    pub attack_ramp: i16,
    /// `ctx[+0x6D4]` - the approach-distance term.
    pub guard_ramp: i16,
    /// The hit's power byte (`0x0C..=0x1F`).
    pub power_byte: u8,
    /// The defender picked Spirit this round (`+0x1DE == 4`).
    pub defender_spirit: bool,
    /// The attacker's committed clip is the dynamic art slot `0x11`.
    pub attacker_art_slot: bool,
}

/// The roll's verdict: `true` when the defender blocks. Draws `rand()` twice,
/// in retail's order. A zero sum (which would trap retail's `divu`) blocks
/// nothing and draws nothing.
pub fn block_roll(r: &BlockRoll, rand: &mut impl FnMut() -> u32) -> bool {
    let side = |s: &BlockSide, ramp: i16| -> u32 {
        u32::from(s.spd)
            .wrapping_add(u32::from(s.atk) * 4 / 5)
            .wrapping_add(i32::from(ramp) as u32)
    };
    let mut s0 = side(&r.attacker, r.attack_ramp);
    let mut s1 = side(&r.defender, r.guard_ramp);
    if s0 < s1 {
        s0 = s1;
    }
    if s0 == 0 || s1 == 0 {
        return false;
    }
    let idx = usize::from(r.power_byte.wrapping_sub(0x0C)) % BLOCK_SCALARS.len();
    let a = rand() % s0;
    s0 = s0.wrapping_add(a.wrapping_mul(u32::from(BLOCK_SCALARS[idx])) >> 1);
    s1 = s1.wrapping_add(rand() % s1);
    let x15 = |v: u32| v.wrapping_mul(3) >> 1;
    let x08 = |v: u32| v.wrapping_mul(8) / 10;
    if r.defender_spirit {
        s1 = x15(s1);
    }
    if r.attacker_art_slot {
        s0 = x15(s0);
    }
    if r.attacker.status & STATUS_BLOCK_DAMPEN != 0 {
        s0 = x08(s0);
    }
    if r.defender.status & STATUS_BLOCK_DAMPEN != 0 {
        s1 = x08(s1);
    }
    if let Some(ab) = r.attacker.ability {
        if ab & ABILITY_BLOCK_ATTACK_X2 != 0 {
            s0 = s0.wrapping_shl(1);
        }
        if ab & ABILITY_BLOCK_EVEN != 0 {
            s0 = s1;
        }
    }
    if let Some(ab) = r.defender.ability {
        if ab & ABILITY_BLOCK_GUARD_X15 != 0 {
            s1 = x15(s1);
        } else if ab & ABILITY_BLOCK_EVEN != 0 {
            s0 = s1;
        }
    }
    if r.defender.status & STATUS_GUARD_DISABLED != 0 {
        s0 = s1;
    }
    s0 < s1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(seed: u32) -> impl FnMut() -> u32 {
        let mut s = seed;
        move || {
            s = s.wrapping_mul(0x41C6_4E6D).wrapping_add(12345);
            (s >> 16) & 0x7FFF
        }
    }

    fn even(spd: u16, atk: u16) -> BlockRoll {
        BlockRoll {
            attacker: BlockSide {
                spd,
                atk,
                ..Default::default()
            },
            defender: BlockSide {
                spd,
                atk,
                ..Default::default()
            },
            power_byte: 0x0C,
            ..Default::default()
        }
    }

    /// Property: the attacker's term starts at `max(s0, s1)`, so a defender
    /// can only win on its own `rand() % s1` beating the attacker's - an even
    /// match blocks sometimes, never always.
    #[test]
    fn an_even_match_blocks_sometimes_but_not_always() {
        let mut rng = lcg(7);
        let n = 4000;
        let blocks = (0..n)
            .filter(|_| block_roll(&even(60, 80), &mut rng))
            .count();
        assert!(blocks > 0 && blocks < n, "{blocks} / {n}");
    }

    /// Property: the guard-disabled status pins the attacker to the defender
    /// after every bonus, and `s0 < s0` never holds.
    #[test]
    fn a_guard_disabled_defender_never_blocks() {
        for seed in 0..200 {
            let mut r = even(40 + seed as u16, 90);
            r.defender.status = STATUS_GUARD_DISABLED;
            r.defender_spirit = true;
            assert!(!block_roll(&r, &mut lcg(seed)));
        }
    }

    /// Property: a much faster, stronger attacker cannot be blocked - its
    /// floor is already the larger sum, and the defender's draw adds less
    /// than its own sum.
    #[test]
    fn a_dominant_attacker_is_never_blocked() {
        for seed in 0..500 {
            let mut r = even(10, 10);
            r.attacker.spd = 400;
            r.attacker.atk = 400;
            assert!(!block_roll(&r, &mut lcg(seed)));
        }
    }

    /// Property: Spirit on the defender never lowers the block count over a
    /// shared random stream (it only grows `s1`).
    #[test]
    fn spirit_never_lowers_the_block_rate() {
        let base = even(50, 60);
        let mut spirit = base;
        spirit.defender_spirit = true;
        let (mut a, mut b) = (0, 0);
        for seed in 0..2000 {
            a += block_roll(&base, &mut lcg(seed)) as u32;
            b += block_roll(&spirit, &mut lcg(seed)) as u32;
            assert!(
                !block_roll(&base, &mut lcg(seed)) || block_roll(&spirit, &mut lcg(seed)),
                "seed {seed}: Spirit un-blocked a hit"
            );
        }
        assert!(b > a, "{a} -> {b}");
    }

    /// Property: an off-axis opening strike is rarely blocked. The angle
    /// term is `<= 0` and added unsigned (`addu`, compared `sltu`): once it
    /// outweighs the attacker's sum, the sum wraps past every defender sum,
    /// and only a later wrap of the attacker's own random add can bring it
    /// back under. Face-on (`0`) the even match blocks routinely.
    #[test]
    fn an_off_axis_opening_strike_is_rarely_blocked() {
        let count = |ramp: i16| {
            (0..2000u32)
                .filter(|&seed| {
                    let mut r = even(20, 20);
                    r.attack_ramp = ramp;
                    block_roll(&r, &mut lcg(seed))
                })
                .count()
        };
        let face_on = count(0);
        let off_axis = count(-0x400);
        assert!(face_on > 100, "face-on blocks: {face_on}");
        assert!(off_axis * 10 < face_on, "{off_axis} vs {face_on}");
    }

    /// Exactly two draws, and none on a zero sum.
    #[test]
    fn the_roll_draws_twice() {
        let mut n = 0;
        block_roll(&even(5, 5), &mut || {
            n += 1;
            3
        });
        assert_eq!(n, 2);
        let mut m = 0;
        block_roll(&even(0, 0), &mut || {
            m += 1;
            3
        });
        assert_eq!(m, 0);
    }

    /// The table is the 0898 bytes at `0x801F64E4`.
    #[test]
    fn the_scalar_index_wraps_by_five() {
        let mut r = even(50, 50);
        r.power_byte = 0x11; // (0x11 - 0x0C) % 5 = 0 -> 6, same as 0x0C
        let mut a = even(50, 50);
        a.power_byte = 0x0C;
        for seed in 0..100 {
            assert_eq!(
                block_roll(&r, &mut lcg(seed)),
                block_roll(&a, &mut lcg(seed))
            );
        }
    }
}
