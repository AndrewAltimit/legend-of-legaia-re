//! The story-flag half of MAIN INIT's **back-from-battle arm** - the block
//! every battle end passes through on its way back to the field, and the
//! only writer of the script-readable battle outcome.
//!
//! PORT: FUN_8003AEB0 (`0x8003B518..0x8003B60C`, the story-flag stores of the
//! `_DAT_8007B8B8 == 2` arm)
//!
//! `FUN_8003AEB0` tests the battle-return marker (`lw v1,-0x4748(v0)`,
//! `bne v1,2` at `0x8003B510`) and, on a return from battle, rewrites four
//! bytes of the shared flag bank `DAT_80085758` (`0x80084140 + 0x1618`,
//! MSB-first: flag `n` is bit `0x80 >> (n & 7)` of byte `n >> 3`):
//!
//! ```text
//! 8003b534  lbu  v0,0x1618(v1)      ; 0x80085759
//! 8003b53c  and  v0,v0,a3           ; a3 = ~2: clear flag 14
//! 8003b54c  andi v0,v1,0x8          ; 0x8008575B bit 0x08 = flag 28 ...
//! 8003b55c  beq  a1,zero,...        ; ... set ->
//! 8003b560  _andi v0,v1,0xfb        ;    clear flag 29 (delay slot)
//! 8003b564  and  v0,v0,a3           ;    and flag 30
//! 8003b570  lbu  v0,-0x42a0(v0)     ; DAT_8007BD60, the party-survived bit
//! 8003b57c  beq  v0,zero,0x8003b598 ; clear -> the wipe arm
//! 8003b58c  ori  v0,v0,0x40         ; survived: SET flag 1
//! 8003b5a0  andi v0,v0,0xbf         ; wiped:    CLEAR flag 1
//! 8003b5bc  bne  v1,zero,0x8003b5f8 ;   flag 0 set -> back to the field
//! 8003b5d4  sh   v0,-0x47c4(v1)     ;   else game_mode = 0x16 (the title)
//! 8003b608  andi v0,v0,0x7f         ; every arm: CLEAR flag 0
//! ```
//!
//! So flag `1` is the **battle outcome** a scene script can test after the
//! fight - set on every survived end, cleared on a wipe - and flag `0` is the
//! scripted-loss latch, consumed on every return whatever the outcome.
//!
//! The survived bit `DAT_8007BD60 & 0x80` is seeded at battle load, cleared
//! by the action SM's `0x5A` wipe scans and raised again on the surviving
//! exits: the results sequencer's phase 4 (a monster wipe), the escape roll's
//! success arm (`0x801E802C`), the sparring fight's exit arm (`0x801F735C`,
//! PROT 0967) and Cort's form transition (PROT 0969). A party wipe is the one
//! battle end that leaves it clear, which is why the port keys it on
//! [`legaia_engine_vm::battle_action::BattleEndCause::PartyWipe`] alone.
//!
//! Flags 14, 29 and 30 are cleared here too; the port reproduces the stores
//! without a reading of what those three flags gate.
//!
//! Source: `ghidra/scripts/funcs/8003aeb0.txt` (disassembly).

/// Story flag MAIN INIT **sets** on a survived battle end and **clears** on a
/// wipe (`ori 0x40` at `0x8003B58C` / `andi 0xbf` at `0x8003B5A0`).
pub const BATTLE_OUTCOME_FLAG: u16 = 1;

/// Story flag MAIN INIT **clears** on every return from battle (`andi 0x7f`
/// at `0x8003B608`): the scripted-loss latch a scene raises before a fight
/// the party is meant to lose.
pub const SCRIPTED_LOSS_LATCH_FLAG: u16 = 0;

/// Flag cleared unconditionally on every return (`0x80085759 & ~0x02`,
/// `0x8003B534..0x8003B540`).
pub const CLEARED_ON_RETURN_FLAG: u16 = 14;

/// Flag whose set state clears [`GATED_CLEAR_FLAGS`] on return
/// (`0x8008575B & 0x08`, `0x8003B54C`).
pub const GATE_FLAG: u16 = 28;

/// Flags cleared on return when [`GATE_FLAG`] is set (`0x8008575B &
/// ~0x06`, `0x8003B560..0x8003B568`).
pub const GATED_CLEAR_FLAGS: [u16; 2] = [29, 30];

/// A story-flag bank the back-from-battle arm reads and writes.
pub trait FlagBank {
    fn test(&self, idx: u16) -> bool;
    fn set(&mut self, idx: u16);
    fn clear(&mut self, idx: u16);
}

/// What the arm decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleReturn {
    /// The wipe arm with no scripted-loss latch: `game_mode = 0x16` and the
    /// title context word (`0x8003B5C4..0x8003B5EC`) - the game over.
    pub game_over: bool,
}

/// Run the arm's flag stores for a battle that ended with the party-survived
/// bit `survived`.
pub fn apply_battle_return_flags(bank: &mut impl FlagBank, survived: bool) -> BattleReturn {
    bank.clear(CLEARED_ON_RETURN_FLAG);
    if bank.test(GATE_FLAG) {
        for f in GATED_CLEAR_FLAGS {
            bank.clear(f);
        }
    }
    let mut game_over = false;
    if survived {
        bank.set(BATTLE_OUTCOME_FLAG);
    } else {
        bank.clear(BATTLE_OUTCOME_FLAG);
        // `0x8003B5A8..0x8003B5BC`: the latch is read BEFORE the shared
        // clear below, so a scripted loss skips the hand-off.
        game_over = !bank.test(SCRIPTED_LOSS_LATCH_FLAG);
    }
    bank.clear(SCRIPTED_LOSS_LATCH_FLAG);
    BattleReturn { game_over }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Bank([u8; 8]);
    impl FlagBank for Bank {
        fn test(&self, idx: u16) -> bool {
            self.0[usize::from(idx >> 3)] & (0x80 >> (idx & 7)) != 0
        }
        fn set(&mut self, idx: u16) {
            self.0[usize::from(idx >> 3)] |= 0x80 >> (idx & 7);
        }
        fn clear(&mut self, idx: u16) {
            self.0[usize::from(idx >> 3)] &= !(0x80 >> (idx & 7));
        }
    }

    #[test]
    fn flag_numbers_are_the_retail_bit_masks() {
        // byte 0 bit 0x40 / 0x80, byte 1 bit 0x02, byte 3 bits 0x08 / 0x04 / 0x02.
        let mut b = Bank::default();
        b.set(BATTLE_OUTCOME_FLAG);
        b.set(SCRIPTED_LOSS_LATCH_FLAG);
        b.set(CLEARED_ON_RETURN_FLAG);
        b.set(GATE_FLAG);
        b.set(GATED_CLEAR_FLAGS[0]);
        b.set(GATED_CLEAR_FLAGS[1]);
        assert_eq!(b.0[..4], [0xC0, 0x02, 0x00, 0x0E]);
    }

    #[test]
    fn a_survived_battle_sets_the_outcome_flag_and_consumes_the_latch() {
        let mut b = Bank::default();
        b.set(SCRIPTED_LOSS_LATCH_FLAG);
        let r = apply_battle_return_flags(&mut b, true);
        assert!(!r.game_over);
        assert!(b.test(BATTLE_OUTCOME_FLAG));
        assert!(!b.test(SCRIPTED_LOSS_LATCH_FLAG));
    }

    #[test]
    fn a_wipe_clears_the_outcome_flag_and_the_latch_decides_the_game_over() {
        let mut b = Bank::default();
        b.set(BATTLE_OUTCOME_FLAG);
        assert!(apply_battle_return_flags(&mut b, false).game_over);
        assert!(!b.test(BATTLE_OUTCOME_FLAG));

        let mut b = Bank::default();
        b.set(BATTLE_OUTCOME_FLAG);
        b.set(SCRIPTED_LOSS_LATCH_FLAG);
        assert!(!apply_battle_return_flags(&mut b, false).game_over);
        assert!(!b.test(BATTLE_OUTCOME_FLAG));
        assert!(!b.test(SCRIPTED_LOSS_LATCH_FLAG));
    }

    #[test]
    fn the_side_clears_follow_their_gate() {
        let mut b = Bank::default();
        b.set(CLEARED_ON_RETURN_FLAG);
        b.set(GATED_CLEAR_FLAGS[0]);
        b.set(GATED_CLEAR_FLAGS[1]);
        apply_battle_return_flags(&mut b, true);
        assert!(!b.test(CLEARED_ON_RETURN_FLAG));
        // Gate clear: 29 / 30 survive.
        assert!(b.test(GATED_CLEAR_FLAGS[0]) && b.test(GATED_CLEAR_FLAGS[1]));
        b.set(GATE_FLAG);
        apply_battle_return_flags(&mut b, true);
        assert!(!b.test(GATED_CLEAR_FLAGS[0]) && !b.test(GATED_CLEAR_FLAGS[1]));
        assert!(b.test(GATE_FLAG), "the gate flag itself is left alone");
    }
}
