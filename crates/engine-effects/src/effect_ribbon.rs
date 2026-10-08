//! Battle **effect-ribbon** geometry: the random-walk quad chain the actor
//! render-mode-4 multi-target emitter builds (lightning bolts, beams, whips).
//!
//! PORT: FUN_801CFA48
//!
//! `FUN_8001ADA4` case 4 (multi-target) picks one of three primitive emitters
//! off the actor's `+0x9E` flags; the `0x2000` arm is this one, resident in the
//! battle overlay. All three share the call shape
//! `(out_buf, mode, packed, src)`: they zero the header's first three words,
//! build a chain at `out_buf + 0xC`, and take a primitive **count** from
//! `packed >> 8`. What is specific to this arm is the geometry - it is not a
//! static quad strip but a **random walk**:
//!
//! - the chain advances one step per primitive, each step turning by a wander
//!   angle and moving a randomised distance along it;
//! - each step emits **six** vertices at 8-byte stride (three lateral pairs at
//!   ±1, ±2 and ±8 times a randomised radius), so a step is two quads wide at
//!   the near end and flares at the far end;
//! - the radius **tapers** over the second half of the chain, so the ribbon
//!   thins toward its tip;
//! - a leading run of steps can be forced degenerate (zero radius), which is
//!   how a growing bolt is animated: the same buffer is rebuilt each frame with
//!   fewer suppressed steps.
//!
//! [`build_ribbon`] is the geometry half. `801CFA48` also assembles the packet
//! chain that draws the vertices - a Legaia TMD object (header at
//! `out_buf + 0xC`, vertex block, one `GT4` group of 9-word packets, colour
//! words from `src[+4]` / `src[+8]`) that the render dispatcher installs as the
//! actor's model - ported as [`ribbon_quads`] / [`ribbon_vram_mesh`].
//!
//! `see ghidra/scripts/funcs/overlay_battle_action_801cfa48.txt`. The sibling
//! dump `overlay_menu_801cfa48.txt` is only a citation pointer (its own header
//! says so) - the enclosing function there is a different one.
//!
//! Wired on both battle hosts: `World::active_effect_ribbons` walks the live
//! summon, move-FX and effect-script scenes
//! ([`crate::summon::SummonScene::ribbon_draws`] -> [`ribbon_mesh_for_actor`]),
//! and the native window's part pass (`redraw_passes.rs`,
//! `build_summon_and_move_fx_part_draws`) and the browser play page's FX
//! frame (`play_battle_fx.rs`, `build_battle_fx`) both draw that list,
//! composed like a mesh part. In play the ribbon belongs to **Gilium**'s
//! summon (spell `0x95`, PROT 0923, one node); the other carriers are Ozma
//! (`0xA0`, PROT 0934, two nodes), PROT 0957 (Death Game / Thunder Storm) and
//! PROT 0964 (Element Change and the Rogue spells), which the engine does not
//! stage as scenes yet. The other two draw-kind-4 emitters are
//! [`crate::effect_sprite_arm`] (`0x4000`) and [`crate::effect_default_arm`]
//! (the default `FUN_80028158`), drawn through the same list.
//!
//! ## Who selects this arm, and what `src` is
//!
//! Read off the disassembly (`ghidra/scripts/funcs/8001ada4.txt`
//! `0x8001B060..0x8001B124`, `ghidra/scripts/funcs/80023070.txt`):
//!
//! * The call is `(scratch, actor[+0x9E], (s16)actor[+0x9C] +
//!   (((s16)actor[+0xC8] >> 3) << 8), actor + 0x9C)`. `src` is **the actor
//!   itself** from `+0x9C`, not an effect record, so the five fields below are
//!   actor fields: `src[+0x0C]` = `actor[+0xA8]`, `src[+0x18..+0x1E]` =
//!   `actor[+0xB4..+0xBA]`, and the packet half's two colour words `src[+0x04]`
//!   / `src[+0x08]` = `actor[+0xA0]` / `actor[+0xA4]`.
//! * The writer is the **move VM's op `0x42`** (jump-table arm `0x80023F94`,
//!   fifteen halfwords): render mode `actor[+0x56] = 4`, physics byte
//!   `+0x5A = 2`, `+0x10 &= ~2`, then `+0x9E = op[1] | 0x2000` (`ori 0x2000`
//!   at `0x80023FBC`, `sh v0,0x1e(s1)` at `0x80023FC0` with
//!   `s1 = actor + 0x80` from `addiu s1,s2,0x80` at `0x80023088`),
//!   `+0x9C = op[2]`, `+0xC8 = op[3]`, `+0xB4..+0xBA = op[4..7]`,
//!   `+0xA8 = op[8]`, and the two colour words packed
//!   `op[9] + op[10]<<8 + op[11]<<16` into `+0xA0` and `op[12..14]` the same
//!   way into `+0xA4`. Op `0x23` (arm `0x800237D8`) is the `0x4000` sibling
//!   (`ori 0x4000` at `0x80023800`).
//!
//! An earlier census here reported that no store anywhere writes `0x2000` or
//! `0x4000` into `+0x9E` and that no `actor + 0x80`-relative store exists; both
//! are false - the two stores above are exactly that form, in the SCUS move VM
//! itself. Walking the disc's move programs through the engine's decoder at
//! instruction boundaries (default branch path), op `0x42` occurs five times,
//! all in slot-B cast / summon images: PROT 0923, PROT 0934 (twice), PROT 0957
//! and PROT 0964, each a transform-node record (`model_sel -1`) with a
//! seed word `0x3039` at `+0xB8` and a cap of 7..12 steps in `+0x9C`; none
//! occurs in the PROT 0898 move-FX prototypes or the field stager records.
//! So the lightning arm is shipped content. The 97 catalogued battle states
//! hold 188 render-mode-4 nodes - 128 on the default emitter, 60 on the
//! `0x4000` sprite arm, none on this one - so no capture yet shows a live
//! bolt.
//!
//! ## Growth over a node's life
//!
//! The part tick's mode-`2` channel block (`FUN_80021DF4`
//! `0x80021E78..0x80021FA0`, ported as
//! `legaia_engine_vm::move_vm::integrate_draw_channels`) steps `+0xB4..+0xBA`
//! and `+0xC8` by the rates at `+0xC0..+0xC6` / `+0xCA`, so a carrier that
//! sets a `+0xCA` rate extends its bolt over time. The shipped carriers set
//! none: `+0x9C` is the plain step cap op `0x42` stores (12 / 10 / 10 / 10 / 7
//! across the five nodes) and `+0xC8` stays `0`, so each bolt draws its whole
//! length from its first frame. An earlier note here read `+0x9C` as
//! `0x040C` (cap 12, total 4) on PROT 0923; that value was the engine's
//! summon translation glide adding its `0x400` frame step to `+0x9C`, which it
//! treated as a clock on every node - the glide now leaves draw-kind-4 nodes'
//! `+0x9C` / `+0x9E` alone.
//!
//! The `src[+0x1C]` word is the overlay RNG's **seed** - the emitter stores
//! it `>> 2` into `0x801F6950` (`lhu v0,0x1c(t8)` at `0x801CFC08`,
//! `sw v0,0x6950(v1)` at `0x801CFC18`) before the first draw - which is why
//! every shipped carrier's `0x3039` redraws one fixed bolt shape.
//!
//! REF: FUN_8001ADA4 (the render dispatcher arm that selects this emitter),
//! FUN_80028158, FUN_8002A5A4 (the other two arms), FUN_801D0290 (the RNG)

use legaia_engine_vm::battle_action::OverlayRng;

/// PSX angle units in a full revolution - the sin / cos LUT index space.
pub const ANGLE_MASK: i32 = 0xFFF;

/// The lateral offsets' narrowing shift (`sra 0xd` at `0x801CFD58`). The LUTs
/// are `1 << 12`, so this also halves the offset.
const TRIG_SHIFT: u32 = 13;

/// Fixed-point shift of the per-step advance (the walk integrates position at
/// a coarser scale than it computes the lateral offsets).
const STEP_SHIFT: u32 = 12;

/// The base heading the walk starts from (`0x801cfbe8`: `li t8,-0x400`).
pub const RIBBON_START_ANGLE: i32 = -0x400;

/// Vertices a single ribbon step emits.
pub const RIBBON_VERTS_PER_STEP: usize = 6;

/// Byte stride between two ribbon vertices in the output buffer.
pub const RIBBON_VERT_STRIDE: usize = 8;

/// Which component of the 8-byte vertex each axis lands in.
///
/// The emitter picks one of three permutations off `mode & 3`, so the same walk
/// can be laid out in the XY, XZ or YZ plane of the consumer's vertex format.
/// Retail leaves the pointers uninitialised for `mode == 3`; the port treats
/// that as [`RibbonPlane::Xy`] rather than reading garbage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RibbonPlane {
    /// `mode & 3 == 0`: walk-x at `+0`, walk-y at `+2`, zero at `+4`.
    Xy,
    /// `mode & 3 == 1`: walk-x at `+0`, walk-y at `+4`, zero at `+2`.
    Xz,
    /// `mode & 3 == 2`: walk-x at `+4`, walk-y at `+0`, zero at `+2`.
    Yz,
}

impl RibbonPlane {
    /// The plane `mode & 3` selects.
    pub fn from_mode(mode: u32) -> Self {
        match mode & 3 {
            1 => Self::Xz,
            2 => Self::Yz,
            _ => Self::Xy,
        }
    }

    /// Byte offsets of `(walk_x, walk_y, zero)` within the 8-byte vertex.
    pub fn component_offsets(self) -> (usize, usize, usize) {
        match self {
            Self::Xy => (0, 2, 4),
            Self::Xz => (0, 4, 2),
            Self::Yz => (4, 0, 2),
        }
    }
}

/// The ribbon's shape parameters, read out of the emitter's `src` struct.
///
/// Field names are by role; the offsets are the `src` reads at
/// `0x801cfaac..0x801cfc44`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RibbonParams {
    /// `src[+0x0C]` (`u16`) - spread of the per-step heading wander. Each step
    /// adds `rand % spread - spread / 2` to the wander accumulator.
    pub wander_spread: u16,
    /// `src[+0x18]` (`i16`) - base lateral radius; the emitter halves it.
    pub radius: i16,
    /// `src[+0x1A]` (`i16`) - base step length; the emitter halves it and adds
    /// `rand % half` per step.
    pub step_len: i16,
    /// `src[+0x1C]` (`i16`) - the overlay RNG seed: the emitter stores it
    /// `>> 2` into `0x801F6950`, the state of `FUN_801D0290`, before the walk.
    pub rng_seed: i16,
    /// `src[+0x1E]` (`i16`) - constant heading advance per step, on top of the
    /// wander.
    pub turn_rate: i16,
}

impl RibbonParams {
    /// Read the five shape fields off a move-VM actor, from the `src =
    /// actor + 0x9C` view the render dispatcher hands the emitter
    /// (`addiu a3,s1,0x1c` at `0x8001B104`, `s1 = actor + 0x80`):
    /// `src[+0x0C..]` is `actor[+0xA8]`, `src[+0x18..+0x1E]` is
    /// `actor[+0xB4..+0xBA]`.
    ///
    /// Move-VM op `0x42` is the writer of every one of them
    /// (`legaia_engine_vm::move_vm`, arm `0x80023F94`), so an actor that has
    /// run that op reads back the carrier's own parameters here.
    pub fn from_actor(s: &legaia_engine_vm::move_vm::ActorState) -> Self {
        Self {
            wander_spread: s.actor_u16(0xA8),
            radius: s.actor_u16(0xB4) as i16,
            step_len: s.actor_u16(0xB6) as i16,
            rng_seed: s.actor_u16(0xB8) as i16,
            turn_rate: s.actor_u16(0xBA) as i16,
        }
    }
}

/// The emitter's `mode` and packed-count arguments as the render dispatcher
/// builds them off an actor (`0x8001B104..0x8001B124`): `mode` is
/// `actor[+0x9E]` (the emitter reads only its low two bits), and `packed` is
/// `(s16)actor[+0x9C] + (((s16)actor[+0xC8] >> 3) << 8)` - the `sll 0x10` /
/// `sra 0x13` pair is a sign-extending `>> 3` of the halfword.
///
/// Returns `None` unless the actor is a draw-kind-4 node on this arm
/// (`+0x56 == 4` and `+0x9E & 0x2000`), which is exactly the state op `0x42`
/// leaves.
pub fn ribbon_call_args(s: &legaia_engine_vm::move_vm::ActorState) -> Option<(u32, u32)> {
    if s.move_substate != 4 || s.field_9e & 0x2000 == 0 {
        return None;
    }
    let count = i32::from(s.actor_u16(0x9C) as i16);
    let total = i32::from(s.actor_u16(0xC8) as i16) >> 3;
    Some((u32::from(s.field_9e), count.wrapping_add(total << 8) as u32))
}

/// The two packed colour words the emitter's packet half reads at `src[+0x04]`
/// / `src[+0x08]` - `actor[+0xA0]` / `actor[+0xA4]`, which op `0x42` builds
/// from its last six operands.
pub fn ribbon_colour_words(s: &legaia_engine_vm::move_vm::ActorState) -> (u32, u32) {
    (s.actor_u32(0xA0), s.actor_u32(0xA4))
}

/// One emitted ribbon step: six vertices in `(walk_x, walk_y)` pairs, in the
/// order the emitter stores them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RibbonStep {
    /// The six `(x, y)` pairs at `+0x00`, `+0x08`, `+0x10`, `+0x18`, `+0x20`
    /// and `+0x28` of the step's `0x30`-byte block. The third component is
    /// always zero.
    pub verts: [(i16, i16); RIBBON_VERTS_PER_STEP],
}

/// The header words the emitter writes at `out_buf + 0xC`, and the packet-chain
/// geometry it derives from them.
///
/// This is the render-track half, kept as data so the layout is checkable
/// without a GPU: `vertex_base` / `packet_base` are byte offsets from
/// `out_buf`, not pointers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RibbonPackets {
    /// `header[+0x00]` - byte offset of the vertex block (`out_buf + 0x28`).
    pub vertex_base: usize,
    /// `header[+0x04]` - `(steps + 2) * 6`, the allocated vertex count (two
    /// steps of slack past the emitted chain).
    pub allocated_verts: usize,
    /// `header[+0x14]` - `steps * 6`, the emitted vertex count. Also the `u16`
    /// the packet header's first word carries.
    pub emitted_verts: usize,
    /// `header[+0x10]` - byte offset of the packet block
    /// (`vertex_base + allocated_verts * 8`).
    pub packet_base: usize,
    /// Bytes of packet header before the per-quad words
    /// (`0x801d0000..0x801d0034`).
    pub packet_header_bytes: usize,
    /// Bytes per emitted quad (`0x801d00c8`: `addiu t3,t3,0x24`).
    pub packet_stride: usize,
    /// Quads emitted per step (six `0x24`-byte packets per iteration of the
    /// packet loop).
    pub packets_per_step: usize,
    /// The GPU command base the colour words carry (`0x3C000000` - a Gouraud
    /// textured quad).
    pub command_base: u32,
}

/// GPU command base the emitter ORs into every colour word.
pub const RIBBON_COMMAND_BASE: u32 = 0x3C00_0000;

/// The whole emitter result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ribbon {
    /// Steps actually walked. `steps.len()` is `emitted + 1` - the emitter's
    /// loop bound is `count + 1`, so it writes one step past the count the
    /// packet half draws.
    pub steps: Vec<RibbonStep>,
    /// Vertex-component permutation.
    pub plane: RibbonPlane,
    /// Header + packet-chain layout.
    pub packets: RibbonPackets,
    /// The value the emitter stores to `0x801F6950` - the RNG state it
    /// then draws from (`rng_seed >> 2`).
    pub rng_seed: i32,
}

/// Split the emitter's packed count argument.
///
/// PORT: FUN_801CFA48 (`0x801cfa5c..0x801cfafc`).
///
/// `packed & 0xFF` is the per-batch cap and `packed >> 8` the total. When the
/// total is non-zero the emitter clamps the batch to it and keeps the surplus
/// as a **suppression run**: the first `remainder` steps are emitted with zero
/// radius. A zero total leaves the cap untouched and the remainder at zero,
/// which is the "draw the whole ribbon" form.
///
/// Reached from both battle hosts through [`build_ribbon`] (module doc).
/// Split out as its own function because the cap/total packing is the part a
/// caller has to construct.
pub fn split_packed_count(packed: u32) -> (i32, i32) {
    let cap = (packed & 0xFF) as i32;
    let total = (packed >> 8) as i32;
    if total == 0 {
        return (cap, 0);
    }
    if cap < total {
        (cap, total - cap)
    } else {
        (total, 0)
    }
}

/// Sin / cos source the walk integrates against.
///
/// Retail reads two `i16` LUTs through the pointers `_DAT_8007B7F8` and
/// `_DAT_8007B81C`, both indexed by a 12-bit angle. The emitter always feeds
/// the **first** table into the walk's X component and the **second** into its
/// Y component (`0x801cfd0c`/`0x801cfd28` for the lateral offsets,
/// `0x801cff8c`/`0x801cffa8` for the advance), so that pairing - not the
/// sin-versus-cos naming - is what the port depends on. `sin` here is the
/// `_DAT_8007B7F8` table and `cos` the `_DAT_8007B81C` one, following the
/// naming the subsystem docs already use - although `_DAT_8007B7F8` is in fact
/// the cosine view and `_DAT_8007B81C` the sine ([`RetailTrig`]).
pub trait TrigTable {
    /// The `_DAT_8007B7F8` table (walk X) in `1 << 12` fixed point, angle
    /// masked to 12 bits.
    fn sin(&self, angle: i32) -> i32;
    /// The `_DAT_8007B81C` table (walk Y) in `1 << 12` fixed point, angle
    /// masked to 12 bits.
    fn cos(&self, angle: i32) -> i32;
}

/// A `4096`-entry analytic table in the retail LUTs' fixed point. Not a lift of
/// the disc tables - it is generated, and exists so the walk is testable
/// without disc data.
#[derive(Debug, Clone)]
pub struct AnalyticTrig {
    sin: Vec<i16>,
}

impl Default for AnalyticTrig {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyticTrig {
    /// Build the table.
    pub fn new() -> Self {
        let one = f64::from(1 << STEP_SHIFT);
        let sin = (0..4096)
            .map(|i| {
                let a = f64::from(i) * std::f64::consts::TAU / 4096.0;
                (a.sin() * one).round() as i16
            })
            .collect();
        Self { sin }
    }
}

impl TrigTable for AnalyticTrig {
    fn sin(&self, angle: i32) -> i32 {
        i32::from(self.sin[(angle & ANGLE_MASK) as usize])
    }
    fn cos(&self, angle: i32) -> i32 {
        i32::from(self.sin[((angle + 1024) & ANGLE_MASK) as usize])
    }
}

/// Narrowing the emitter uses on the **lateral** trig products:
/// `(v + (v >>> 31)) >> 13` - add the sign bit back before the arithmetic shift
/// so the result truncates toward zero.
fn narrow_lateral(v: i64) -> i32 {
    let v = v as i32;
    (v.wrapping_add(((v as u32) >> 31) as i32)) >> TRIG_SHIFT
}

/// Narrowing the emitter uses on the **advance** trig products: a plain
/// arithmetic `>> 12`, with no sign-bit correction, so a negative advance
/// floors instead of truncating. The asymmetry with [`narrow_lateral`] is in
/// the bytes (`0x801cfd58` adds the sign bit, `0x801cffd0` does not) and it
/// biases a leftward walk by one unit per step.
fn narrow_advance(v: i64) -> i32 {
    (v as i32) >> STEP_SHIFT
}

/// Build a ribbon.
///
/// PORT: FUN_801CFA48 (`0x801cfa48..0x801d028c`).
///
/// `mode` is the emitter's second argument (only its low two bits are read),
/// `packed` the third, `params` the fields it reads out of the fourth, and
/// `rand` a source of the RNG values `FUN_801D0290` supplies - the emitter
/// calls it **four** times per step, in this order:
///
/// 1. the near lateral radius `radius/2 + r % radius`,
/// 2. the mid lateral radius `radius + r % radius`,
/// 3. the wander roll: `r & 7 == 0` damps the accumulator, otherwise a fifth
///    call adds `r % spread - spread/2`,
/// 4. the step length `step/2 + r % (step/2)`.
///
/// The damping arm is asymmetric in the bytes and the port keeps it that way:
/// a negative accumulator is folded twice (magnitude `/16`, sign preserved),
/// a positive one once (magnitude `/4`, sign flipped). See [`damp_wander`].
///
/// The RNG modulus is guarded at `1`; retail divides by the raw radius / step
/// and would trap on a zero one, which the emitter is never handed.
///
/// The emitter's only retail caller is the render dispatcher `FUN_8001ADA4`
/// case 4 on an actor move-VM op `0x42` set up; the engine's caller is
/// [`ribbon_mesh_for_actor`], which both battle hosts reach (module doc). The
/// emitter is pure and takes its RNG and LUTs as parameters.
pub fn build_ribbon<T: TrigTable, R: FnMut() -> u32>(
    mode: u32,
    packed: u32,
    params: RibbonParams,
    trig: &T,
    mut rand: R,
) -> Ribbon {
    let (count, suppressed) = split_packed_count(packed);
    let plane = RibbonPlane::from_mode(mode);
    let half = (count >> 1) + 1;
    let base_radius = i32::from(params.radius) >> 1;
    let base_step = i32::from(params.step_len) >> 1;
    let spread = i32::from(params.wander_spread);

    let mut angle = RIBBON_START_ANGLE;
    let mut wander = 0i32;
    let mut walk_x = 0i32;
    let mut walk_y = 0i32;
    let mut steps: Vec<RibbonStep> = Vec::new();

    // Retail's loop bound is `i < count + 1`, so a `count` of 0 still writes
    // one step and a negative count writes none.
    for i in 0..(count + 1).max(0) {
        // Radius for this step: 1 at the head, tapering over the second half.
        let radius = if half < i {
            let scaled = base_radius * (half - (i - half));
            let r = if half != 0 { scaled / half } else { scaled };
            if r > 0 { r } else { 1 }
        } else if i == 0 {
            1
        } else {
            base_radius
        };
        let suppress = i < suppressed;
        let modulus = radius.max(1);

        // Pair 1 + 2: near lateral offset at +-1r and +-2r.
        let r1 = (radius >> 1) + (rand() % modulus as u32) as i32;
        let (dx1, dy1) = if suppress {
            (0, 0)
        } else {
            (
                narrow_lateral(i64::from(trig.sin(angle)) * i64::from(r1)),
                narrow_lateral(i64::from(trig.cos(angle)) * i64::from(r1)),
            )
        };
        // Pair 3: far lateral offset at +-8r.
        let r2 = radius + (rand() % modulus as u32) as i32;
        let (dx2, dy2) = if suppress {
            (0, 0)
        } else {
            (
                narrow_lateral(i64::from(trig.sin(angle)) * i64::from(r2)),
                narrow_lateral(i64::from(trig.cos(angle)) * i64::from(r2)),
            )
        };
        let w = |v: i32| v as i16;
        steps.push(RibbonStep {
            verts: [
                (w(walk_x - dx1), w(walk_y - dy1)),
                (w(walk_x + dx1), w(walk_y + dy1)),
                (w(walk_x - dx1 * 2), w(walk_y - dy1 * 2)),
                (w(walk_x + dx1 * 2), w(walk_y + dy1 * 2)),
                (w(walk_x - dx2 * 8), w(walk_y - dy2 * 8)),
                (w(walk_x + dx2 * 8), w(walk_y + dy2 * 8)),
            ],
        });

        // Wander roll.
        if rand() & 7 == 0 {
            wander = damp_wander(wander);
        } else if spread > 0 {
            wander += (rand() % spread as u32) as i32 - (spread >> 1);
        } else {
            // A zero spread would divide by zero in retail; the emitter is
            // only ever handed a non-zero one. Consume the roll so the RNG
            // stream stays aligned.
            let _ = rand();
        }
        angle += i32::from(params.turn_rate);

        // Advance the walk.
        let step_mod = base_step.max(1);
        let len = base_step + (rand() % step_mod as u32) as i32;
        let heading = (angle + wander) & ANGLE_MASK;
        walk_x += narrow_advance(i64::from(trig.sin(heading)) * i64::from(len));
        walk_y += narrow_advance(i64::from(trig.cos(heading)) * i64::from(len));
    }

    let emitted = (count.max(0) as usize) * RIBBON_VERTS_PER_STEP;
    let allocated = (count.max(0) as usize + 2) * RIBBON_VERTS_PER_STEP;
    Ribbon {
        steps,
        plane,
        packets: RibbonPackets {
            vertex_base: 0x28,
            allocated_verts: allocated,
            emitted_verts: emitted,
            packet_base: 0x28 + allocated * RIBBON_VERT_STRIDE,
            packet_header_bytes: 8,
            packet_stride: 0x24,
            packets_per_step: 6,
            command_base: RIBBON_COMMAND_BASE,
        },
        rng_seed: i32::from(params.rng_seed) >> 2,
    }
}

/// The wander accumulator's damping fold - the arm taken when the third RNG
/// roll of a step comes up `r & 7 == 0`.
///
/// PORT: FUN_801CFA48 (`0x801cfeec..0x801cff44`).
///
/// The bytes are asymmetric and the asymmetry is real, not a decompiler
/// artifact: the negative arm falls **through** into the positive arm, so it
/// applies both folds.
///
/// - `w < 0`: `w = -(w >> 2)` makes it positive, then the positive arm runs on
///   the result, so the net effect is magnitude `/16` with the sign preserved.
/// - `w > 0`: `w = -(w >> 2)` - magnitude `/4`, sign flipped.
/// - `w == 0`: unchanged.
///
/// Both shifts are arithmetic (floor, not truncate), so both arms collapse
/// small magnitudes to exactly `0`: `1..=3` from above and `-12..=-1` from
/// below. The negative arm's branch that would skip the second fold
/// (`bgez v0` at `0x801cfef4`) can never be taken - `w >> 2` of a negative `w`
/// is at most `-1` - so the fall-through really is unconditional.
///
/// Reached through [`build_ribbon`] (module doc). Exposed rather than inlined
/// because the asymmetric fold is the emitter's least obvious behaviour and is
/// worth being separately testable.
pub fn damp_wander(w: i32) -> i32 {
    let mut w = w;
    if w < 0 {
        w = -(w >> 2);
    }
    if w > 0 {
        w = -(w >> 2);
    }
    w
}

/// The retail sin / cos LUT pair behind `_DAT_8007B7F8` / `_DAT_8007B81C`,
/// generated from its definition ([`crate::action_effect_script::retail_rotation_lut`]).
///
/// The emitter reads the `_DAT_8007B7F8` pointer (`lw v1,-0x4808(t8)` at
/// `0x801CFD0C`, `t8 = 0x80080000`) into walk X and the `_DAT_8007B81C` one
/// (`lw v0,-0x47e4(t8)` at `0x801CFD28`) into walk Y. `_DAT_8007B7F8` is the
/// table a quarter turn on (cosine) and `_DAT_8007B81C` the sine
/// (`FUN_80026BE0`), both in `1 << 12` fixed point - so the lateral `>> 13`
/// narrowing halves the offset, and the advance's `>> 12` does not.
#[derive(Debug, Clone, Copy, Default)]
pub struct RetailTrig;

impl TrigTable for RetailTrig {
    fn sin(&self, angle: i32) -> i32 {
        use crate::action_effect_script::RotationLut;
        crate::action_effect_script::retail_rotation_lut().a(angle)
    }
    fn cos(&self, angle: i32) -> i32 {
        use crate::action_effect_script::RotationLut;
        crate::action_effect_script::retail_rotation_lut().b(angle)
    }
}

/// Texture page word every ribbon packet carries (`0x801D005C`: `0x001F`) -
/// page `(960, 256)`, 4bpp, no semi-transparency bit.
pub const RIBBON_TPAGE: u16 = 0x001F;

/// CLUT word every ribbon packet carries (`0x801D003C`: `0x7F84`) - CLUT at
/// `(64, 510)`.
pub const RIBBON_CLUT: u16 = 0x7F84;

/// The four corner UVs every ribbon packet carries, in packet order: a 2x2
/// texel patch at `(0..2, 0xF0..0xF2)` of [`RIBBON_TPAGE`]
/// (`0x801D0038..0x801D0074`).
pub const RIBBON_UVS: [[u8; 2]; 4] = [[0, 0xF0], [2, 0xF0], [0, 0xF2], [2, 0xF2]];

/// The group header's `flags` halfword (`0x801D0010`: `0x26`) - row 5 of the
/// per-mode table, the quad half: a baked-colour Gouraud textured quad (`GT4`).
pub const RIBBON_GROUP_FLAGS: u16 = 0x26;

/// Which colour word a packet corner carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RibbonShade {
    /// `src[+0x04]` (`actor[+0xA0]`) - the core colour.
    Core,
    /// `src[+0x08]` (`actor[+0xA4]`) - the flare colour.
    Flare,
    /// The bare command word `0x3C000000` - black, the fringe.
    Black,
}

/// One packet of the chain: four vertex indices into [`Ribbon::steps`]'
/// flattened vertex list (step `i`'s vertex `k` is `i * 6 + k`) and the
/// corner shades, in GPU corner order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RibbonQuad {
    /// `[v0, v1, v2, v3]`: packet words `+0x1C` (low / high) then `+0x20`.
    pub verts: [usize; 4],
    /// Corner colours, packet words `+0x00..+0x0C`.
    pub shades: [RibbonShade; 4],
}

/// The packet half of `FUN_801CFA48` (`0x801CFFF4..0x801D025C`): the six
/// `GT4` packets each step contributes, joining step `i`'s six vertices to
/// step `i + 1`'s.
///
/// PORT: FUN_801CFA48 (`0x801CFFF4..0x801D025C`, the packet loop)
///
/// Per step, with `b = 6 * i` and the vertex order [`build_ribbon`] emits
/// (`0/1` = the ±1 pair, `2/3` = ±2, `4/5` = ±8):
///
/// | packet | vertices | shades |
/// |---|---|---|
/// | 1, 2 | `b, b+1, b+6, b+7` | core ×4 |
/// | 3 | `b+2, b, b+8, b+6` | flare, core, flare, core |
/// | 4 | `b+1, b+3, b+7, b+9` | core, flare, core, flare |
/// | 5 | `b+4, b+2, b+10, b+8` | black, flare, black, flare |
/// | 6 | `b+3, b+5, b+9, b+11` | flare, black, flare, black |
///
/// The first packet is written **twice** - the loop body stores the same
/// words at `t3` and `t3 + 0x24` (`0x801D00A4..0x801D00F4`) - so the core
/// strip draws twice; the port keeps both. The group header is
/// `count = steps * 6`, `flags = 0x26`, `ilen = 9`, `mode = 0x3C`
/// (`0x801D000C..0x801D0024`), and the chain is closed by twenty zero words
/// (`0x801D0248..0x801D025C`).
pub fn ribbon_quads(ribbon: &Ribbon) -> Vec<RibbonQuad> {
    use RibbonShade::{Black as K, Core as A, Flare as B};
    let count = ribbon.packets.emitted_verts / RIBBON_VERTS_PER_STEP;
    let mut out = Vec::with_capacity(count * ribbon.packets.packets_per_step);
    for i in 0..count {
        let b = i * RIBBON_VERTS_PER_STEP;
        let core = RibbonQuad {
            verts: [b, b + 1, b + 6, b + 7],
            shades: [A, A, A, A],
        };
        out.push(core);
        out.push(core);
        out.push(RibbonQuad {
            verts: [b + 2, b, b + 8, b + 6],
            shades: [B, A, B, A],
        });
        out.push(RibbonQuad {
            verts: [b + 1, b + 3, b + 7, b + 9],
            shades: [A, B, A, B],
        });
        out.push(RibbonQuad {
            verts: [b + 4, b + 2, b + 10, b + 8],
            shades: [K, B, K, B],
        });
        out.push(RibbonQuad {
            verts: [b + 3, b + 5, b + 9, b + 11],
            shades: [B, K, B, K],
        });
    }
    out
}

/// The ribbon as a local-space VRAM mesh - what the TMD renderer draws off the
/// object header the emitter builds at `out_buf + 0xC` (`FUN_8001ADA4` stores
/// that header into every slot of the actor's `+0x44` model list,
/// `0x8001B08C..0x8001B0B4`, so the ribbon is drawn as the actor's own model).
///
/// Positions are the step vertices laid out by [`RibbonPlane`]; each packet
/// becomes four vertices (`[v0, v1, v2]` + `[v2, v1, v3]`), textured from the
/// fixed 2x2 patch ([`RIBBON_UVS`] / [`RIBBON_CLUT`] / [`RIBBON_TPAGE`]) and
/// modulated by the corner shade, `core` / `flare` being the two colour words'
/// low 24 bits (`and` with `0x00FFFFFF` at `0x801CFABC` / `0x801CFAC8`).
///
/// PORT: FUN_801CFA48 (the object header + packet words it hands the renderer)
pub fn ribbon_vram_mesh(ribbon: &Ribbon, core: u32, flare: u32) -> legaia_tmd::mesh::VramMesh {
    let rgb = |w: u32| {
        [
            (w & 0xFF) as u8,
            ((w >> 8) & 0xFF) as u8,
            ((w >> 16) & 0xFF) as u8,
        ]
    };
    let (ox, oy, _) = ribbon.plane.component_offsets();
    let vertex = |idx: usize| -> [f32; 3] {
        let step = &ribbon.steps[idx / RIBBON_VERTS_PER_STEP];
        let (wx, wy) = step.verts[idx % RIBBON_VERTS_PER_STEP];
        let mut v = [0.0f32; 3];
        v[ox / 2] = f32::from(wx);
        v[oy / 2] = f32::from(wy);
        v
    };
    let mut mesh = legaia_tmd::mesh::VramMesh {
        positions: Vec::new(),
        uvs: Vec::new(),
        cba_tsb: Vec::new(),
        indices: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
    };
    for q in ribbon_quads(ribbon) {
        if q.verts
            .iter()
            .any(|&v| v / RIBBON_VERTS_PER_STEP >= ribbon.steps.len())
        {
            continue;
        }
        let base = mesh.positions.len() as u32;
        for (corner, &vi) in q.verts.iter().enumerate() {
            mesh.positions.push(vertex(vi));
            mesh.uvs.push(RIBBON_UVS[corner]);
            mesh.cba_tsb.push([RIBBON_CLUT, RIBBON_TPAGE]);
            mesh.normals.push([0.0; 3]);
            mesh.colors.push(match q.shades[corner] {
                RibbonShade::Core => rgb(core),
                RibbonShade::Flare => rgb(flare),
                RibbonShade::Black => [0, 0, 0],
            });
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    }
    mesh
}

/// The whole per-frame ribbon a live move-VM node draws, or `None` when the
/// node is not on the ribbon arm - [`ribbon_call_args`] +
/// [`RibbonParams::from_actor`] + [`build_ribbon`] (retail RNG
/// [`legaia_engine_vm::battle_action::OverlayRng`], reseeded from `+0xB8` on
/// every call, and the retail LUTs) + [`ribbon_vram_mesh`].
///
/// A pure function of the node's state, exactly as retail's emitter is: it
/// rebuilds the buffer every draw from the same seed.
pub fn ribbon_mesh_for_actor(
    s: &legaia_engine_vm::move_vm::ActorState,
) -> Option<legaia_tmd::mesh::VramMesh> {
    let (mode, packed) = ribbon_call_args(s)?;
    let params = RibbonParams::from_actor(s);
    // `lhu` / `sll 0x10` / `sra 0x12` / `sw` at `0x801CFC08..0x801CFC18`: the
    // seed halfword sign-extended and shifted into the RNG state word.
    let mut rng = OverlayRng::new((i32::from(params.rng_seed) >> 2) as u32);
    let ribbon = build_ribbon(mode, packed, params, &RetailTrig, || rng.draw());
    let (core, flare) = ribbon_colour_words(s);
    Some(ribbon_vram_mesh(&ribbon, core, flare))
}

/// One live ribbon, ready to draw: the local-space mesh and the node's
/// transform, in the same `(world_pos, rot)` form as
/// [`crate::summon::SummonPartDraw`] so a host composes it exactly like a
/// mesh part (`T * Ry * Rx * Rz * flip`).
#[derive(Debug, Clone)]
pub struct RibbonDraw {
    /// [`ribbon_vram_mesh`] of the node's current state.
    pub mesh: legaia_tmd::mesh::VramMesh,
    /// World position (move-VM `world_x/y/z`).
    pub world_pos: [f32; 3],
    /// Euler XYZ rotation in radians (from the move-VM rotation banks).
    pub rot: [f32; 3],
    /// The node's `+0x52` word (move-VM op `0x15`). Its `0x780` bits select
    /// retail's camera-relative rotation `FUN_8001CF50` at the render
    /// dispatcher `FUN_8001ADA4` (`0x8001B374`); a host resolves them with
    /// `legaia_engine_ui::gte::camera_relative_model_prefix`. `0` for draws
    /// that are not move-VM nodes.
    pub flags_52: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> RibbonParams {
        RibbonParams {
            wander_spread: 0x40,
            radius: 0x80,
            step_len: 0x200,
            rng_seed: 0x40,
            turn_rate: 0x20,
        }
    }

    /// Deterministic stand-in for `FUN_801D0290`.
    fn lcg() -> impl FnMut() -> u32 {
        let mut s: u32 = 0x1234_5678;
        move || {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            s >> 8
        }
    }

    #[test]
    fn packed_count_splits_into_batch_and_suppression_run() {
        // total 0: the cap passes through, nothing suppressed.
        assert_eq!(split_packed_count(0x0000_000A), (10, 0));
        // cap >= total: the batch is the total.
        assert_eq!(split_packed_count(0x0000_050A), (5, 0));
        // cap < total: surplus becomes the suppression run.
        assert_eq!(split_packed_count(0x0000_0A05), (5, 5));
    }

    #[test]
    fn plane_permutations_are_distinct_and_cover_three_components() {
        for mode in 0..4u32 {
            let (a, b, c) = RibbonPlane::from_mode(mode).component_offsets();
            let mut v = [a, b, c];
            v.sort_unstable();
            assert_eq!(v, [0, 2, 4], "mode {mode} must cover +0/+2/+4 exactly once");
        }
        assert_eq!(RibbonPlane::from_mode(0), RibbonPlane::Xy);
        assert_eq!(RibbonPlane::from_mode(1), RibbonPlane::Xz);
        assert_eq!(RibbonPlane::from_mode(2), RibbonPlane::Yz);
        // Retail leaves mode 3 uninitialised; the port pins it to the mode-0
        // layout rather than reading stale pointers.
        assert_eq!(RibbonPlane::from_mode(3), RibbonPlane::Xy);
    }

    #[test]
    fn damping_preserves_a_negative_sign_and_flips_a_positive_one() {
        // -100 -> -(-100>>2) = 25 -> -(25>>2) = -6: sign kept, /16.
        assert_eq!(damp_wander(-100), -6);
        // 100 -> -(100>>2) = -25: sign flipped, /4.
        assert_eq!(damp_wander(100), -25);
        assert_eq!(damp_wander(0), 0);
        // Both arms floor toward zero, so small magnitudes damp out entirely:
        // 1..=3 from above and -12..=-1 from below.
        for w in [1, 2, 3] {
            assert_eq!(damp_wander(w), 0, "w = {w}");
        }
        for w in -12..0 {
            assert_eq!(damp_wander(w), 0, "w = {w}");
        }
        // The first magnitude that survives on each side.
        assert_eq!(damp_wander(-13), -1);
        assert_eq!(damp_wander(4), -1);
        // Sign is preserved from below and flipped from above at every
        // surviving magnitude.
        for w in [-13, -16, -100, -4096] {
            assert!(damp_wander(w) < 0, "w = {w}");
        }
        for w in [4, 16, 100, 4096] {
            assert!(damp_wander(w) < 0, "w = {w}");
        }
        // Magnitude ratios: /16 from below, /4 from above.
        assert_eq!(damp_wander(-4096), -256);
        assert_eq!(damp_wander(4096), -1024);
    }

    #[test]
    fn header_layout_allocates_two_steps_of_slack() {
        let r = build_ribbon(0, 0x0000_0008, params(), &AnalyticTrig::new(), lcg());
        assert_eq!(r.packets.emitted_verts, 8 * RIBBON_VERTS_PER_STEP);
        assert_eq!(r.packets.allocated_verts, 10 * RIBBON_VERTS_PER_STEP);
        assert_eq!(
            r.packets.packet_base,
            r.packets.vertex_base + r.packets.allocated_verts * RIBBON_VERT_STRIDE
        );
        // The emitter walks one step past the drawn count.
        assert_eq!(r.steps.len(), 9);
        assert_eq!(r.rng_seed, 0x40 >> 2);
    }

    #[test]
    fn a_suppression_run_leaves_the_leading_steps_degenerate() {
        // cap 3, total 8 -> batch 3, suppression run 5. Retail then walks
        // `batch + 1` steps, so every emitted step is inside the run.
        let r = build_ribbon(0, 0x0000_0803, params(), &AnalyticTrig::new(), lcg());
        for (i, s) in r.steps.iter().enumerate() {
            let head = s.verts[0];
            let all_same = s.verts.iter().all(|&v| v == head);
            assert!(all_same, "step {i} should be degenerate: {:?}", s.verts);
        }
    }

    #[test]
    fn an_unsuppressed_ribbon_flares_and_the_head_is_thin() {
        let r = build_ribbon(0, 0x0000_0010, params(), &AnalyticTrig::new(), lcg());
        let spread = |s: &RibbonStep| {
            let xs: Vec<i32> = s.verts.iter().map(|v| i32::from(v.0)).collect();
            let ys: Vec<i32> = s.verts.iter().map(|v| i32::from(v.1)).collect();
            (xs.iter().max().unwrap() - xs.iter().min().unwrap())
                + (ys.iter().max().unwrap() - ys.iter().min().unwrap())
        };
        // Step 0 forces radius 1, so it is much thinner than a mid step.
        assert!(
            spread(&r.steps[0]) < spread(&r.steps[4]),
            "head {} should be thinner than mid {}",
            spread(&r.steps[0]),
            spread(&r.steps[4])
        );
        // The walk actually moves.
        let head = r.steps[0].verts[0];
        let tail = r.steps.last().unwrap().verts[0];
        assert_ne!(head, tail);
    }

    #[test]
    fn op_42_on_the_move_vm_feeds_the_emitter_its_own_operands() {
        use legaia_engine_vm::move_vm::{ActorState, MoveHost, StepResult, step};
        struct NoHost;
        impl MoveHost for NoHost {
            fn rotation_lut(&self, _: u16) -> (i16, i16) {
                (0, 0)
            }
        }
        // The shape every shipped carrier has: a `0x3039` seed, a step cap in
        // `+0x9C`, no total in `+0xC8`.
        let program: Vec<u16> = vec![
            0x42, 0x0001, 9, 0, 0x40, 0x200, 0x3039, 0x20, 0x40, 0x80, 0x80, 0xFF, 0x10, 0x10,
            0x40, 0x08,
        ];
        let mut s = ActorState::new();
        assert_eq!(step(&mut NoHost, &mut s, &program), StepResult::Advance);
        let (mode, packed) = ribbon_call_args(&s).expect("op 0x42 arms the ribbon arm");
        assert_eq!(mode & 3, 1);
        assert_eq!(split_packed_count(packed), (9, 0));
        let p = RibbonParams::from_actor(&s);
        assert_eq!(
            p,
            RibbonParams {
                wander_spread: 0x40,
                radius: 0x40,
                step_len: 0x200,
                rng_seed: 0x3039,
                turn_rate: 0x20,
            }
        );
        assert_eq!(ribbon_colour_words(&s), (0x00FF_8080, 0x0040_1010));
        let r = build_ribbon(mode, packed, p, &AnalyticTrig::new(), lcg());
        assert_eq!(r.steps.len(), 10);
        assert_eq!(r.rng_seed, 0x3039 >> 2);
        assert_eq!(r.plane, RibbonPlane::Xz);
        // A node that has not run op 0x42 is not on this arm.
        assert_eq!(ribbon_call_args(&ActorState::new()), None);
    }

    /// The packet loop's per-step table (`0x801D0080..0x801D0230`), including
    /// the duplicated core packet.
    #[test]
    fn each_step_emits_the_six_retail_packets() {
        use RibbonShade::{Black as K, Core as A, Flare as B};
        let r = build_ribbon(0, 2, params(), &RetailTrig, lcg());
        let q = ribbon_quads(&r);
        assert_eq!(q.len(), 12);
        let b = 6;
        assert_eq!(q[6].verts, [b, b + 1, b + 6, b + 7]);
        assert_eq!(q[6], q[7], "the core packet is stored twice");
        assert_eq!(q[8].verts, [b + 2, b, b + 8, b + 6]);
        assert_eq!(q[8].shades, [B, A, B, A]);
        assert_eq!(q[9].verts, [b + 1, b + 3, b + 7, b + 9]);
        assert_eq!(q[10].verts, [b + 4, b + 2, b + 10, b + 8]);
        assert_eq!(q[10].shades, [K, B, K, B]);
        assert_eq!(q[11].verts, [b + 3, b + 5, b + 9, b + 11]);
        assert_eq!(q[11].shades, [B, K, B, K]);
    }

    /// The mesh lays the walk into the `mode & 3` plane and carries the fixed
    /// patch and the two colour words' RGB.
    #[test]
    fn the_mesh_uses_the_plane_patch_and_colour_words() {
        let r = build_ribbon(1, 3, params(), &RetailTrig, lcg());
        let m = ribbon_vram_mesh(&r, 0x3C11_2233, 0x3C44_5566);
        assert_eq!(m.positions.len(), 3 * 6 * 4);
        assert!(
            m.positions.iter().all(|p| p[1] == 0.0),
            "XZ plane: Y is zero"
        );
        assert_eq!(m.uvs[..4], RIBBON_UVS);
        assert_eq!(m.cba_tsb[0], [RIBBON_CLUT, RIBBON_TPAGE]);
        assert_eq!(
            m.colors[0],
            [0x33, 0x22, 0x11],
            "core, command byte dropped"
        );
        // Packet 5's first corner is black, its second the flare word.
        assert_eq!(m.colors[4 * 4], [0, 0, 0]);
        assert_eq!(m.colors[4 * 4 + 1], [0x66, 0x55, 0x44]);
    }

    #[test]
    fn a_zero_count_still_emits_one_step() {
        let r = build_ribbon(0, 0, params(), &AnalyticTrig::new(), lcg());
        assert_eq!(r.steps.len(), 1);
        assert_eq!(r.packets.emitted_verts, 0);
    }

    #[test]
    fn the_same_rng_stream_reproduces_the_same_ribbon() {
        let t = AnalyticTrig::new();
        let a = build_ribbon(1, 0x0000_0020, params(), &t, lcg());
        let b = build_ribbon(1, 0x0000_0020, params(), &t, lcg());
        assert_eq!(a, b);
    }
}
