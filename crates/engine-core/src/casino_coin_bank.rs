//! The casino coin bank's **script delta**: field-VM op `0x4C 0xE5`.
//!
//! The bank is `_DAT_800845A4` (save block `0x80084140 + 0x464`), the same
//! word the slot machine's cash-out assigns and the coin counter's commit
//! credits. Op `0x4C 0xE5` is the only field-VM op that writes it, and the
//! scripts use it for the casino's fees and prices: the Baka Fighter
//! cabinets' one-coin charge, the Muscle Dome entry fee, the Ra-Seru egg's
//! price and the free coins a balden NPC hands out
//! (`docs/subsystems/script-vm-menuctrl.md`).
//!
//! The engine-vm dispatcher decodes the operand and calls the host hook
//! `op4c_n_e_sub_5_add_coins`; until this module the world's host left that
//! hook at its no-op default, so no script fee was ever charged.
//!
//! ## The arm, from the field overlay's bytes
//!
//! `FUN_801DE840` in PROT 0897 (base `0x801CE818`):
//!
//! ```text
//! 801e328c  jal   0x8003ceb8            ; s7 = u24 LE at operand + 1
//! 801e3290  _addiu a0,s6,0x1
//! 801e3298  lui   v0,0x80               ; bit 23 set?
//! 801e329c  and   v0,s7,v0
//! 801e32a0  beq   v0,zero,0x801e32ac
//! 801e32a8  or    s7,s7,v0              ;   sign-extend (v0 = 0xFF000000)
//! 801e32ac  lui   a0,0x98               ; a0 = 0x0098967F = 9,999,999
//! 801e32b8  lw    v0,0x464(v1)          ; v1 = 0x80084140
//! 801e32bc  ori   a0,a0,0x967f
//! 801e32c0  addu  v0,v0,s7
//! 801e32c4  sw    v0,0x464(v1)          ; bank += delta
//! 801e32c8  slt   v0,a0,v0
//! 801e32cc  beq   v0,zero,0x801e32d8
//! 801e32d4  sw    a0,0x464(v1)          ;   bank = 9,999,999 when above it
//! 801e32d8  jal   0x8003ce08            ; system flag 8 set
//! 801e32dc  _li   a0,0x8
//! 801e32e0  j     0x801df89c
//! 801e32e4  _addiu s8,s8,0x5            ; PC += 5
//! ```
//!
//! There is **no lower clamp**: the op's sibling, the gold delta at
//! `0x801E0470`, stores zero for a negative result (`bgez` at `0x801E0498`),
//! and this one does not. A debit below zero would leave a negative bank in
//! retail. The port's bank is unsigned and floors at zero instead; every
//! debit on the disc sits behind a script compare on the bank, so the floor
//! is not reached by a shipped script (inference - see the doc).

use crate::world::World;

/// The bank's ceiling, `0x0098967F` - the same bound the slot machine's
/// cash-out and the mode-24 return warp apply.
pub const COIN_BANK_CEILING: u32 = 9_999_999;

/// The system flag the op raises after every delta (`FUN_8003CE08(8)`).
pub const COIN_DELTA_FLAG: u16 = 8;

/// The bank after one op `0x4C 0xE5` delta: `bank + delta`, capped at
/// [`COIN_BANK_CEILING`]. Retail has no floor; the port's unsigned bank
/// floors at zero (see the module docs).
pub fn apply_coin_delta(bank: u32, delta: i32) -> u32 {
    let sum = i64::from(bank) + i64::from(delta);
    sum.clamp(0, i64::from(COIN_BANK_CEILING)) as u32
}

impl World {
    /// Field-VM op `0x4C 0xE5`: add a signed 24-bit delta to the casino coin
    /// bank, cap it at [`COIN_BANK_CEILING`] and raise system flag
    /// [`COIN_DELTA_FLAG`].
    // PORT: FUN_801DE840 (`0x801E328C..0x801E32E4`, op 0x4C nibble-E sub-5)
    pub fn add_script_coins(&mut self, delta: i32) {
        self.minigames.casino_coins = apply_coin_delta(self.minigames.casino_coins, delta);
        self.system_flag_set(COIN_DELTA_FLAG);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fee_debits_and_a_gift_credits() {
        assert_eq!(apply_coin_delta(150, -100), 50);
        assert_eq!(apply_coin_delta(150, 10), 160);
        assert_eq!(apply_coin_delta(0, 0), 0);
    }

    #[test]
    fn the_bank_caps_at_the_ceiling() {
        assert_eq!(apply_coin_delta(9_999_990, 100), COIN_BANK_CEILING);
        assert_eq!(apply_coin_delta(COIN_BANK_CEILING, 1), COIN_BANK_CEILING);
    }

    #[test]
    fn the_unsigned_bank_floors_at_zero() {
        assert_eq!(apply_coin_delta(1, -100_000), 0);
    }

    #[test]
    fn the_world_hook_moves_the_bank_and_raises_flag_8() {
        let mut w = World::default();
        w.minigames.casino_coins = 250;
        w.add_script_coins(-100);
        assert_eq!(w.minigames.casino_coins, 150);
        assert!(w.system_flag_test(COIN_DELTA_FLAG));
    }

    /// `4C E5 9C FF FF` is koin1 P1[9]'s dome-entry fee (three sites) and
    /// `4C E5 FF FF FF` the Baka cabinets' (P1[51..53]); run through the real
    /// field VM, the world's host must charge it. The default hook is a
    /// no-op, so a host that forgot the override leaves the bank alone.
    #[test]
    fn the_field_vm_op_charges_the_fee_through_the_world_host() {
        for (script, start, want) in [
            (vec![0x4C, 0xE5, 0x9C, 0xFF, 0xFF], 250u32, 150u32),
            (vec![0x4C, 0xE5, 0xFF, 0xFF, 0xFF], 7, 6),
            (vec![0x4C, 0xE5, 0x0A, 0x00, 0x00], 0, 10),
        ] {
            let mut w = World::new();
            w.mode = crate::world::SceneMode::Field;
            w.clock.display_frame_step = 1;
            w.minigames.casino_coins = start;
            w.load_field_script(script);
            for _ in 0..4 {
                let _ = w.tick();
            }
            assert_eq!(w.minigames.casino_coins, want);
            assert!(w.system_flag_test(COIN_DELTA_FLAG));
        }
    }
}
