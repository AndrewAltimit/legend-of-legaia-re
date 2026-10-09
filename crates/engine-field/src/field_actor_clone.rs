//! The field overlay's **actor clone** - the spawn behind field-VM op `0x4C`
//! sub-1 sub-op `0x14`, and the one allocation site of the static actor
//! template at SCUS `0x80070644` whose `+0x08` tick is
//! [`legaia_engine_vm::actor_tick::clip_fraction_step`].
//!
//! PORT: FUN_801D835C
//! REF: FUN_80020DE0 (the pool allocator the helper calls),
//! REF: FUN_8003C83C (the id resolve the dispatcher arm runs first),
//! REF: FUN_801D820C (the clone's own per-frame tick)
//!
//! # What the script asks for
//!
//! The dispatcher arm at `0x801E0E80` is eight bytes wide -
//! `4C 14 <r> <g> <b> <rate_lo> <rate_hi> <src_id>` - and hands the helper
//! three things: the resolved source actor, the 24-bit word
//! `FUN_8003CEB8(&operand[1])`, and the sign-extended `s16`
//! `FUN_8003CE9C(&operand[4])`. Six shipped scenes issue it (`vozz`,
//! `retona`, `urudre3`, `kor5`, `nilboa`, `noaru`), always as a short burst
//! against one target - `vozz` runs three at eight-frame spacing with the
//! word stepping `(0x32,0x28,0x1E)`, `(0x37,0x2D,0x23)`, `(0x3C,0x32,0x28)`.
//!
//! # What the helper does
//!
//! Reading `0x801D835C..0x801D8418` off `extracted/overlays/overlay_field_0897.bin`
//! at base `0x801CE818` (48 instructions):
//!
//! 1. `sh src[+0x64], 4(0x80070644)` - the descriptor's `+0x04` word is
//!    `0xFFFF0000` in the image, and this is a **halfword** store, so only its
//!    low half is replaced. The clone therefore inherits the source's `+0x64`
//!    animation-set word through the descriptor rather than by a direct copy.
//! 2. `FUN_80020DE0(0x80070644, _DAT_8007C34C)` - allocate off the generic
//!    effect-actor list. A null return ends the routine with nothing written.
//! 3. Copy `src[+0x14..+0x1B]` and `src[+0x24..+0x2B]` verbatim through
//!    `lwl`/`lwr` pairs - the position triple and the rotation triple, each
//!    with the fourth halfword carried along.
//! 4. Copy the `u32` at `src[+0x4C]` - the bound model/record pointer, so the
//!    clone draws the same body.
//! 5. `dst[+0x54] = rate`, `dst[+0x74] = word`, `dst[+0x68] = src[+0x68]`.
//!
//! `+0x54` is what the clone's tick multiplies by the cadence byte each
//! frame, and `+0x74` is the modulation colour the sprite/widget family reads
//! as a packed RGB (`FUN_801F7A9C` draws from it, `FUN_801F8004` writes it;
//! see `docs/subsystems/move-vm.md`). So a burst of these is a fading,
//! tinted copy of the target - an after-image - and the `s16` is its speed.
//!
//! The rate is what bounds the clone's life: the tick retires it once the
//! 12-bit accumulator at `+0x78` reaches `0x1000`, so `vozz`'s `0x199` is
//! about ten vsyncs.

use legaia_engine_vm::actor_tick::CLIP_FRACTION_FULL;

/// Spawn descriptor the helper allocates from - the third record of the
/// static actor-template table at `0x800705FC`.
///
/// Its words in `SCUS_942.54` are `[+0x00] = 0x00150000` (id `0x15` at
/// `+0x02`), `[+0x04] = 0xFFFF0000`, `[+0x08] = 0x801D820C` (the tick) and
/// `[+0x0C] = 0x80` (the flag word); `[+0x14]` is the initial state `1`.
pub const CLONE_DESCRIPTOR: u32 = 0x8007_0644;

/// The descriptor's `+0x08` handler word, i.e. the clone's `+0x0C`.
pub const CLONE_HANDLER: u32 = 0x801D_820C;

/// The descriptor's `+0x0C` flag word.
pub const CLONE_DESCRIPTOR_FLAGS: u32 = 0x80;

/// The descriptor's `+0x14` initial state word.
pub const CLONE_DESCRIPTOR_INITIAL_STATE: u16 = 1;

/// The fields the helper reads off the **source** actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CloneSource {
    /// `+0x14 / +0x16 / +0x18`.
    pub pos: (i16, i16, i16),
    /// `+0x24 / +0x26 / +0x28`.
    pub rot: (i16, i16, i16),
    /// `+0x64` - the animation-set word, routed through the descriptor's
    /// `+0x04` low halfword.
    pub anim_set: u16,
    /// `+0x68`.
    pub field_68: i16,
}

/// The clone the helper builds, as field writes rather than as a pool node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ClonePlan {
    /// `+0x14 / +0x16 / +0x18`, copied.
    pub pos: (i16, i16, i16),
    /// `+0x24 / +0x26 / +0x28`, copied.
    pub rot: (i16, i16, i16),
    /// `+0x54` - the dispatcher's `s16`, the per-vsync rate of the clone's
    /// own tick.
    pub rate: i16,
    /// `+0x74` - the dispatcher's 24-bit word, the modulation colour.
    pub modulation: u32,
    /// `+0x68`, copied.
    pub field_68: i16,
    /// `+0x78` - the clip accumulator starts at zero (the allocator zeroes
    /// the node and the helper never writes it).
    pub fraction: i16,
    /// The descriptor `+0x04` low halfword the allocation carries, i.e. the
    /// source's `+0x64`.
    pub anim_set: u16,
}

impl ClonePlan {
    /// Vsyncs this clone survives at a cadence of one vsync per tick - the
    /// count of [`legaia_engine_vm::actor_tick::clip_fraction_step`] calls
    /// before it sets its own retire bit. `0` for a non-positive rate, which
    /// never reaches the ceiling and so never retires.
    pub fn lifetime_vsyncs(&self) -> u32 {
        if self.rate <= 0 {
            return 0;
        }
        let rate = u32::from(self.rate as u16);
        u32::from(CLIP_FRACTION_FULL).div_ceil(rate)
    }
}

/// Build the clone `FUN_801D835C` would allocate.
///
/// PORT: FUN_801D835C
///
/// `src` is the resolved source actor's fields; `modulation` and `rate` are
/// the dispatcher arm's two decoded operands. Retail returns null when the
/// pool is empty - that failure belongs to the allocator, so this kernel
/// always yields the plan and the caller decides whether a slot exists.
pub fn clone_plan(src: CloneSource, modulation: u32, rate: i16) -> ClonePlan {
    ClonePlan {
        pos: src.pos,
        rot: src.rot,
        rate,
        // The `sw` at `0x801D83FC` stores the whole word the dispatcher's
        // `FUN_8003CEB8` produced; that decoder only ever fills 24 bits.
        modulation: modulation & 0x00FF_FFFF,
        field_68: src.field_68,
        fraction: 0,
        anim_set: src.anim_set,
    }
}

/// The modulation word split into the RGB lanes the sprite family reads
/// (`+0x74` low byte first, matching the `FUN_8003CEB8` operand order).
pub fn modulation_rgb(word: u32) -> [u8; 3] {
    [
        (word & 0xFF) as u8,
        ((word >> 8) & 0xFF) as u8,
        ((word >> 16) & 0xFF) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src() -> CloneSource {
        CloneSource {
            pos: (0x100, -0x40, 0x200),
            rot: (1, 2, 3),
            anim_set: 0x1234,
            field_68: 0x55,
        }
    }

    #[test]
    fn the_plan_copies_the_transform_and_takes_the_two_operands() {
        let p = clone_plan(src(), 0x001E_2832, 0x0199);
        assert_eq!(p.pos, (0x100, -0x40, 0x200));
        assert_eq!(p.rot, (1, 2, 3));
        assert_eq!(p.rate, 0x0199);
        assert_eq!(p.modulation, 0x001E_2832);
        assert_eq!(p.field_68, 0x55);
        assert_eq!(p.fraction, 0);
        assert_eq!(p.anim_set, 0x1234);
    }

    #[test]
    fn the_modulation_word_is_24_bit() {
        // Nothing on the disc sets the top byte, but the field is a `u32`
        // and the `sw` keeps whatever it is handed.
        assert_eq!(clone_plan(src(), 0xFF1E_2832, 1).modulation, 0x001E_2832);
        assert_eq!(modulation_rgb(0x001E_2832), [0x32, 0x28, 0x1E]);
    }

    #[test]
    fn the_vozz_burst_lives_about_ten_vsyncs() {
        assert_eq!(clone_plan(src(), 0x001E_2832, 0x0199).lifetime_vsyncs(), 11);
        // A zero or negative rate never reaches the ceiling.
        assert_eq!(clone_plan(src(), 0, 0).lifetime_vsyncs(), 0);
        assert_eq!(clone_plan(src(), 0, -4).lifetime_vsyncs(), 0);
    }
}
