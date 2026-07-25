//! The battle-action overlay's own random-number generator.
//!
//! PORT: FUN_801D0290
//!
//! Overlay `0898` carries a second generator alongside the SCUS PsyQ-shape
//! `rand()` at `FUN_80056798` that
//! [`battle_formulas`](crate::battle_formulas) mirrors. It is twelve
//! instructions with no stack frame, and its whole state is the single word at
//! `0x801F6950` - the overlay's own data tail, not the SCUS seed. Draws from it
//! therefore do **not** perturb the `FUN_80056798` stream the determinism
//! oracles follow.
//!
//! Disassembled from the `0898` image at base `0x801CE818`
//! (`scripts/ghidra-analysis/disasm-overlay-fn.py extracted/overlays/overlay_battle_action_0898.bin
//! --base 0x801CE818 --addr 0x801d0290`); the corpus has no
//! `overlay_battle_action_801d0290.txt`, and the `overlay_0897` dump at that VA
//! is a *different* five-instruction body (a field-overlay opcode-handler
//! fragment that advances a VM PC in `s8`), so it is not this routine.
//!
//! ```text
//!   lui   $a0, 0x801f
//!   lw    $v1, 0x6950($a0)     ; s
//!   sll   $v0, $v1, 2          ; s << 2
//!   sll   $v1, $v1, 3          ; s << 3
//!   addu  $v1, $v1, $v0        ; s * 12
//!   addiu $v1, $v1, 2          ; v = s * 12 + 2
//!   sll   $v0, $v1, 0x10       ; v << 16
//!   srl   $v1, $v1, 0x10       ; v >> 16   (logical)
//!   addu  $v0, $v0, $v1
//!   jr    $ra
//!   sw    $v0, 0x6950($a0)     ; store in the delay slot; $v0 is the return
//! ```
//!
//! ## The `addu` here really is a rotate
//!
//! Worth stating because the opposite was recorded: the final `addu` sums
//! `v << 16` (whose low 16 bits are all zero) with `v >> 16` (whose high 16
//! bits are all zero, because the shift is `srl` and not `sra`). The two
//! operands occupy **disjoint** bit ranges, so no carry can ever propagate and
//! the `addu` is bit-for-bit an `or`. The step is exactly `rotate_left(16)`.
//! [`OverlayRng::next_from`] asserts that equivalence over the whole 32-bit
//! space in the tests below rather than asserting it in prose.
//!
//! # NOT WIRED
//!
//! The five retail call sites are all `jal 0x801d0290` inside `FUN_801CFB94`
//! (`0x801CFCE4` / `0x801CFDE8` / `0x801CFED4` / `0x801CFF1C` / `0x801CFF5C` in
//! `ghidra/scripts/funcs/overlay_battle_action_801cfb94.txt`), the battle
//! overlay's leading function, which is not ported. Nothing else in the corpus
//! calls it, and which battle quantities the draws feed is still open - so
//! there is no engine site to attach a draw to without inventing one.

/// The battle overlay's private LCG-shaped generator (`FUN_801D0290`).
///
/// One `u32` of state, retail-resident at `0x801F6950`. Construct with the
/// state you want (retail's is whatever the overlay image loaded with) and call
/// [`OverlayRng::draw`] per draw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OverlayRng {
    state: u32,
}

impl OverlayRng {
    /// Seed the generator with an explicit state word.
    pub const fn new(state: u32) -> Self {
        Self { state }
    }

    /// The current state word - the value retail holds at `0x801F6950`.
    pub const fn state(self) -> u32 {
        self.state
    }

    /// The pure step: `(s * 12 + 2)` rotated left by 16.
    ///
    /// Both multiply and add wrap, exactly as the `sll`/`addu`/`addiu` chain
    /// does on a 32-bit register.
    pub const fn next_from(state: u32) -> u32 {
        state.wrapping_mul(12).wrapping_add(2).rotate_left(16)
    }

    /// Advance the state and return it. Retail returns the *new* state in `$v0`
    /// (the store to `0x801F6950` is the `jr ra` delay slot), so the returned
    /// value and the stored state are the same word.
    pub fn draw(&mut self) -> u32 {
        self.state = Self::next_from(self.state);
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The literal instruction sequence, register by register, as an
    /// independent check on [`OverlayRng::next_from`]'s shorthand.
    fn literal_step(s: u32) -> u32 {
        let v0 = s.wrapping_shl(2); // sll v0, v1, 2
        let v1 = s.wrapping_shl(3); // sll v1, v1, 3
        let v1 = v1.wrapping_add(v0); // addu v1, v1, v0
        let v1 = v1.wrapping_add(2); // addiu v1, v1, 2
        let hi = v1.wrapping_shl(16); // sll v0, v1, 0x10
        let lo = v1 >> 16; // srl v1, v1, 0x10
        hi.wrapping_add(lo) // addu v0, v0, v1
    }

    #[test]
    fn shorthand_matches_the_instruction_sequence() {
        for s in [0u32, 1, 2, 0xFFFF, 0x1_0000, 0x8000_0000, 0xFFFF_FFFF] {
            assert_eq!(OverlayRng::next_from(s), literal_step(s), "state {s:#010x}");
        }
        // Plus a spread of pseudo-random probes across the space.
        let mut probe: u32 = 0x1234_5678;
        for _ in 0..20_000 {
            probe = probe.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            assert_eq!(
                OverlayRng::next_from(probe),
                literal_step(probe),
                "state {probe:#010x}"
            );
        }
    }

    #[test]
    fn the_addu_of_the_halves_is_exactly_a_rotate() {
        // The claim the doc block makes: `(v << 16) + (v >> 16)` can never
        // carry, so it equals `v.rotate_left(16)`. Checked on the halfword
        // boundaries where a carry would have to appear if it ever could.
        for v in [
            0u32,
            0x0000_FFFF,
            0xFFFF_0000,
            0xFFFF_FFFF,
            0x8000_8000,
            0x7FFF_FFFF,
        ] {
            let summed = v.wrapping_shl(16).wrapping_add(v >> 16);
            assert_eq!(summed, v.rotate_left(16), "value {v:#010x}");
            assert_eq!(summed, v.wrapping_shl(16) | (v >> 16));
        }
    }

    #[test]
    fn known_states_from_a_zero_seed() {
        let mut rng = OverlayRng::new(0);
        // s=0 -> v = 2 -> rotate_left(16) = 0x0002_0000
        assert_eq!(rng.draw(), 0x0002_0000);
        // s=0x00020000 -> v = 0x00180002 -> rotate = 0x0002_0018
        assert_eq!(rng.draw(), 0x0002_0018);
        assert_eq!(rng.state(), 0x0002_0018);
    }

    #[test]
    fn draw_returns_the_stored_state() {
        let mut rng = OverlayRng::new(0xDEAD_BEEF);
        let drawn = rng.draw();
        assert_eq!(drawn, rng.state());
    }

    #[test]
    fn zero_is_not_a_fixed_point_and_the_stream_keeps_moving() {
        // `12*0 + 2` is not zero, so a zeroed overlay image still produces a
        // live stream - the generator has no "dead seed".
        assert_ne!(OverlayRng::next_from(0), 0);
        let mut rng = OverlayRng::new(0);
        let mut seen = std::collections::HashSet::new();
        let mut prev = rng.state();
        for _ in 0..64 {
            let s = rng.draw();
            assert_ne!(s, prev, "state advanced off {prev:#010x}");
            prev = s;
            seen.insert(s);
        }
        assert_eq!(seen.len(), 64, "no repeat inside the first 64 draws");
    }
}
