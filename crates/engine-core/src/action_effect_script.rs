//! Battle **action effect script**: the 8-byte record stream a battle action
//! walks to place its visual effects, and the move-power install + homing seed
//! it does when the stream terminates.
//!
//! PORT: FUN_801DEA50
//!
//! One call per frame for the acting actor. The actor carries a cursor byte at
//! `+0x1F5`; each call consumes records from that cursor until it hits a
//! terminator or runs the cursor to `8`. Every record is an offset in the
//! actor's own local frame, which the stepper **rotates by the actor's facing**
//! (`actor[+0x46]`) before spawning anything, so an art authored facing "north"
//! plays correctly whichever way the actor is turned.
//!
//! Three things happen per record, and only the first is effect-specific:
//!
//! 1. the record's offset is rotated into world space and an effect is spawned
//!    at it (`FUN_80050ED4` for the table-driven forms, `FUN_801DFDF0` for the
//!    `0x80`-flagged direct form);
//! 2. on the terminator, the acting actor's **move-power record** is installed
//!    at the battle context's `+0x1014` and its `+0x04` word mirrored to
//!    `+0x6C6`;
//! 3. every target slot gets its homing state seeded - phase `+0x24E`, launch
//!    position `+0x1144`, bearing `+0x1166` and target index `+0x252`.
//!
//! The kernels here are the deterministic ones: the record layout, the facing
//! rotation, the move-power index chain, the effect-id classification, and the
//! target **band** the homing seed sweeps. The spawns themselves come back as a
//! list of requests rather than calls, so a host can route them at whatever
//! layer owns effects.
//!
//! `see ghidra/scripts/funcs/overlay_battle_action_801dea50.txt` and
//! [`docs/formats/move-power.md`](../../../docs/formats/move-power.md) for the
//! record the terminator installs.
//!
//! REF: FUN_80050ED4, FUN_801DFDF0 (effect spawn), FUN_801E295C (the action SM
//! that drives this), FUN_80019B28 (the bearing helper)

/// Bytes per effect-script record.
pub const RECORD_STRIDE: usize = 8;

/// Byte offset of record 0 inside the script block (`0x801deca8`: the cursor
/// scales by `8` then adds `0x14`).
pub const RECORD_BASE: usize = 0x14;

/// Cursor values `>= this` end the walk (`0x801dec6c`: `sltiu v0,v0,0x8`).
pub const MAX_CURSOR: u8 = 8;

/// Record `+0x01` low-7-bit value that marks the stream's end. The two spawn
/// branches test it differently - the direct branch compares the whole byte
/// against `0xFF` (`0x801dee88`) and the table branch compares against `0x7F`
/// (`0x801df0d4`) - and since the branch is selected by bit `0x80`, the two
/// tests together are exactly "`effect & 0x7F == 0x7F`".
pub const TERMINATOR_EFFECT_MASKED: u8 = 0x7F;

/// Slots the terminator's unconditional homing pre-seed sweeps
/// (`0x801df294..0x801df2e0`: `ctx[+0x24E + i] = 1` and the launch position
/// into `ctx[+0x1144 + i*8]`, for `i` in `0..4`).
pub const HOMING_SLOT_COUNT: usize = 4;

/// Record `+0x01` bit that selects the **direct** spawn form (`FUN_801DFDF0`)
/// over the table form.
pub const EFFECT_DIRECT_BIT: u8 = 0x80;

/// PSX angle units in a full revolution.
const ANGLE_MASK: i32 = 0xFFF;

/// Half a revolution - the bias the stepper adds to the facing before indexing
/// the LUTs.
const ANGLE_HALF: i32 = 0x800;

/// Fixed-point shift of the rotation products.
const ROT_SHIFT: u32 = 12;

/// Move-power table stride (`0x801df264..0x801df27c` builds `id * 0x1A` out of
/// shifts and adds).
pub const MOVE_POWER_STRIDE: usize = 0x1A;

/// Target-slot band the homing seed sweeps, derived from the acting actor's
/// `+0x1DD` scope byte.
///
/// PORT: FUN_801DEA50 (`0x801df2e4..0x801df338`).
///
/// The byte is a scope selector, not a slot index:
///
/// - `0..=7`: a single slot - `first == last == scope`, so the sweep touches
///   exactly that actor.
/// - `9`: the **monster** row - slots `3..=6`.
/// - `8`: the **party** row - slots `0..=2`.
/// - anything else: the registers keep whatever the `0..=7` arm left, which for
///   a fresh call is the raw byte in both, i.e. the single-slot form.
///
/// Retail's loop is `for (i = 0; first + i <= last; i++)`, so an empty band is
/// impossible - the single-slot form always runs once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetBand {
    /// First slot index the sweep visits.
    pub first: u8,
    /// Last slot index (inclusive).
    pub last: u8,
}

impl TargetBand {
    /// Classify the acting actor's `+0x1DD` scope byte.
    pub fn from_scope(scope: u8) -> Self {
        match scope {
            9 => Self { first: 3, last: 6 },
            8 => Self { first: 0, last: 2 },
            s => Self { first: s, last: s },
        }
    }

    /// The slots the sweep visits, in order.
    pub fn slots(self) -> impl Iterator<Item = u8> {
        (0u8..).map_while(move |i| {
            let s = self.first.checked_add(i)?;
            (s <= self.last).then_some(s)
        })
    }
}

/// One decoded effect-script record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectRecord {
    /// `+0x00` - the frame gate. The stepper skips the whole record while the
    /// action's frame counter plus one is below this.
    pub frame: u8,
    /// `+0x01` - the effect selector. [`TERMINATOR_EFFECT_MASKED`] in the low
    /// seven bits ends the stream; [`EFFECT_DIRECT_BIT`] picks the direct spawn
    /// form.
    pub effect: u8,
    /// `+0x02` (`i16`) - local-frame X offset.
    pub off_x: i16,
    /// `+0x04` (`i16`) - local-frame Y offset. Subtracted from the actor's Y,
    /// not added, and scaled by the actor's mesh scale.
    pub off_y: i16,
    /// `+0x06` (`i16`) - local-frame Z offset.
    pub off_z: i16,
}

impl EffectRecord {
    /// Decode the record at `cursor` from a script block.
    ///
    /// PORT: FUN_801DEA50 (`0x801dec84..0x801ded48`).
    pub fn at(block: &[u8], cursor: u8) -> Option<Self> {
        let o = RECORD_BASE + usize::from(cursor) * RECORD_STRIDE;
        let r = block.get(o..o + RECORD_STRIDE)?;
        Some(Self {
            frame: r[0],
            effect: r[1],
            off_x: i16::from_le_bytes([r[2], r[3]]),
            off_y: i16::from_le_bytes([r[4], r[5]]),
            off_z: i16::from_le_bytes([r[6], r[7]]),
        })
    }

    /// `true` when this record ends the stream (`0x7F` or `0xFF`).
    pub fn is_terminator(self) -> bool {
        self.effect & 0x7F == TERMINATOR_EFFECT_MASKED
    }

    /// `true` when the record spawns through the direct form.
    pub fn is_direct(self) -> bool {
        self.effect & EFFECT_DIRECT_BIT != 0
    }
}

/// The two `i16` LUTs the rotation indexes, in the pairing the stepper uses.
///
/// Retail dereferences `_DAT_8007B7F8` and `_DAT_8007B81C`. The stepper indexes
/// the first at `0xFFF - a` and the second at `a`, where `a` is the biased
/// facing - the complement is what makes the pair a rotation rather than two
/// independent scalings.
pub trait RotationLut {
    /// The `_DAT_8007B7F8` table, `1 << 12` fixed point.
    fn a(&self, angle: i32) -> i32;
    /// The `_DAT_8007B81C` table, `1 << 12` fixed point.
    fn b(&self, angle: i32) -> i32;
}

/// Rotate a local-frame `(x, z)` offset into world space by an actor facing.
///
/// PORT: FUN_801DEA50 (`0x801dedd4..0x801dee7c` and the sibling block at
/// `0x801defa0..0x801df050`).
///
/// Four LUT reads, not two - the pair and its **complement** `0xFFF - a`:
///
/// ```text
/// dx = (b[a]    * z) >> 12  +  (a[~a] * x) >> 12
/// dz = (b[~a]   * x) >> 12  +  (a[a]  * z) >> 12
/// ```
///
/// Each product narrows on its own with a plain arithmetic `>> 12`, so the two
/// halves round independently - summing first and shifting once is *not*
/// equivalent.
///
/// The two blocks that use this differ only in the angle: the direct-spawn
/// branch takes `facing & 0xFFF` ([`FacingBias::None`]) and the table branch
/// takes `(facing + 0x800) & 0xFFF` ([`FacingBias::Half`]).
pub fn rotate_offset<L: RotationLut>(
    lut: &L,
    facing: u16,
    bias: FacingBias,
    x: i32,
    z: i32,
) -> (i32, i32) {
    let a = (i32::from(facing) + bias.units()) & ANGLE_MASK;
    let comp = ANGLE_MASK - a;
    let dx = ((lut.b(a) * z) >> ROT_SHIFT) + ((lut.a(comp) * x) >> ROT_SHIFT);
    let dz = ((lut.b(comp) * x) >> ROT_SHIFT) + ((lut.a(a) * z) >> ROT_SHIFT);
    (dx, dz)
}

/// Which of the stepper's two facing conventions a rotation uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacingBias {
    /// `facing & 0xFFF` - the direct-spawn branch (`0x801ded7c`).
    None,
    /// `(facing + 0x800) & 0xFFF` - the table-spawn branch (`0x801defb0`).
    Half,
}

impl FacingBias {
    /// The angle units added before masking.
    pub fn units(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Half => ANGLE_HALF,
        }
    }

    /// The bias a record's effect byte selects.
    pub fn for_effect(effect: u8) -> Self {
        if effect & EFFECT_DIRECT_BIT != 0 {
            Self::None
        } else {
            Self::Half
        }
    }
}

/// Scale a record's local offset by the actor's mesh scale.
///
/// PORT: FUN_801DEA50 (`0x801ded0c..0x801ded38` and siblings).
///
/// `scale` is the `u16` at the actor's mesh header `+0x72`. The product is
/// biased by `0xFFF` when negative before the `>> 12`, i.e. it truncates
/// toward zero rather than flooring.
pub fn scale_offset(off: i16, scale: u16) -> i32 {
    let p = i32::from(off) * i32::from(scale);
    let p = if p < 0 { p + 0xFFF } else { p };
    p >> ROT_SHIFT
}

/// Resolve the move-power table index for an acting actor.
///
/// PORT: FUN_801DEA50 (`0x801df248..0x801df284`).
///
/// The actor's queued action byte `+0x1DF` indexes the **map** at
/// `0x801F4E64` - and retail reads `map[action - 1]`, not `map[action]`
/// (`0x801df25c`: `lbu v1,-0x1(v1)` after the add). The resulting id scales by
/// [`MOVE_POWER_STRIDE`] into the record table at `0x801F4F5C`.
///
/// Returns `None` for action `0` (no queued action - the read would run off the
/// front of the map).
pub fn move_power_record_offset(map: &[u8], action: u8) -> Option<usize> {
    let idx = usize::from(action).checked_sub(1)?;
    let id = usize::from(*map.get(idx)?);
    Some(id * MOVE_POWER_STRIDE)
}

/// One effect the stepper wants spawned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectSpawn {
    /// Cursor the record sat at.
    pub cursor: u8,
    /// The record's effect selector, with [`EFFECT_DIRECT_BIT`] still set when
    /// the direct form applies.
    pub effect: u8,
    /// `true` when this goes through `FUN_801DFDF0` rather than
    /// `FUN_80050ED4`.
    pub direct: bool,
    /// World-space position the effect is spawned at.
    pub at: (i32, i32, i32),
}

/// What one call of the stepper decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectScriptStep {
    /// Cursor value the actor's `+0x1F5` should be left at.
    pub cursor: u8,
    /// Effects to spawn, in record order.
    pub spawns: Vec<EffectSpawn>,
    /// `Some(offset)` when the stream terminated this call - the byte offset of
    /// the move-power record to install at the battle context's `+0x1014`.
    pub move_power_offset: Option<usize>,
    /// `Some(band)` when the stream terminated - the target slots whose homing
    /// state the terminator seeds.
    pub homing_band: Option<TargetBand>,
}

/// Actor state the stepper reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectScriptActor {
    /// `+0x1F5` - the record cursor.
    pub cursor: u8,
    /// `+0x46` - facing, in PSX angle units.
    pub facing: u16,
    /// `+0x34`/`+0x38`/`+0x3C` - the actor's world position the offsets are
    /// applied to.
    pub world: (i32, i32, i32),
    /// The actor's mesh scale (`actor[+0x22C][+0x72]`).
    pub scale: u16,
    /// `+0x1DD` - the target-scope byte.
    pub scope: u8,
    /// `+0x1DF` - the queued action byte.
    pub action: u8,
    /// `+0x1DC` bit `0x8` - "this actor's effects are suppressed", which makes
    /// the whole call a no-op.
    pub suppressed: bool,
}

/// Walk the effect script for one frame.
///
/// PORT: FUN_801DEA50 (`0x801dec4c..0x801df53c`).
///
/// `frame` is the action's frame counter (the stepper's third argument); a
/// record whose `+0x00` gate is above `frame + 1` stops the walk for this call
/// without advancing the cursor, and a gate of `0` also stops it. Otherwise the
/// record spawns, the cursor advances, and the walk continues while the cursor
/// is below [`MAX_CURSOR`].
///
/// **A terminator does not break the loop.** Retail's terminator arm only sets
/// the local flag that enables the move-power install; the cursor still
/// advances and the walk still continues (`0x801df50c..0x801df538` is the sole
/// loop back-edge). What actually stops it one record later is the `+0x00 == 0`
/// gate on the zero-filled tail. The port keeps that structure, so a script
/// with two terminators installs twice - which is what the bytes do.
///
/// NOT WIRED: the retail caller is the battle-action SM `FUN_801E295C`, whose
/// engine port drives typed art strikes rather than an 8-byte effect-script
/// stream - `engine-core` has no `ctx[+0x1014]` move-power slot, no per-target
/// `+0x1144` homing block, and no actor `+0x1F5` cursor to advance. Wiring this
/// needs the battle action path to carry the disc effect-script block for the
/// active move (the block is reachable - `legaia_asset::move_power` already
/// parses the record the terminator installs).
pub fn step_effect_script<L: RotationLut>(
    lut: &L,
    block: &[u8],
    actor: EffectScriptActor,
    frame: u8,
    move_power_map: &[u8],
) -> EffectScriptStep {
    let mut out = EffectScriptStep {
        cursor: actor.cursor,
        spawns: Vec::new(),
        move_power_offset: None,
        homing_band: None,
    };
    if actor.suppressed || actor.cursor >= MAX_CURSOR {
        return out;
    }
    while out.cursor < MAX_CURSOR {
        let Some(rec) = EffectRecord::at(block, out.cursor) else {
            break;
        };
        // Frame gate: `frame + 1 < rec.frame` defers, `rec.frame == 0` ends.
        if i32::from(frame) + 1 < i32::from(rec.frame) || rec.frame == 0 {
            break;
        }
        if rec.is_terminator() {
            out.move_power_offset = move_power_record_offset(move_power_map, actor.action);
            out.homing_band = Some(TargetBand::from_scope(actor.scope));
        } else {
            let sx = scale_offset(rec.off_x, actor.scale);
            let sy = scale_offset(rec.off_y, actor.scale);
            let sz = scale_offset(rec.off_z, actor.scale);
            let (dx, dz) = rotate_offset(
                lut,
                actor.facing,
                FacingBias::for_effect(rec.effect),
                sx,
                sz,
            );
            out.spawns.push(EffectSpawn {
                cursor: out.cursor,
                effect: rec.effect,
                direct: rec.is_direct(),
                // Y is subtracted (`0x801ded38`: `subu v0,v0,v1`).
                at: (actor.world.0 + dx, actor.world.1 - sy, actor.world.2 + dz),
            });
        }
        out.cursor += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rotation LUT in the retail fixed point. Generated, not lifted.
    struct Lut {
        t: Vec<i16>,
    }

    impl Lut {
        fn new() -> Self {
            let one = f64::from(1 << ROT_SHIFT);
            Self {
                t: (0..4096)
                    .map(|i| {
                        let a = f64::from(i) * std::f64::consts::TAU / 4096.0;
                        (a.sin() * one).round() as i16
                    })
                    .collect(),
            }
        }
    }

    impl RotationLut for Lut {
        fn a(&self, angle: i32) -> i32 {
            i32::from(self.t[(angle & ANGLE_MASK) as usize])
        }
        fn b(&self, angle: i32) -> i32 {
            i32::from(self.t[((angle + 1024) & ANGLE_MASK) as usize])
        }
    }

    fn actor() -> EffectScriptActor {
        EffectScriptActor {
            cursor: 0,
            facing: 0,
            world: (1000, 200, 3000),
            scale: 1 << ROT_SHIFT,
            scope: 9,
            action: 5,
            suppressed: false,
        }
    }

    /// `[frame, effect, x, y, z]` per record, laid out at `RECORD_BASE`.
    fn block(records: &[(u8, u8, i16, i16, i16)]) -> Vec<u8> {
        let mut b = vec![0u8; RECORD_BASE + records.len() * RECORD_STRIDE];
        for (i, &(f, e, x, y, z)) in records.iter().enumerate() {
            let o = RECORD_BASE + i * RECORD_STRIDE;
            b[o] = f;
            b[o + 1] = e;
            b[o + 2..o + 4].copy_from_slice(&x.to_le_bytes());
            b[o + 4..o + 6].copy_from_slice(&y.to_le_bytes());
            b[o + 6..o + 8].copy_from_slice(&z.to_le_bytes());
        }
        b
    }

    #[test]
    fn scope_bytes_pick_a_row_or_a_single_slot() {
        assert_eq!(
            TargetBand::from_scope(9).slots().collect::<Vec<_>>(),
            vec![3, 4, 5, 6]
        );
        assert_eq!(
            TargetBand::from_scope(8).slots().collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        // The `0..=7` arm is a single slot, and it always runs once.
        for s in 0..8u8 {
            assert_eq!(
                TargetBand::from_scope(s).slots().collect::<Vec<_>>(),
                vec![s]
            );
        }
        // Out-of-range bytes fall through to the single-slot form.
        assert_eq!(
            TargetBand::from_scope(20).slots().collect::<Vec<_>>(),
            vec![20]
        );
    }

    #[test]
    fn a_unit_scale_passes_the_offset_through_and_truncates_toward_zero() {
        assert_eq!(scale_offset(100, 1 << ROT_SHIFT), 100);
        assert_eq!(scale_offset(-100, 1 << ROT_SHIFT), -100);
        // Half scale: 100 -> 50, -100 -> -50 (truncate, not floor).
        assert_eq!(scale_offset(100, 1 << (ROT_SHIFT - 1)), 50);
        assert_eq!(scale_offset(-100, 1 << (ROT_SHIFT - 1)), -50);
        // A fractional negative rounds toward zero, so a small offset vanishes
        // rather than becoming -1.
        assert_eq!(scale_offset(-1, 1), 0);
    }

    #[test]
    fn the_rotation_is_facing_dependent_and_preserves_length() {
        let lut = Lut::new();
        let len = |(x, z): (i32, i32)| (((x * x + z * z) as f64).sqrt()) as i32;
        let base = rotate_offset(&lut, 0, FacingBias::None, 1000, 0);
        let quarter = rotate_offset(&lut, 1024, FacingBias::None, 1000, 0);
        assert_ne!(base, quarter, "a quarter turn must move the offset");
        // The four-read pair is a rotation, so the magnitude survives (within
        // the fixed point's rounding).
        assert!(
            (len(base) - len(quarter)).abs() <= 4,
            "{base:?} {quarter:?}"
        );
        assert!((len(base) - 1000).abs() <= 4, "{base:?}");
    }

    #[test]
    fn the_half_bias_is_the_opposite_facing() {
        let lut = Lut::new();
        let none = rotate_offset(&lut, 0, FacingBias::None, 1000, 500);
        let half = rotate_offset(&lut, 0, FacingBias::Half, 1000, 500);
        // A half-revolution bias negates the rotated offset (to within the
        // fixed point's independent per-product rounding).
        assert!((none.0 + half.0).abs() <= 4, "{none:?} {half:?}");
        assert!((none.1 + half.1).abs() <= 4, "{none:?} {half:?}");
        // And the bias a record picks follows its direct bit.
        assert_eq!(FacingBias::for_effect(0x10), FacingBias::Half);
        assert_eq!(
            FacingBias::for_effect(EFFECT_DIRECT_BIT | 0x10),
            FacingBias::None
        );
    }

    #[test]
    fn move_power_index_reads_one_before_the_action() {
        let map = [7u8, 9, 11, 13];
        // action 1 -> map[0] = 7.
        assert_eq!(
            move_power_record_offset(&map, 1),
            Some(7 * MOVE_POWER_STRIDE)
        );
        // action 4 -> map[3] = 13.
        assert_eq!(
            move_power_record_offset(&map, 4),
            Some(13 * MOVE_POWER_STRIDE)
        );
        // action 0 has no record.
        assert_eq!(move_power_record_offset(&map, 0), None);
        // past the map end.
        assert_eq!(move_power_record_offset(&map, 9), None);
    }

    #[test]
    fn the_walk_spawns_then_terminates_and_installs_the_move_power() {
        let lut = Lut::new();
        let b = block(&[
            (1, 0x10, 100, 0, 0),
            (1, 0x11, 0, 0, 100),
            (1, 0xFF, 0, 0, 0),
        ]);
        let map = [0u8, 0, 0, 0, 3];
        let s = step_effect_script(&lut, &b, actor(), 4, &map);
        assert_eq!(s.spawns.len(), 2);
        // The terminator advances the cursor too, then the zero-filled tail's
        // `+0x00 == 0` gate stops the walk.
        assert_eq!(s.cursor, 3);
        assert_eq!(s.move_power_offset, Some(3 * MOVE_POWER_STRIDE));
        assert_eq!(s.homing_band, Some(TargetBand { first: 3, last: 6 }));
        assert!(!s.spawns[0].direct);
    }

    #[test]
    fn both_terminator_encodings_end_the_stream() {
        // The direct branch's `0xFF` and the table branch's `0x7F` are the
        // same marker seen through the bit-0x80 branch split.
        for e in [0x7Fu8, 0xFF] {
            let lut = Lut::new();
            let b = block(&[(1, e, 0, 0, 0)]);
            let s = step_effect_script(&lut, &b, actor(), 4, &[0u8, 0, 0, 0, 1]);
            assert!(s.spawns.is_empty(), "effect {e:#x} should not spawn");
            assert!(s.move_power_offset.is_some(), "effect {e:#x}");
        }
        // A neighbouring id is an ordinary effect.
        let lut = Lut::new();
        let b = block(&[(1, 0x7E, 0, 0, 0)]);
        let s = step_effect_script(&lut, &b, actor(), 4, &[]);
        assert_eq!(s.spawns.len(), 1);
        assert!(s.move_power_offset.is_none());
    }

    #[test]
    fn the_direct_bit_routes_to_the_other_spawn_form() {
        let lut = Lut::new();
        let b = block(&[(1, EFFECT_DIRECT_BIT | 0x13, 0, 0, 0)]);
        let s = step_effect_script(&lut, &b, actor(), 1, &[]);
        assert_eq!(s.spawns.len(), 1);
        assert!(s.spawns[0].direct);
        assert_eq!(s.spawns[0].effect, EFFECT_DIRECT_BIT | 0x13);
    }

    #[test]
    fn a_future_frame_gate_defers_without_advancing_the_cursor() {
        let lut = Lut::new();
        let b = block(&[(1, 0x10, 0, 0, 0), (9, 0x11, 0, 0, 0)]);
        let s = step_effect_script(&lut, &b, actor(), 0, &[]);
        // frame 0 + 1 == 1 clears the first gate but not the second.
        assert_eq!(s.spawns.len(), 1);
        assert_eq!(s.cursor, 1);
        assert_eq!(s.move_power_offset, None);
    }

    #[test]
    fn a_suppressed_actor_does_nothing() {
        let lut = Lut::new();
        let b = block(&[(1, 0x10, 0, 0, 0)]);
        let mut a = actor();
        a.suppressed = true;
        let s = step_effect_script(&lut, &b, a, 4, &[]);
        assert!(s.spawns.is_empty());
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn an_exhausted_cursor_does_nothing() {
        let lut = Lut::new();
        let b = block(&[(1, 0x10, 0, 0, 0)]);
        let mut a = actor();
        a.cursor = MAX_CURSOR;
        let s = step_effect_script(&lut, &b, a, 4, &[]);
        assert!(s.spawns.is_empty());
        assert_eq!(s.cursor, MAX_CURSOR);
    }

    #[test]
    fn the_y_offset_is_subtracted_not_added() {
        let lut = Lut::new();
        let b = block(&[(1, 0x10, 0, 300, 0)]);
        let a = actor();
        let s = step_effect_script(&lut, &b, a, 1, &[]);
        assert_eq!(s.spawns[0].at.1, a.world.1 - 300);
    }
}
