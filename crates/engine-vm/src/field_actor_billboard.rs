//! The field overlay's **attached-sprite tick** - the per-frame body of an
//! actor that rides a parent actor and draws a screen-space billboard at the
//! parent's position.
//!
//! The actor comes from template `0x801F28B8` (word `2` = `0x801E4470`), and
//! the template's one reference is the spawner `FUN_801E5668` (`lui`+`addiu`
//! at `0x801E5694`/`0x801E56A8`), which seats `+0x90 = parent`,
//! `+0x14..+0x18` = the offset, `+0x3C`/`+0x3E` = the size, `+0x74` = the
//! sprite asset, `+0x88` / `+0x5A` from its stack arguments, `+0x50 = 0x3E7`
//! and **`+0x94 = 0`**. Its one `jal` is `0x801DFFE0`, the field VM's op
//! `0x34` sub-1 spawn (`FieldHost::op34_sub1_spawn_attached`), which the op's
//! capture form then fills `+0x94` from. The arc-hop family is **not** the
//! spawner: its chained record (template `0x801F22AC`) ticks `FUN_801D5D60`,
//! not this routine.
//!
//! # What it draws: a light pool, not a sprite
//!
//! The "billboard" this actor projects is never a textured sprite. The
//! emitter `FUN_801E3984` draws an untextured, semi-transparent ellipse
//! centred on the projected point - a gouraud fan from colour A at the centre
//! to a quarter-mix at half radius, a ring from there to colour B at the rim -
//! and, when colour B is non-zero, flat colour-B fills from the rim out to
//! both screen edges and the bands above and below. Op0 bit 0 picks the
//! shape: clear is an additive glow (`A` = the operand colour, `B` = black;
//! the night-time lamp pools of `town01`), set is a subtractive darkness mask
//! with a lit hole (`A` = black, `B` = the operand colour; the lantern light
//! round the player in `cave01`). The two extents are view-space half sizes,
//! the height a lift above the parent's feet, and a following `0x40` block a
//! keyframe script ([`attached_sprite_script_tick`]) that tweens extents,
//! lift and both colours - the lamp pools breathe.
//!
//! REF: FUN_800172c0, FUN_800195a8, FUN_8003d2c4
//!
//! The ports carry their own `PORT` tags: `FUN_801e4470`
//! ([`attached_sprite_tick`] / [`sprite_rect`]), `FUN_801e5668`
//! ([`spawn_attached_sprite`]), `FUN_801e3e00`
//! ([`attached_sprite_script_tick`]) and `FUN_801e3984` with its three ring
//! emitters ([`light_pool_polys`]).
//!
//! # Provenance
//!
//! `FUN_801e4470` lives in the field overlay (PROT entry `0897_xxx_dat`,
//! slot-A base `0x801CE818`, file offset `0x15C58`): 83 instructions opening
//! `addiu sp, sp, -0x48` and closing `jr ra / addiu sp, sp, 0x48` at
//! `0x801E45B4`. Its own `overlay_0897_801e4470.txt` dump reports `size=1
//! bytes, 0 instructions` - the catalogued "no disassembly" artifact - so the
//! read here is off the extracted image, corroborated by
//! `ghidra/scripts/funcs/overlay_cutscene_dialogue_801e4470.txt`.
//!
//! Every VA-alias sibling that covers this address (`baka_fighter`, `dance`,
//! `debug_menu`, `fishing`, `slot_machine`) turns out not to: an image scan
//! finds a stack-frame prologue at `0x801E4470` in the **field** overlay
//! alone, and no other image holds a `jal` to it.
//!
//! # Shape
//!
//! ```text
//! parent = actor[+0x90]                     ; null -> nothing to ride
//! if parent[+0x10] & 8   -> actor[+0x10] |= 8 ; parent torn down, follow it
//! if parent[+0x10] & 2   -> return             ; parent hidden, draw nothing
//! pos = parent[+0x14..+0x1A] + actor[+0x14..+0x1A]
//! if actor[+0x94] -> FUN_801e3e00(actor)    ; the per-tick hook
//! FUN_800172c0()                            ; scene camera for this frame
//! FUN_800195a8(&pos, actor[+0x3C], actor[+0x3E], 0, &p0, &p1, &p2, &p3)
//! FUN_801e3984(&rect, actor[+0x74], actor[+0x88], actor[+0x5A])
//! ```
//!
//! The two flag tests are **not** the same shape. `8` propagates - the sprite
//! tears itself down with its parent - while `2` is a plain early return that
//! leaves the sprite alive and simply skips a frame's draw. A port that folds
//! them into one "parent gone" branch loses the difference between a hidden
//! parent and a dead one.

/// Parent-actor flag bits `FUN_801e4470` tests in `+0x10`.
pub mod parent_flag {
    /// Tear-down. Propagates into the sprite's own `+0x10`.
    pub const TEARDOWN: u32 = 8;
    /// Hidden. Skips this frame's draw and nothing else.
    pub const HIDDEN: u32 = 2;
}

/// The four screen points `FUN_800195a8` writes back, in the order retail
/// passes their addresses (`sp+0x30`, `sp+0x34`, `sp+0x38`, `sp+0x3C`).
///
/// Only the first and the last are read afterwards; the middle pair is
/// written and dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProjectedQuad {
    /// `sp+0x30` - the first corner.
    pub p0: (i16, i16),
    /// `sp+0x34`.
    pub p1: (i16, i16),
    /// `sp+0x38`.
    pub p2: (i16, i16),
    /// `sp+0x3C` - the opposite corner.
    pub p3: (i16, i16),
}

/// The centre-plus-span rect retail assembles at `sp+0x28..sp+0x30` and hands
/// to the sprite emitter `FUN_801e3984`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteRect {
    /// `sp+0x28` - `(p0.x + p3.x) >> 1`.
    pub centre_x: i16,
    /// `sp+0x2A` - `(p0.y + p3.y) >> 1`.
    pub centre_y: i16,
    /// `sp+0x2C` - `p3.x - p0.x`.
    pub width: i16,
    /// `sp+0x2E` - `p3.y - p0.y`.
    pub height: i16,
}

/// What one tick decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpriteTick {
    /// `+0x90` was null - retail falls straight through to the epilogue.
    NoParent,
    /// The parent carries `+0x10 & 8`; the sprite sets the same bit on itself
    /// and draws nothing.
    TearDown,
    /// The parent carries `+0x10 & 2`; nothing at all happens this frame.
    Hidden,
    /// The draw ran.
    Draw {
        /// The world point the billboard is projected from.
        world: (i16, i16, i16),
        /// `true` when `+0x94` was non-null and retail called the per-tick
        /// hook `FUN_801e3e00` before projecting.
        ran_hook: bool,
    },
}

/// Fold the parent's transform into the sprite's own offset, exactly as
/// retail does with three `lhu`/`lhu`/`addu`/`sh` triples - i.e. modulo
/// `2^16`, never saturating.
fn offset_world(parent: (i16, i16, i16), local: (i16, i16, i16)) -> (i16, i16, i16) {
    let add = |a: i16, b: i16| (a as u16).wrapping_add(b as u16) as i16;
    (
        add(parent.0, local.0),
        add(parent.1, local.1),
        add(parent.2, local.2),
    )
}

/// Reduce the projected quad to the emitter's rect.
///
/// Retail mixes signed and unsigned half-word reads here - `lh` for the two
/// values that go into a `sra`-halved sum, `lhu` for the two that go into a
/// `subu`. The results are stored back through `sh`, so the span pair is the
/// same 16 bits either way; the port uses wrapping arithmetic rather than
/// picking one signedness and pretending the other is not there.
///
/// PORT: FUN_801e4470 (`0x801E4550..0x801E4594`)
pub fn sprite_rect(quad: &ProjectedQuad) -> SpriteRect {
    let (x0, y0) = quad.p0;
    let (x3, y3) = quad.p3;
    SpriteRect {
        centre_x: ((x0 as i32 + x3 as i32) >> 1) as i16,
        centre_y: ((y0 as i32 + y3 as i32) >> 1) as i16,
        width: (x3 as u16).wrapping_sub(x0 as u16) as i16,
        height: (y3 as u16).wrapping_sub(y0 as u16) as i16,
    }
}

/// One tick of an attached sprite.
///
/// `parent` is `None` when `+0x90` is null. `project` stands in for the
/// `FUN_800172c0` + `FUN_800195a8` pair: given the folded world point and the
/// sprite's `+0x3C` / `+0x3E` extents it yields the four screen corners.
/// `emit` is `FUN_801e3984`.
///
/// PORT: FUN_801e4470
///
/// Live on both play hosts: `World::field_light_draws` runs it per attached
/// light with a projector over the field camera, and the native window's
/// `field_light_screen_prims` and the browser play page's `field_light_prims`
/// draw the result through `legaia_engine_ui::screen_prim::light_pool_prims`.
/// The per-tick hook `FUN_801E3E00` ([`attached_sprite_script_tick`]) runs in
/// the world's simulation tick rather than inside the draw, which is the same
/// order - retail calls it before projecting.
pub fn attached_sprite_tick<P, E>(
    parent: Option<(u32, (i16, i16, i16))>,
    local_offset: (i16, i16, i16),
    extents: (i16, i16),
    has_tick_hook: bool,
    project: P,
    emit: E,
) -> SpriteTick
where
    P: FnOnce((i16, i16, i16), (i16, i16)) -> ProjectedQuad,
    E: FnOnce(SpriteRect),
{
    let Some((parent_flags, parent_pos)) = parent else {
        return SpriteTick::NoParent;
    };
    if parent_flags & parent_flag::TEARDOWN != 0 {
        return SpriteTick::TearDown;
    }
    if parent_flags & parent_flag::HIDDEN != 0 {
        return SpriteTick::Hidden;
    }
    let world = offset_world(parent_pos, local_offset);
    // Retail folds the position *before* the hook and the camera call, so the
    // hook sees the frame's world point already committed to the stack.
    let quad = project(world, extents);
    emit(sprite_rect(&quad));
    SpriteTick::Draw {
        world,
        ran_hook: has_tick_hook,
    }
}

// ---------------------------------------------------------------------------
// Spawn: op 0x34 sub-1 -> FUN_801E5668
// ---------------------------------------------------------------------------

/// The decoded operand of the field VM's op `0x34` sub-1 (`34 1x`), read off
/// the arm's own loads (`FUN_801DE840` case `0x34`, `0x801DFEFC..0x801E0018`
/// in PROT 0897; `s6` points at the `0x1x` byte).
///
/// | Operand | Load | Meaning |
/// |---|---|---|
/// | `+0` | `lbu (s6)` | `0x1x`; bit 0 picks which colour slot the packed word fills and the blend mode |
/// | `+1..+3` | three `lbu` + shifts | the packed colour `R << 16 \| G << 8 \| B` |
/// | `+4` / `+6` | `jal 0x8003CE9C` | the light's half extents (`+0x3C` / `+0x3E`) |
/// | `+8` | `jal 0x8003CE9C`, `negu` | the height above the parent, stored as Y offset `-h` |
/// | `+10..+11` | not read | |
///
/// The earlier reading named `+4` / `+6` world X / Z. They are the two
/// extents `FUN_801E4470` hands the billboard projector as `a1` / `a2`, and
/// the offset triple is `(0, -h, 0)` - the arm zeroes `sp+0x60` and
/// `sp+0x64` before the one load it makes into `sp+0x62`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachedSpriteSpawn {
    /// The `0x1x` byte.
    pub op0: u8,
    /// `R << 16 | G << 8 | B`.
    pub packed_rgb: u32,
    /// `(+0x3C, +0x3E)` half extents, view-space units.
    pub half_extent: (i16, i16),
    /// The raw `+8` height; the actor's Y offset is its negation.
    pub height: i16,
}

impl AttachedSpriteSpawn {
    /// Decode from the operand starting at the `0x1x` byte. `None` when the
    /// sub-op is not 1 or the twelve operand bytes are not all there.
    pub fn decode(operand: &[u8]) -> Option<Self> {
        let op0 = *operand.first()?;
        if op0 >> 4 != 1 || operand.len() < 12 {
            return None;
        }
        let s16 = |at: usize| i16::from_le_bytes([operand[at], operand[at + 1]]);
        Some(Self {
            op0,
            packed_rgb: (u32::from(operand[1]) << 16)
                | (u32::from(operand[2]) << 8)
                | u32::from(operand[3]),
            half_extent: (s16(4), s16(6)),
            height: s16(8),
        })
    }
}

/// Blend mode `+0x5A`: the ABR equation every primitive of the light pool
/// draws with (`FUN_801E3984` folds it into the draw-mode word as
/// `abr << 5 | 0x1E`).
pub mod light_abr {
    /// `B + F` - an additive glow (op0 bit 0 clear).
    pub const ADDITIVE: u8 = 1;
    /// `B - F` - a darkness mask with a lit hole (op0 bit 0 set).
    pub const SUBTRACTIVE: u8 = 2;
}

/// The attached light actor: template `0x801F28B8`'s fields this family
/// reads and writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachedSprite {
    /// `+0x14..+0x18` - offset from the parent, `(0, -height, 0)` at spawn.
    pub offset: (i16, i16, i16),
    /// `+0x3C` / `+0x3E`.
    pub half_extent: (i16, i16),
    /// `+0x74` - the centre colour.
    pub color_a: u32,
    /// `+0x88` - the rim / outside colour.
    pub color_b: u32,
    /// `+0x5A` - see [`light_abr`].
    pub abr: u8,
    /// `+0x94` - the keyframe script, copied out of the bytecode. Empty =
    /// no script (retail's null pointer).
    pub script: Vec<u8>,
    /// `+0x9E` - byte cursor into [`Self::script`].
    pub cursor: i16,
    /// `+0x9C` - frames into the current keyframe.
    pub frame: i16,
    /// `+0x40` / `+0x58` / `+0x6A` - the extents and Y offset captured at a
    /// keyframe's first frame.
    pub from_extent: (i16, i16),
    pub from_y: i16,
    /// `+0x80..+0x82` / `+0x83..+0x85` - the two colours captured likewise.
    pub from_a: [u8; 3],
    pub from_b: [u8; 3],
    /// `+0x10 & 8` - the tear-down bit.
    pub torn_down: bool,
}

/// Seed the attached light: retail `FUN_801e5668(parent, &offset, &extent,
/// a3, [sp+0x38], [sp+0x3C])` (field overlay PROT 0897,
/// `0x801E5668..0x801E5738`), with the argument set the op `0x34` sub-1 arm
/// builds at `0x801DFFAC..0x801DFFDC`.
///
/// Op0 bit 0 clear: `a3` (`+0x74`) = the packed colour, `+0x88 = 0`,
/// `+0x5A = 1`. Bit 0 set: `+0x74 = 0`, `+0x88` = the colour, `+0x5A = 2`.
/// `+0x94 = 0`, `+0x54 = 0`, `+0x50 = 0x3E7` in both; the arm fills `+0x94`
/// afterwards when a `0x40` block follows.
///
/// PORT: FUN_801e5668
pub fn spawn_attached_sprite(spawn: &AttachedSpriteSpawn, script: Vec<u8>) -> AttachedSprite {
    let (color_a, color_b, abr) = if spawn.op0 & 1 == 0 {
        (spawn.packed_rgb, 0, light_abr::ADDITIVE)
    } else {
        (0, spawn.packed_rgb, light_abr::SUBTRACTIVE)
    };
    AttachedSprite {
        offset: (0, spawn.height.wrapping_neg(), 0),
        half_extent: spawn.half_extent,
        color_a,
        color_b,
        abr,
        script,
        cursor: 0,
        frame: 0,
        from_extent: (0, 0),
        from_y: 0,
        from_a: [0; 3],
        from_b: [0; 3],
        torn_down: false,
    }
}

/// How many bytes of a captured keyframe script retail can ever read, from
/// its start: records are `0x02` (fifteen bytes), `0x40 len` headers (two -
/// the walker steps over the header and reads the keyframe inside), and a
/// `0x00` (retire) or `0x01` (restart) terminator. Any other byte makes the
/// walker restart, so the scan stops there too.
pub fn attached_script_extent(bytes: &[u8]) -> usize {
    let mut i = 0usize;
    while let Some(&b) = bytes.get(i) {
        match b {
            0 | 1 => return i + 1,
            2 => i += 15,
            0x40 => i += 2,
            _ => return i,
        }
    }
    bytes.len()
}

fn rgb_bytes(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

fn lerp_i16(from: i16, to: i16, t: i32, dur: i32) -> i16 {
    // `subu` / `mult` / `div` / `addu`: a truncating divide, stored `sh`.
    if dur == 0 {
        return to;
    }
    (i32::from(from) + (i32::from(to) - i32::from(from)) * t / dur) as i16
}

/// One tick of the attached light's keyframe script: retail
/// `FUN_801e3e00(actor)` (field overlay PROT 0897, `0x801E3E00..0x801E446C`),
/// run by `FUN_801E4470` whenever `+0x94` is non-null. `dt` is
/// `DAT_1F800393`.
///
/// The walker loops until one record is consumed:
///
/// | Byte | Record |
/// |---|---|
/// | `0x00` | retire: `+0x10 \|= 8` |
/// | `0x01` | restart: cursor `= 0` |
/// | `0x02` | keyframe `[2][dur][ext_w][ext_h][A: rgb][B: rgb][y]` - fifteen bytes |
/// | `0x40` | a block header: cursor `+= 2`, keep walking |
/// | other | restart and keep walking |
///
/// A keyframe's first frame (`+0x9C == 0`) captures the current extents,
/// Y offset and both colours and draws nothing new; later frames interpolate
/// each towards the record by `t / dur` (the Y target is the record's `-y`);
/// the frame `+0x9C >= dur` snaps to the record, resets `+0x9C` and moves the
/// cursor on. Colour channels interpolate separately and are masked to a byte.
///
/// PORT: FUN_801e3e00
pub fn attached_sprite_script_tick(s: &mut AttachedSprite, dt: u8) {
    if s.script.is_empty() {
        return;
    }
    // A malformed script must not spin: retail would, the port gives up after
    // one walk of the whole record list.
    for _ in 0..=s.script.len() {
        let at = s.cursor.max(0) as usize;
        let Some(&kind) = s.script.get(at) else {
            s.cursor = 0;
            return;
        };
        match kind {
            0 => {
                s.torn_down = true;
                return;
            }
            1 => {
                s.cursor = 0;
                return;
            }
            2 => {
                let Some(rec) = s.script.get(at..at + 15) else {
                    s.cursor = 0;
                    return;
                };
                let s16 = |o: usize| i16::from_le_bytes([rec[o], rec[o + 1]]);
                let dur = s16(1);
                let (w, h, y) = (s16(3), s16(5), s16(13).wrapping_neg());
                let a = [rec[7], rec[8], rec[9]];
                let b = [rec[10], rec[11], rec[12]];
                let pack =
                    |c: [u8; 3]| (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2]);
                if s.frame >= dur {
                    s.half_extent = (w, h);
                    s.offset.1 = y;
                    s.color_a = pack(a);
                    s.color_b = pack(b);
                    s.frame = 0;
                    s.cursor = s.cursor.wrapping_add(15);
                    return;
                }
                if s.frame == 0 {
                    s.from_extent = s.half_extent;
                    s.from_y = s.offset.1;
                    s.from_a = rgb_bytes(s.color_a);
                    s.from_b = rgb_bytes(s.color_b);
                } else {
                    let t = i32::from(s.frame);
                    let d = i32::from(dur);
                    s.half_extent = (
                        lerp_i16(s.from_extent.0, w, t, d),
                        lerp_i16(s.from_extent.1, h, t, d),
                    );
                    s.offset.1 = lerp_i16(s.from_y, y, t, d);
                    let mix = |from: [u8; 3], to: [u8; 3]| -> u32 {
                        let ch = |i: usize| {
                            ((i32::from(from[i]) + (i32::from(to[i]) - i32::from(from[i])) * t / d)
                                & 0xFF) as u32
                        };
                        (ch(0) << 16) | (ch(1) << 8) | ch(2)
                    };
                    s.color_a = mix(s.from_a, a);
                    s.color_b = mix(s.from_b, b);
                }
                s.frame = s.frame.wrapping_add(i16::from(dt));
                return;
            }
            0x40 => s.cursor = s.cursor.wrapping_add(2),
            _ => s.cursor = 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Draw: FUN_801E3984 and its three ring emitters
// ---------------------------------------------------------------------------

/// Retail's quarter-turn sine table at `0x801F2904` (field overlay PROT
/// 0897 file `0x240EC`): `sin(k * 22.5deg) * 0x1000` for `k = 0..=4`.
pub const QUARTER_SINE: [i32; 5] = [0, 0x61F, 0xB50, 0xEC8, 0x1000];

/// The one screen-space primitive shape the light pool emits: an untextured,
/// semi-transparent polygon with per-vertex colour. Three vertices for the
/// inner fan (`POLY_G3`), four for the ring and the fills (`POLY_G4` /
/// `POLY_F4`, the flat ones carrying one colour four times).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightPoly {
    /// Screen corners in the PSX primitive's own vertex order.
    pub xy: [(i16, i16); 4],
    /// Per-vertex `R << 16 | G << 8 | B`.
    pub rgb: [u32; 4],
    /// `3` for the fan triangle (only `xy[0..3]` are live), else `4`.
    pub verts: u8,
}

fn half(v: i32) -> i32 {
    // `srl 31` / `addu` / `sra 1`: halve rounding toward zero.
    (v + (((v as u32) >> 31) as i32)) >> 1
}

/// `colA + (colB - colA) / 4` per channel, retail's masked form
/// (`0x801E39FC..0x801E3AC4`): each channel masked to its six high bits
/// before the subtract, divided rounding toward zero, re-masked after the add.
fn quarter_mix(a: u32, b: u32) -> u32 {
    let step = |mask: u32, keep: u32| -> u32 {
        let d = (b & mask) as i32 - (a & mask) as i32;
        let d = (if d < 0 { d + 3 } else { d }) >> 2;
        ((d + a as i32) as u32) & keep
    };
    step(0xFC_0000, 0xFF_0000) + step(0xFC00, 0xFF00) + step(0xFC, 0xFF)
}

/// Build one frame of the attached light: retail `FUN_801e3984(&rect, colA,
/// colB, abr)` (field overlay PROT 0897, `0x801E3984..0x801E3DFC`) with its
/// three emitters `FUN_801e3658` (inner fan), `FUN_801e3764` (ring) and
/// `FUN_801e3894` (side fill).
///
/// An ellipse of half-width `rect.width / 2` and half-height
/// `rect.height / 2` centred on `rect`, in four quadrants of four
/// `22.5deg` steps each:
///
/// * the **fan** - centre (colour A) to the half-radius points (the mix
///   `A + (B - A) / 4`);
/// * the **ring** - half radius (the mix) to full radius (colour B);
/// * the **side fill**, only when colour B is non-zero - from the rim
///   horizontally to the screen edge (`x = 320` or `0`) in flat colour B;
/// * the **bands** above and below the ellipse, full width in flat colour B,
///   each only when on screen (`cy - ry > 0`, `cy + ry < 0xEF`).
///
/// Every primitive is semi-transparent at [`AttachedSprite::abr`]: an
/// additive glow (`A` bright, `B` black) or a subtractive darkness mask (`A`
/// black, `B` the darkness) - the draw-mode packet `abr << 5 | 0x1E` is the
/// last one linked, so the ordering table draws it first.
///
/// Returned in **link order** - the order retail's `AddPrim`
/// (`FUN_8003D2C4`) calls run, all into one ordering-table slot
/// (`*0x1F8003F4 + 8`). That slot is LIFO, so the drawn order is the reverse;
/// `legaia_engine_ui::screen_prim`'s sort reproduces exactly that for prims
/// submitted in link order at one `ot_index`.
///
/// PORT: FUN_801e3984
/// PORT: FUN_801e3658
/// PORT: FUN_801e3764
/// PORT: FUN_801e3894
pub fn light_pool_polys(rect: &SpriteRect, color_a: u32, color_b: u32) -> Vec<LightPoly> {
    let cx = i32::from(rect.centre_x);
    let cy = i32::from(rect.centre_y);
    let w = i32::from(rect.width);
    let h = i32::from(rect.height);
    let mid = quarter_mix(color_a, color_b);
    let mut linked: Vec<LightPoly> = Vec::with_capacity(52);
    let flat = |xy: [(i32, i32); 4]| LightPoly {
        xy: xy.map(|(x, y)| (x as i16, y as i16)),
        rgb: [color_b; 4],
        verts: 4,
    };
    // `(table * extent) >> 13` - the `srl 13` of an unsigned `mflo`.
    let scale = |k: usize, e: i32| (((QUARTER_SINE[k] * e) as u32) >> 13) as i32;
    let mut px = scale(0, w);
    let mut py = scale(4, h);
    // Top band (`0x801E3AE0..0x801E3B60`).
    let top = cy - py;
    if top > 0 {
        linked.push(flat([(0, -4), (320, -4), (0, top), (320, top)]));
    }
    // Bottom band (`0x801E3B64..0x801E3BFC`).
    let bottom = cy + py;
    if bottom < 0xEF {
        linked.push(flat([(0, bottom), (320, bottom), (0, 0xF0), (320, 0xF0)]));
    }
    for k in 0..4usize {
        let nx = scale(k + 1, w);
        let ny = scale(3 - k, h);
        // The four quadrant mirrors, in retail's call order.
        let quads = [
            (px, py, nx, ny),
            (-px, py, -nx, ny),
            (px, -py, nx, -ny),
            (-px, -py, -nx, -ny),
        ];
        for &(x0, y0, x1, y1) in &quads {
            // FUN_801E3658 - the fan.
            linked.push(LightPoly {
                xy: [
                    (cx as i16, cy as i16),
                    ((cx + half(x0)) as i16, (cy + half(y0)) as i16),
                    ((cx + half(x1)) as i16, (cy + half(y1)) as i16),
                    ((cx + half(x1)) as i16, (cy + half(y1)) as i16),
                ],
                rgb: [color_a, mid, mid, mid],
                verts: 3,
            });
        }
        for &(x0, y0, x1, y1) in &quads {
            // FUN_801E3764 - the ring.
            linked.push(LightPoly {
                xy: [
                    ((cx + half(x0)) as i16, (cy + half(y0)) as i16),
                    ((cx + half(x1)) as i16, (cy + half(y1)) as i16),
                    ((cx + x0) as i16, (cy + y0) as i16),
                    ((cx + x1) as i16, (cy + y1) as i16),
                ],
                rgb: [mid, mid, color_b, color_b],
                verts: 4,
            });
        }
        if color_b != 0 {
            for &(x0, y0, x1, y1) in &quads {
                // FUN_801E3894 - the side fill to the screen edge.
                let edge = if x1 > 0 { 320 } else { 0 };
                linked.push(flat([
                    (cx + x0, cy + y0),
                    (cx + x1, cy + y1),
                    (edge, cy + y0),
                    (edge, cy + y1),
                ]));
            }
        }
        px = nx;
        py = ny;
    }
    linked
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn quad(p0: (i16, i16), p3: (i16, i16)) -> ProjectedQuad {
        ProjectedQuad {
            p0,
            p1: (0, 0),
            p2: (0, 0),
            p3,
        }
    }

    #[test]
    fn null_parent_is_a_plain_return() {
        let r = attached_sprite_tick(
            None,
            (0, 0, 0),
            (0, 0),
            false,
            |_, _| unreachable!(),
            |_| unreachable!(),
        );
        assert_eq!(r, SpriteTick::NoParent);
    }

    #[test]
    fn teardown_and_hidden_are_different_branches() {
        let td = attached_sprite_tick(
            Some((parent_flag::TEARDOWN, (0, 0, 0))),
            (0, 0, 0),
            (0, 0),
            false,
            |_, _| unreachable!(),
            |_| unreachable!(),
        );
        assert_eq!(td, SpriteTick::TearDown);
        // `8` wins over `2`: retail tests it first.
        let both = attached_sprite_tick(
            Some((parent_flag::TEARDOWN | parent_flag::HIDDEN, (0, 0, 0))),
            (0, 0, 0),
            (0, 0),
            false,
            |_, _| unreachable!(),
            |_| unreachable!(),
        );
        assert_eq!(both, SpriteTick::TearDown);
        let hidden = attached_sprite_tick(
            Some((parent_flag::HIDDEN, (0, 0, 0))),
            (0, 0, 0),
            (0, 0),
            false,
            |_, _| unreachable!(),
            |_| unreachable!(),
        );
        assert_eq!(hidden, SpriteTick::Hidden);
    }

    #[test]
    fn draw_folds_the_parent_transform_and_emits_once() {
        let seen = RefCell::new(Vec::new());
        let r = attached_sprite_tick(
            Some((0, (100, 200, 300))),
            (-10, 5, 20),
            (16, 32),
            true,
            |world, ext| {
                assert_eq!(world, (90, 205, 320));
                assert_eq!(ext, (16, 32));
                quad((10, 20), (50, 80))
            },
            |rect| seen.borrow_mut().push(rect),
        );
        assert_eq!(
            r,
            SpriteTick::Draw {
                world: (90, 205, 320),
                ran_hook: true
            }
        );
        assert_eq!(
            seen.into_inner(),
            vec![SpriteRect {
                centre_x: 30,
                centre_y: 50,
                width: 40,
                height: 60,
            }]
        );
    }

    #[test]
    fn centres_floor_rather_than_truncate() {
        // `addu` then `sra 1`: -3 halves to -2, not -1.
        let r = sprite_rect(&quad((-3, -3), (0, 0)));
        assert_eq!((r.centre_x, r.centre_y), (-2, -2));
    }

    #[test]
    fn spans_wrap_like_the_halfword_store() {
        let r = sprite_rect(&quad((0x4000, 0), (-0x4000, 0)));
        assert_eq!(r.width, -0x8000);
    }
}

#[cfg(test)]
mod light_tests {
    use super::*;

    fn spawn(op0: u8) -> AttachedSpriteSpawn {
        // `town01` lamp: `10 20 20 00 6A 03 88 01 80 00 00 00`.
        AttachedSpriteSpawn::decode(&[
            op0, 0x20, 0x20, 0x00, 0x6A, 0x03, 0x88, 0x01, 0x80, 0x00, 0x00, 0x00,
        ])
        .unwrap()
    }

    #[test]
    fn op0_bit0_picks_glow_or_darkness() {
        let glow = spawn_attached_sprite(&spawn(0x10), Vec::new());
        assert_eq!(
            (glow.color_a, glow.color_b, glow.abr),
            (0x202000, 0, light_abr::ADDITIVE)
        );
        assert_eq!(glow.offset, (0, -0x80, 0));
        assert_eq!(glow.half_extent, (0x36A, 0x188));
        let dark = spawn_attached_sprite(&spawn(0x11), Vec::new());
        assert_eq!(
            (dark.color_a, dark.color_b, dark.abr),
            (0, 0x202000, light_abr::SUBTRACTIVE)
        );
        assert!(AttachedSpriteSpawn::decode(&[0x01; 12]).is_none());
    }

    #[test]
    fn script_extent_stops_at_the_terminator() {
        let mut s = vec![2u8; 15];
        s.extend([0x40, 0x0F]);
        s.extend([2u8; 15]);
        s.extend([0x40, 0x01, 0x01, 0xAA, 0xBB]);
        assert_eq!(attached_script_extent(&s), s.len() - 2);
    }

    #[test]
    fn keyframe_captures_then_tweens_then_snaps() {
        // One keyframe: 4 frames to extents (100, 200), A = (0x40,0,0),
        // B = 0, lift 8, then restart.
        let mut rec = vec![2u8, 4, 0, 100, 0, 200, 0, 0x40, 0, 0, 0, 0, 0, 8, 0];
        rec.push(1);
        let mut s = spawn_attached_sprite(&spawn(0x10), rec);
        s.half_extent = (0, 0);
        s.offset.1 = 0;
        s.color_a = 0;
        attached_sprite_script_tick(&mut s, 1); // frame 0: capture only
        assert_eq!((s.half_extent, s.frame), ((0, 0), 1));
        attached_sprite_script_tick(&mut s, 1); // frame 1: 1/4
        assert_eq!(s.half_extent, (25, 50));
        assert_eq!(s.offset.1, -2);
        assert_eq!(s.color_a, 0x10_0000);
        attached_sprite_script_tick(&mut s, 1);
        attached_sprite_script_tick(&mut s, 1);
        attached_sprite_script_tick(&mut s, 1); // frame 4 >= dur: snap
        assert_eq!(
            (s.half_extent, s.offset.1, s.color_a),
            ((100, 200), -8, 0x40_0000)
        );
        assert_eq!((s.frame, s.cursor), (0, 15));
        attached_sprite_script_tick(&mut s, 1); // `01`: restart
        assert_eq!(s.cursor, 0);
        let mut end = spawn_attached_sprite(&spawn(0x10), vec![0]);
        attached_sprite_script_tick(&mut end, 1);
        assert!(end.torn_down);
    }

    #[test]
    fn glow_draws_fan_and_ring_only_darkness_adds_the_fills() {
        let rect = SpriteRect {
            centre_x: 160,
            centre_y: 120,
            width: 80,
            height: 40,
        };
        // Colour B zero: 2 bands on screen + 16 fan + 16 ring, no fills.
        let glow = light_pool_polys(&rect, 0x404040, 0);
        assert_eq!(glow.len(), 2 + 16 + 16);
        // First fan triangle: centre in colour A, half-radius points in the
        // quarter mix (0x40 - 0x40/4 = 0x30).
        let fan = glow[2];
        assert_eq!(fan.verts, 3);
        assert_eq!(fan.xy[0], (160, 120));
        assert_eq!(fan.xy[1], (160, 130)); // (0, h/2 / 2)
        assert_eq!(fan.rgb, [0x404040, 0x303030, 0x303030, 0x303030]);
        // The rim reaches the full half extents: x = w/2 at the last step.
        let rims: Vec<_> = glow
            .iter()
            .filter(|p| p.verts == 4)
            .flat_map(|p| p.xy)
            .collect();
        assert!(rims.contains(&(200, 120)));
        assert!(rims.contains(&(160, 140)));
        // Colour B set: +16 side fills to the screen edges.
        let dark = light_pool_polys(&rect, 0, 0x808020);
        assert_eq!(dark.len(), 2 + 16 + 16 + 16);
        assert!(dark.iter().any(|p| p.xy[2].0 == 320) && dark.iter().any(|p| p.xy[2].0 == 0));
        // An ellipse touching the top edge drops the top band.
        let tall = SpriteRect {
            height: 400,
            ..rect
        };
        assert_eq!(light_pool_polys(&tall, 0, 0x10).len(), 16 * 3);
    }
}
