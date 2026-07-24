//! Fishing-overlay **chrome kernels** - the small leaf routines the venue's
//! sub-screens share: the centred panel frame, the idle sway vector, the
//! catch-slot table reset and the scene camera reset.
//!
//! Every routine here is a leaf of the slot-A minigame overlay PROT 0972
//! (link base `0x801CE818`), the image the *debug-menu* overlay's own dumps
//! alias: PROT 0971's own content stops at file `+0x1800` (VA `0x801D0018`),
//! so a dump taken from the debug-menu image above that VA is 0972's bytes
//! read through 0971's extraction footprint. All four addresses below sit
//! above the split, so they are fishing-overlay routines whatever the dump
//! filename says. See `docs/tooling/dump-corpus-integrity.md`.

/// Steps of the shared sine table (`*_DAT_8007B81C`), a full turn.
pub const SINE_TURN: i32 = 0x1000;
/// Quarter turn - the phase offset between the sway triple's components.
pub const SINE_QUARTER: i32 = 0x400;
/// Constant bias subtracted from every sway component after the shift.
pub const SWAY_BIAS: i32 = 0xA;
/// Angle advance per tick, before the frame-step multiply.
pub const SWAY_STEP_SHIFT: u32 = 4;

/// The three-component sway offset plus the (unused) leading halfword the
/// routine clears, in the order retail stores them into the render scratch
/// block at `0x1F80035C`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SwayVector {
    /// `+0x48` - cleared every call.
    pub pad: i16,
    /// `+0x4A` - sample at the current angle.
    pub x: i16,
    /// `+0x4C` - sample a quarter turn on.
    pub y: i16,
    /// `+0x4E` - sample a half turn on.
    pub z: i16,
}

// NOT WIRED: the block it writes is the PSX scratchpad render struct at
// `0x1F800314`, which the clean-room renderer does not model - the engine
// carries no `0x1F80035C` sway triple for a camera to read. The kernel is
// pure, so a host can call it once that block has an equivalent.
/// PORT: FUN_801D03B0 - the sub-screen idle sway.
///
/// `sine` is the shared 4096-step table the overlay reads through
/// `*_DAT_8007B81C`; `angle` is the running phase (`0x801D9118`) and
/// `frame_step` the per-frame tick (`DAT_1F800393`).
///
/// The routine samples the table three times - at `angle`, `angle + 0x400`
/// and `angle + 0x800`, i.e. sine, cosine and negated sine - and scales each
/// with the same round-toward-zero shift retail spells as
/// `bgez / addiu 0xFF / sra 8`, then subtracts [`SWAY_BIAS`]. It returns the
/// advanced angle: `angle + (frame_step << 4)`, with the *unmasked* value
/// carried forward (only the table index is masked to [`SINE_TURN`]).
pub fn sway_vector(sine: &[i16], angle: i32, frame_step: i32) -> (SwayVector, i32) {
    let sample = |phase: i32| -> i16 {
        let raw = sine
            .get((phase & (SINE_TURN - 1)) as usize)
            .copied()
            .unwrap_or(0) as i32;
        let raw = if raw >= 0 { raw } else { raw + 0xFF };
        ((raw >> 8) - SWAY_BIAS) as i16
    };
    let v = SwayVector {
        pad: 0,
        x: sample(angle),
        y: sample(angle + SINE_QUARTER),
        z: sample(angle + 2 * SINE_QUARTER),
    };
    (v, angle + (frame_step << SWAY_STEP_SHIFT))
}

/// Records in the catch-slot table the venue reset clears.
pub const CATCH_SLOTS: usize = 16;
/// Bytes per catch-slot record (two words).
pub const CATCH_SLOT_STRIDE: usize = 8;
/// Runtime VA of the catch-slot table.
pub const CATCH_SLOT_TABLE_VA: u32 = 0x801D_91E4;
/// Runtime VA of the slot-count word cleared alongside it.
pub const CATCH_SLOT_COUNT_VA: u32 = 0x801D_91DC;

// NOT WIRED: the engine's `fishing::FishingRun` owns catch bookkeeping as
// typed state, so there is no flat slot array to clear; the kernel is kept as
// the shape of the retail table.
/// PORT: FUN_801D746C - the catch-slot table reset.
///
/// Clears the count word at [`CATCH_SLOT_COUNT_VA`] and both words of all
/// [`CATCH_SLOTS`] records at [`CATCH_SLOT_TABLE_VA`]. Retail unrolls the
/// loop two words at a time (`v1` walking the low word, `a0 + base` the
/// high), which is why the table is `16 * 8` bytes and not `32 * 4`.
pub fn clear_catch_slots(count: &mut u32, table: &mut [[u32; 2]]) {
    *count = 0;
    for slot in table.iter_mut().take(CATCH_SLOTS) {
        *slot = [0, 0];
    }
}

/// The camera pose the fishing venue resets to: rotation trio
/// `_DAT_8007B790/92/94` and translation `TR.x` / `TR.z`
/// (`_DAT_800840B8` / `_DAT_800840C0`). `TR.y` (`_DAT_800840BC`) is
/// deliberately left alone - the routine never touches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VenueCameraReset {
    pub rot: [i16; 3],
    pub tr_x: i32,
    pub tr_z: i32,
}

/// Camera distance the venue reset installs (`TR.z`).
pub const VENUE_CAMERA_TR_Z: i32 = 0x974;

// NOT WIRED: `engine-core::camera` drives the field camera from a scene
// controller, not from the retail global trios; a fishing host would apply
// this through that controller instead of writing the globals.
/// PORT: FUN_801D78C0 - the venue camera reset.
///
/// Zeroes the whole rotation trio and `TR.x`, and parks `TR.z` at
/// [`VENUE_CAMERA_TR_Z`] - a fixed head-on framing of the fishing spot. The
/// same global pair the battle and world-map cameras use
/// (`docs/subsystems/battle.md`).
pub const fn venue_camera_reset() -> VenueCameraReset {
    VenueCameraReset {
        rot: [0, 0, 0],
        tr_x: 0,
        tr_z: VENUE_CAMERA_TR_Z,
    }
}

/// The rectangle a centred panel frame resolves to, in the argument order the
/// box-widget dispatcher takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CentredPanel {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
}

/// Widget kind the panel frame stages before the emit (`FUN_80034B6C(0x44)`).
pub const PANEL_WIDGET_KIND: u8 = 0x44;
/// Screen-y past which the panel is suppressed entirely.
pub const PANEL_Y_CUTOFF: i16 = 0xF1;

// NOT WIRED: the fishing sub-screens the panel frames are drawn by
// `engine-ui::ui_fishing`, which lays its own frames out; nothing calls this
// until a host runs the retail sub-screen geometry instead.
/// PORT: FUN_801D74B0 - the centred panel frame.
///
/// `(cx, y, w, h)`: stages widget kind [`PANEL_WIDGET_KIND`] and emits the
/// box dispatcher `FUN_8002C69C` at `(cx - w / 2 - 2, y + 6, w, h)` - the
/// caller passes a **centre** x and the routine converts it to the left edge,
/// biasing two pixels left and six down for the frame skin. Nothing is drawn
/// at all once `y` reaches [`PANEL_Y_CUTOFF`], which is how the venue's
/// sub-screens clip their own panels off the bottom of the screen.
///
/// The width halving is retail's `srl 31; addu; sra 1` - round toward zero,
/// so an odd width biases the panel one pixel right of true centre.
pub fn centred_panel(cx: i16, y: i16, w: i16, h: i16) -> Option<CentredPanel> {
    if y >= PANEL_Y_CUTOFF {
        return None;
    }
    let wi = w as i32;
    let half = (wi + ((wi as u32) >> 31) as i32) >> 1;
    Some(CentredPanel {
        x: (cx as i32 - half - 2) as i16,
        y: y + 6,
        w,
        h,
    })
}

/// One member of the three-part splash burst.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplashPart {
    /// Position handed to the part-spawn API `FUN_80021B04`.
    pub x: i16,
    pub y: i16,
    pub sprite_id: u16,
    /// Fixed-point scale (`0x1000` = 1.0).
    pub scale: i32,
    /// Per-axis position nudge applied to the spawned part after the call,
    /// on whichever field pair [`SplashPart::sub_block`] selects.
    pub nudge: (i32, i32),
    /// Rotation words written to `part + 0x80 + 0x20` / `+ 0x24`.
    pub rot: (u32, u32),
    /// Which field pair the nudge lands on: `true` = the `+0x80` sub-block's
    /// `+0x34/+0x36`, `false` = the part's own `+0x14/+0x16`.
    pub sub_block: bool,
}

/// Bit of the packed argument that selects the `+0x80` sub-block form.
pub const SPLASH_SUB_BLOCK_BIT: i32 = 0x1000;
/// Mask that carries the spread in the low bits of the same argument.
pub const SPLASH_SPREAD_MASK: i32 = 0xFFF;

// NOT WIRED: the engine has no minigame effect-part pool (the same gap
// `baka_fighter::EffectSpawnSpec` and `dance::step_mark_effect_spawn`
// document); this resolves the three spawn specs a host would submit.
/// PORT: FUN_801D7A5C - the three-part splash burst.
///
/// `(x, y, sprite_id, packed)` spawns the same part three times at one point
/// (`FUN_80021B04(x, y, sprite_id, 0x1000)`) and fans them apart. `packed`
/// carries the spread in its low 12 bits and a form bit at
/// [`SPLASH_SUB_BLOCK_BIT`]:
///
/// - **sub-block form** (bit set): the nudge lands on the spawned part's
///   `+0x80` sub-block (`+0x34/+0x36`), by `2 * spread` on the first part and
///   `spread` on the second; the rotation pairs are `(0, 0x00F00000)`,
///   `(0x8000, 0x8000)` and `(0x000000F0, 0)`.
/// - **direct form** (bit clear): the nudge lands on the part's own
///   `+0x14/+0x16`; the first part moves `-spread` on both axes, the third
///   `+spread` on x and `-spread` on y, and the rotation pairs are
///   `(0x00F00000, 0)`, `(0xF000, 0)` and `(0xF0, 0)`.
///
/// The middle part never moves in the direct form, and the third never moves
/// in the sub-block form - each form leaves one part as the burst anchor.
pub fn splash_burst(x: i16, y: i16, sprite_id: u16, packed: i32) -> [SplashPart; 3] {
    let spread = packed & SPLASH_SPREAD_MASK;
    let sub_block = packed & SPLASH_SUB_BLOCK_BIT != 0;
    let base = SplashPart {
        x,
        y,
        sprite_id,
        scale: 0x1000,
        nudge: (0, 0),
        rot: (0, 0),
        sub_block,
    };
    if sub_block {
        [
            SplashPart {
                nudge: (-2 * spread, -2 * spread),
                rot: (0, 0x00F0_0000),
                ..base
            },
            SplashPart {
                nudge: (-spread, -spread),
                rot: (0x8000, 0x8000),
                ..base
            },
            SplashPart {
                nudge: (0, 0),
                rot: (0x0000_00F0, 0),
                ..base
            },
        ]
    } else {
        [
            SplashPart {
                nudge: (-spread, -spread),
                rot: (0x00F0_0000, 0),
                ..base
            },
            SplashPart {
                nudge: (0, 0),
                rot: (0xF000, 0),
                ..base
            },
            SplashPart {
                nudge: (spread, -spread),
                rot: (0xF0, 0),
                ..base
            },
        ]
    }
}

/// Actor flag bit both wrappers below clear (`+0x10 &= ~2`) - the "already
/// ticked this frame" bit the shared actor dispatcher owns.
pub const ACTOR_FLAG_TICK: u32 = 0x2;

/// What [`float_actor_tick`] writes back to the actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatActorTick {
    /// New `+0x16` screen y, straight from the height solver's return.
    pub y: i16,
    /// New flag word.
    pub flags: u32,
}

// NOT WIRED: the height solver it wraps (`FUN_801D6028`) is itself unported,
// and the engine models the float as `fishing::FishingRun` state rather than
// as an actor record.
/// PORT: FUN_801D70EC - the fishing float's per-frame actor wrapper.
///
/// Calls the float height solver `FUN_801D6028(actor)`, stores its return
/// into the actor's `+0x16` screen y, clears [`ACTOR_FLAG_TICK`] and
/// re-enters the shared actor dispatcher `FUN_800204F8`. The wrapper adds
/// nothing else - it exists so the solver can be shared with the sibling
/// caller that does not want the store.
pub fn float_actor_tick(solved_y: i16, flags: u32) -> FloatActorTick {
    FloatActorTick {
        y: solved_y,
        flags: flags & !ACTOR_FLAG_TICK,
    }
}

/// Runtime VA of the ripple part descriptor the splash spawner passes.
pub const RIPPLE_DESCRIPTOR_VA: u32 = 0x801D_899C;

/// The spawn the ripple wrapper issues, in `FUN_80021B04` argument order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RippleSpawn {
    /// Position vector built on the stack: the actor's `+0x14` and `+0x18`
    /// world coordinates with a zero middle component.
    pub pos: [i16; 3],
    /// Rotation vector - always zero.
    pub rot: [i16; 3],
    /// Descriptor pointer, [`RIPPLE_DESCRIPTOR_VA`].
    pub descriptor: u32,
    /// Fixed-point scale.
    pub scale: i32,
}

// NOT WIRED: no minigame effect-part pool exists to spawn into.
/// PORT: FUN_801D7C30 - the ripple spawn wrapper.
///
/// `(actor, mode)`: a non-zero `mode` does nothing at all. Mode zero spawns
/// the [`RIPPLE_DESCRIPTOR_VA`] part at the actor's world position, with a
/// zero rotation and scale `0x1000`. Note the position vector's **middle**
/// component is the zero - the two coordinates come from `+0x14` and `+0x18`,
/// which is the world XZ pair, not a screen point.
pub fn ripple_spawn(actor_x: i16, actor_z: i16, mode: i32) -> Option<RippleSpawn> {
    if mode != 0 {
        return None;
    }
    Some(RippleSpawn {
        pos: [actor_x, 0, actor_z],
        rot: [0, 0, 0],
        descriptor: RIPPLE_DESCRIPTOR_VA,
        scale: 0x1000,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Vec<i16> {
        (0..SINE_TURN)
            .map(|i| {
                let f = (i as f64) * std::f64::consts::TAU / SINE_TURN as f64;
                (f.sin() * 4096.0).round() as i16
            })
            .collect()
    }

    #[test]
    fn sway_is_three_quarter_turn_samples() {
        let t = table();
        let (v, next) = sway_vector(&t, 0, 2);
        // sin(0) = 0 -> 0 - bias; cos = +4096 -> 16 - bias; -sin = 0.
        assert_eq!(v.pad, 0);
        assert_eq!(v.x, -(SWAY_BIAS as i16));
        assert_eq!(v.y, 16 - SWAY_BIAS as i16);
        assert_eq!(v.z, -(SWAY_BIAS as i16));
        assert_eq!(next, 2 << SWAY_STEP_SHIFT);
    }

    #[test]
    fn sway_rounds_negative_samples_toward_zero() {
        // A negative table entry takes the `+0xFF` arm before the shift, so
        // -4096 >> 8 lands on -16 and not -17.
        let mut t = vec![0i16; SINE_TURN as usize];
        t[0] = -4096;
        let (v, _) = sway_vector(&t, 0, 0);
        assert_eq!(v.x, -16 - SWAY_BIAS as i16);
    }

    #[test]
    fn sway_angle_is_not_masked_between_calls() {
        let t = table();
        let (_, a) = sway_vector(&t, SINE_TURN - 1, 1);
        assert_eq!(a, SINE_TURN - 1 + 16);
    }

    #[test]
    fn catch_slot_reset_clears_both_words() {
        let mut count = 7;
        let mut table = [[1u32, 2u32]; CATCH_SLOTS];
        clear_catch_slots(&mut count, &mut table);
        assert_eq!(count, 0);
        assert!(table.iter().all(|s| *s == [0, 0]));
    }

    #[test]
    fn venue_camera_parks_at_a_fixed_distance() {
        let c = venue_camera_reset();
        assert_eq!(c.rot, [0, 0, 0]);
        assert_eq!(c.tr_x, 0);
        assert_eq!(c.tr_z, 0x974);
    }

    #[test]
    fn panel_centres_and_biases() {
        // The menu picker's own call: FUN_801D74B0(0xA0, 0x50, 0x68, 0x50).
        let p = centred_panel(0xA0, 0x50, 0x68, 0x50).unwrap();
        assert_eq!(p.x, 0xA0 - 0x34 - 2);
        assert_eq!(p.y, 0x56);
        assert_eq!((p.w, p.h), (0x68, 0x50));
    }

    #[test]
    fn panel_halving_rounds_toward_zero() {
        let p = centred_panel(100, 0, 7, 4).unwrap();
        assert_eq!(p.x, 100 - 3 - 2);
    }

    #[test]
    fn panel_clips_below_the_cutoff() {
        assert!(centred_panel(0xA0, PANEL_Y_CUTOFF, 8, 8).is_none());
        assert!(centred_panel(0xA0, PANEL_Y_CUTOFF - 1, 8, 8).is_some());
    }

    #[test]
    fn float_tick_clears_only_the_tick_bit() {
        let t = float_actor_tick(-7, 0xFF);
        assert_eq!(t.y, -7);
        assert_eq!(t.flags, 0xFD);
    }

    #[test]
    fn ripple_spawns_only_in_mode_zero() {
        assert!(ripple_spawn(1, 2, 1).is_none());
        let s = ripple_spawn(1, 2, 0).unwrap();
        assert_eq!(s.pos, [1, 0, 2]);
        assert_eq!(s.rot, [0, 0, 0]);
        assert_eq!(s.descriptor, RIPPLE_DESCRIPTOR_VA);
        assert_eq!(s.scale, 0x1000);
    }

    #[test]
    fn splash_forms_differ_in_axis_and_rotation() {
        let h = splash_burst(10, 20, 3, 0x40);
        assert!(!h[0].sub_block);
        assert_eq!(h[0].nudge, (-0x40, -0x40));
        assert_eq!(h[1].nudge, (0, 0));
        assert_eq!(h[2].nudge, (0x40, -0x40));
        assert_eq!(h[1].rot, (0xF000, 0));

        let v = splash_burst(10, 20, 3, 0x1040);
        assert!(v[0].sub_block);
        assert_eq!(v[0].nudge, (-0x80, -0x80));
        assert_eq!(v[1].rot, (0x8000, 0x8000));
        assert_eq!(v[2].nudge, (0, 0));
        // The spread is masked out of the packed word, not read from it whole.
        assert_eq!(v[0].nudge.0, -2 * 0x40);
    }
}
