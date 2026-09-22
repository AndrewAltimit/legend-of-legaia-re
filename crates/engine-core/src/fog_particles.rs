//! Field fog puffs - retail's `fog_set` particle system, the thing the
//! ambient emitter (`FUN_801D6058`) feeds.
//!
//! The emitter names tiles; nothing in it draws. What it calls per particle
//! is `FUN_801D629C`, and that routine is a **spawner**: it takes two
//! arguments (a tile X and a tile Z - the `a2`/`a3` the emitter also loads
//! are never read), looks the tile up in the scene's fog-region table, pops
//! a free slot off an 80-record pool and fills one `0x18`-byte record.
//! Drawing happens in SCUS: the field render pass (`0x80026EBC` in
//! `FUN_80026CE4`) stages the four UV rows of `0x8007322C` into scratchpad
//! `0x1F8002D0` and calls `FUN_8003F348`, which walks the pool and runs
//! `FUN_8003F3FC` on every live record - age, drift, colour, then **two
//! textured semi-transparent quads** per particle through `FUN_8003F86C`.
//! The dev trace the emitter can print is `fog_set %d %d`, and the two
//! numbers it prints are the pool's live count and its cap.
//!
//! Every constant below is read off the disassembly (`see
//! ghidra/scripts/funcs/overlay_cutscene_dialogue_801d629c.txt`,
//! `8003f348.txt`, `8003f3fc.txt`, `8003f86c.txt`, `8003f838.txt`,
//! `8001fa00.txt`, `8001fa34.txt`, `8001fa68.txt`, `80026ce4.txt`), not the
//! C. The `overlay_0896_801d629c.txt` dump at the same VA is a 71-instruction
//! fragment of some other routine (no prologue, `v0` used before it is set)
//! and is not this function; the field image `overlay_field_0897.bin` at
//! file `0x7A84` matches the cutscene-image dump instruction for instruction.
//!
//! # The pool (`_DAT_8007B7E0`)
//!
//! MAIN INIT `FUN_801D6704` allocates it at `0x801D7364`:
//! `FUN_80017888(0, 0x824)`, then `FUN_8001FA00(pool, pool + 4, 0x50)` seeds
//! the free stack and `pool[+2] = 0x50`. Layout:
//!
//! ```text
//! +0x00  i16   free-stack top index (the count `FUN_8001FA34` decrements)
//! +0x02  u16   capacity (0x50 = 80) - what the per-frame walk iterates
//! +0x04  i16[80] free-stack entries, seeded 0..79 (so the first pop is 79)
//! +0xA4  record[80] x 0x18 bytes
//! ```
//!
//! One record:
//!
//! ```text
//! +0x00  u16  age            (0 at spawn; += rate * dt per rendered frame)
//! +0x02  u16  rate           ((rand & 7) + 8)
//! +0x04  u8   slot index
//! +0x05  u8   alive
//! +0x06  i8   vx  = -(sin[angle] * region.speed >> 14)
//! +0x07  i8   vz  = -(cos[angle] * region.speed >> 14)
//! +0x08  i32  x << 4         (tile x << 11)
//! +0x0C  i32  z << 4         (tile z << 11)
//! +0x10  i16  y              (-(rand & 0x7F); world-map bit subtracts 0x28)
//! +0x12  i16  1
//! +0x14  u8x3 grey           (rand & 0x7F, one value in all three bytes)
//! ```
//!
//! # The region table (MAN section 4, `DAT_80073ED8`)
//!
//! `[u8 count][record x 0xB]` after the section's `u24` length: the byte at
//! section `+3` is the count (`DAT_80073EDC`), records start at `+4`. Per
//! record: `+0` enable, `+1..+4` the **open** tile box `(x0, z0, x1, z1)`,
//! `+5` angle base, `+6` angle spread (both in sixteenths of the 4096-step
//! circle), `+7` speed, `+9..+10` a story-flag index whose inverse the field
//! VM's op `0x4C` nibble-C sub-1 writes back into `+0`. Byte `+8` is not read
//! by anything traced here.
//!
//! # What "drawn" means
//!
//! Each half of a particle is one `POLY_FT4` (tag `0x09` words, command
//! `0x2E` = textured, semi-transparent, texture-blended) linked at OT bucket
//! `view_z >> 5`, sampling texture page `0x27` (VRAM `(448, 256)`, 4bpp, ABR
//! `1` = additive) through CLUT `0x7640` (`(0, 473)`). The left half takes
//! UV row 1 of the staged table (`v 0x58..0x6F`), the right half row 0
//! (`v 0x40..0x57`); rows 2 and 3 are staged too but this routine never
//! reads them. The quad is axis-aligned in screen space between two
//! projected world points: the top-left corner sits `0x80` units **above**
//! the particle (`RotMatrixX(0x400)` folded into the matrix the pass sets at
//! `0x8003F384..0x8003F394`, so the packet's `+0x80 z` becomes a world `-y`),
//! the bottom-right at the particle itself, and each half is `2 * half_width`
//! wide - see [`FogParticle::half_width`] for why that width has no random
//! term despite the PRNG call.

use crate::action_effect_script::RotationLut;
use legaia_engine_vm::psx_camera::FieldCameraView;

/// Pool capacity - `pool[+2]`, `addiu a2,zero,0x50` at `0x801D7374`.
pub const FOG_POOL_SLOTS: usize = 0x50;

/// Live-particle cap the spawner tests the pool population against
/// (`_DAT_8007BCA8 < _DAT_8007BCB0`): the field reset writes `0x18` at
/// `0x8003B6E8` (`FUN_8003AEB0`'s body), or `0x48` when either debug byte it
/// tests first is set.
pub const FOG_CAP_DEFAULT: u16 = 0x18;
/// The debug-flag cap (`addiu a0,zero,0x48` at `0x8003B6DC`).
pub const FOG_CAP_DEBUG: u16 = 0x48;

/// One fog-region record's stride in the MAN section-4 table (`addiu v1,v1,0xb`
/// at `0x801D63B4`).
pub const FOG_REGION_STRIDE: usize = 0xB;

/// GP0 texpage word every fog quad carries (`0x8007322C` row word 1, high
/// halfword): page `(448, 256)`, 4bpp, ABR 1.
pub const FOG_TPAGE: u16 = 0x0027;
/// GP0 CLUT word (`0x8007322C` row word 0, high halfword): `(0, 473)`.
pub const FOG_CLUT: u16 = 0x7640;

/// The four staged UV rows at `0x8007322C` (16 bytes each, copied verbatim
/// to `0x1F8002D0` by the field render pass). Word `k` is vertex `k`'s
/// `(u, v)` in its low halfword; word 0 carries the CLUT and word 1 the
/// texpage in the high halfword, the `POLY_FT4` packet layout.
pub const FOG_UV_ROWS: [[u32; 4]; 4] = [
    [0x7640_4000, 0x0027_403F, 0x0000_5700, 0x0000_573F],
    [0x7640_5800, 0x0027_583F, 0x0000_6F00, 0x0000_6F3F],
    [0x7640_7800, 0x0027_782F, 0x0000_8F00, 0x0000_8F2F],
    [0x7640_6000, 0x0027_6027, 0x0000_7700, 0x0000_7727],
];

/// Staged row the **left** half samples (`addiu a3,s5,0x10` at `0x8003F758`).
pub const FOG_LEFT_UV_ROW: usize = 1;
/// Staged row the **right** half samples (`move a3,s5` at `0x8003F7C0`).
pub const FOG_RIGHT_UV_ROW: usize = 0;

/// Age below which the brightness ramps up (`sltiu v0,a0,0x400`).
pub const FOG_FADE_IN_END: u16 = 0x400;
/// Age above which it ramps down (`sltiu v0,a0,0xc01`).
pub const FOG_FADE_OUT_START: u16 = 0xC00;
/// Vertical extent of each sheet above the particle (`addi a2,a2,0x80`).
pub const FOG_SHEET_HEIGHT: i32 = 0x80;
/// Base of the half-width term (`addiu v0,v0,0x180`).
pub const FOG_WIDTH_BASE: i32 = 0x180;
/// NCLIP tolerance (`addi t6,t6,0x1f40`): a half is culled once the signed
/// screen area it would cover reaches this many quarter-pixels.
pub const FOG_NCLIP_TOLERANCE: i32 = 0x1F40;
/// Half-extents of the player proximity box that ages a particle twice
/// (`+-0x180` in X, `+-0x80` in Y and Z).
pub const FOG_PLAYER_BOX: [i32; 3] = [0x180, 0x80, 0x80];
/// D-pad bits of the held pad word (`andi v1,v1,0xf000`): while any is held
/// the proximity boost is three times stronger again.
pub const FOG_DPAD_MASK: u32 = 0xF000;

/// One fog-region record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FogRegion {
    /// `+0` - the spawner keeps a match only when this is non-zero.
    pub enabled: bool,
    /// `+1` / `+2` - open lower bounds (`rec < tile`).
    pub x0: u8,
    pub z0: u8,
    /// `+3` / `+4` - open upper bounds (`tile < rec`).
    pub x1: u8,
    pub z1: u8,
    /// `+5` - angle base, in sixteenths of the 4096-step circle.
    pub angle_base: u8,
    /// `+6` - angle spread, same units; the random term is `rand & 0xFFF`
    /// scaled by it.
    pub angle_spread: u8,
    /// `+7` - drift speed; `sin/cos * speed >> 14` per axis.
    pub speed: u8,
    /// `+8` - read by nothing traced here.
    pub byte_8: u8,
    /// `+9..+10` - the story-flag index the nibble-C sub-1 reset tests.
    pub flag_index: u16,
}

impl FogRegion {
    /// Decode one `0xB`-byte record.
    pub fn parse(rec: &[u8]) -> Option<Self> {
        if rec.len() < FOG_REGION_STRIDE {
            return None;
        }
        Some(Self {
            enabled: rec[0] != 0,
            x0: rec[1],
            z0: rec[2],
            x1: rec[3],
            z1: rec[4],
            angle_base: rec[5],
            angle_spread: rec[6],
            speed: rec[7],
            byte_8: rec[8],
            flag_index: u16::from_le_bytes([rec[9], rec[10]]),
        })
    }

    /// The spawner's box test - strict on all four sides
    /// (`0x801D6340..0x801D6388`: `slt rec,tile` then `slt tile,rec`).
    pub fn contains(&self, tile_x: i32, tile_z: i32) -> bool {
        i32::from(self.x0) < tile_x
            && tile_x < i32::from(self.x1)
            && i32::from(self.z0) < tile_z
            && tile_z < i32::from(self.z1)
    }
}

/// Parse a MAN section-4 payload (the bytes after its `u24` length:
/// `[count][records]`) into fog regions.
pub fn parse_fog_regions(section_body: &[u8]) -> Vec<FogRegion> {
    let Some(&count) = section_body.first() else {
        return Vec::new();
    };
    (0..usize::from(count))
        .filter_map(|i| {
            let at = 1 + i * FOG_REGION_STRIDE;
            section_body
                .get(at..at + FOG_REGION_STRIDE)
                .and_then(FogRegion::parse)
        })
        .collect()
}

/// One pool record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FogParticle {
    pub age: u16,
    pub rate: u16,
    pub slot: u8,
    pub alive: bool,
    pub vx: i8,
    pub vz: i8,
    /// World X `<< 4`.
    pub x: i32,
    /// World Z `<< 4`.
    pub z: i32,
    /// World Y (raw retail Y-down; negative is up).
    pub y: i16,
    /// The one grey value stored in `+0x14..+0x16`.
    pub grey: u8,
}

impl FogParticle {
    /// Brightness from age (`0x8003F4F4..0x8003F550`): a `0..0xFF` ramp up over
    /// the first `0x400`, full until `0xC00`, then down; `None` once the ramp
    /// has gone negative, which is the age-out kill.
    pub fn brightness(age: u16) -> Option<i32> {
        let a = i32::from(age);
        let s = if a < i32::from(FOG_FADE_IN_END) {
            a >> 2
        } else if a <= i32::from(FOG_FADE_OUT_START) {
            0x100
        } else {
            0xFF - ((a - i32::from(FOG_FADE_OUT_START)) >> 2)
        };
        if s < 0 {
            return None;
        }
        Some(s.min(0xFF))
    }

    /// Half-width of each sheet half, from the post-update age
    /// (`0x8003F730..0x8003F748`).
    ///
    /// Retail adds a "random" byte here: it stores `rate` into the PRNG
    /// state `0x1F8002A8` and calls `FUN_8003F838`, whose step is
    /// `v = state * 12 + 2; state = (v << 16) + (v >> 16)` and whose return
    /// value is that new state. With `rate <= 15`, `v < 0x8000`, so the
    /// returned word is `v << 16` and its low byte - the only part the caller
    /// keeps (`andi s0,v0,0xff`) - is always zero. The width is therefore
    /// deterministic: `(0x180 + (age >> 4)) >> 1`.
    pub fn half_width(age: u16) -> i32 {
        (FOG_WIDTH_BASE + (i32::from(age) >> 4)) >> 1
    }
}

/// One drawn half-sheet: the `POLY_FT4` fields `FUN_8003F86C` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FogQuad {
    /// Corners in PSX screen pixels, `POLY_FT4` order: `(x0,y0) (x1,y0)
    /// (x0,y1) (x1,y1)`.
    pub xy: [(i16, i16); 4],
    /// Per-vertex texels from the staged row.
    pub uv: [(u8, u8); 4],
    pub clut: u16,
    pub tpage: u16,
    /// The modulation colour written into the command word.
    pub rgb: [u8; 3],
    /// `view_z >> 5` of the bottom-right point (`SZ2`).
    pub ot_index: u32,
}

/// The camera this frame's fog is projected through - the same
/// view-projection both hosts upload for the scene meshes, so the sheets
/// land where the geometry does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FogView {
    vp: [f32; 16],
}

/// PSX stage size the projection maps NDC back onto.
const STAGE_W: f32 = 320.0;
const STAGE_H: f32 = 240.0;

impl FogView {
    /// From a resolved field camera, at the retail 4:3 aspect (so the NDC to
    /// stage-pixel map is exact).
    pub fn from_field_view(view: &FieldCameraView) -> Self {
        Self::from_vp(view.vp(4.0 / 3.0))
    }

    /// From any column-major view-projection that takes the hosts' Y-up
    /// render frame (raw retail points are Y-negated before multiplying).
    pub fn from_vp(vp: [f32; 16]) -> Self {
        Self { vp }
    }

    /// Project a raw retail world point (Y-down). Returns `(sx, sy, sz)` in
    /// PSX screen pixels + view depth, saturated to the GTE's `SX/SY` range
    /// (`-1024..=1023`); `None` when the point is at or behind the eye, which
    /// the GTE would report as a divide overflow - the caller treats that as
    /// a cull.
    pub fn project(&self, p: [i32; 3]) -> Option<(i32, i32, i32)> {
        let m = &self.vp;
        // Y-up render frame: the hosts' meshes carry the PSX flip in their
        // model matrices; a raw point flips here.
        let v = [p[0] as f32, -(p[1] as f32), p[2] as f32, 1.0];
        let mut clip = [0.0f32; 4];
        for (r, out) in clip.iter_mut().enumerate() {
            *out = m[r] * v[0] + m[4 + r] * v[1] + m[8 + r] * v[2] + m[12 + r] * v[3];
        }
        let w = clip[3];
        if w.is_nan() || w <= 0.5 {
            return None;
        }
        let ndc_x = clip[0] / w;
        let ndc_y = clip[1] / w;
        let sx = ((ndc_x + 1.0) * 0.5 * STAGE_W).round();
        let sy = ((1.0 - ndc_y) * 0.5 * STAGE_H).round();
        Some((
            (sx as i32).clamp(-1024, 1023),
            (sy as i32).clamp(-1024, 1023),
            w.round() as i32,
        ))
    }
}

/// Per-frame inputs of the render step, gathered by [`crate::world::World`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FogFrameEnv {
    /// `DAT_1F800393` - frames since the last rendered frame.
    pub dt: u8,
    /// The player actor's `+0x14 / +0x16 / +0x18`.
    pub player: [i32; 3],
    /// `_DAT_8007B850 & 0xF000 != 0`.
    pub dpad_held: bool,
    /// `_DAT_8007BCAC` - the camera vertical offset the particle Y is
    /// measured against.
    pub y_offset: i32,
    /// `_DAT_8007BCB8..BA` - the global multiply screen tint (op `0x4C`
    /// `0x12`; `0x80` neutral).
    pub tint: [u8; 3],
    /// The walk-region box `0x1F800384..87` as `[x0, z0, x1, z1]` tiles.
    pub window: [u8; 4],
}

/// The pool, its free stack, and the per-frame bookkeeping the spawner reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FogPool {
    pub records: Vec<FogParticle>,
    free_table: Vec<i16>,
    free_top: i16,
    /// `_DAT_8007BCB0`.
    pub cap: u16,
    /// `_DAT_8007BCA8` - live count after the last render step.
    pub live: u16,
    /// MAN section-4 records for the current scene.
    pub regions: Vec<FogRegion>,
    /// `_DAT_8007B854` - the master gate the render pass and the emitter
    /// both test.
    pub gate: bool,
    /// Frame ticks accumulated since the last render step.
    pub pending_dt: u32,
    /// The quads the last render step produced.
    pub quads: Vec<FogQuad>,
}

impl Default for FogPool {
    fn default() -> Self {
        Self::new()
    }
}

impl FogPool {
    /// A freshly allocated pool: every slot free, the stack seeded so the
    /// first pop yields slot 79, nothing live.
    ///
    /// REF: FUN_801D6704 (`0x801D7364..0x801D73D4`, the allocation + seed +
    /// alive-byte clear), FUN_8001FA00 (the seeder)
    pub fn new() -> Self {
        let mut pool = Self {
            records: vec![FogParticle::default(); FOG_POOL_SLOTS],
            free_table: Vec::new(),
            free_top: -1,
            cap: FOG_CAP_DEFAULT,
            live: 0,
            regions: Vec::new(),
            gate: false,
            pending_dt: 0,
            quads: Vec::new(),
        };
        pool.reset();
        pool
    }

    /// Re-seed the free stack and clear every record - what the field reset
    /// does on every scene entry.
    pub fn reset(&mut self) {
        // MAIN_INIT's `jal 0x8001FA00` at `0x801D7384` seeds this stack:
        // `table[i] = i` for `i < n`, then `top = n - 1`.
        let mut seeded = [0u16; FOG_POOL_SLOTS];
        self.free_top =
            crate::scus_leaf_kernels::init_identity_index_list(&mut seeded, FOG_POOL_SLOTS as i16);
        self.free_table = seeded.iter().map(|&i| i as i16).collect();
        for (i, r) in self.records.iter_mut().enumerate() {
            *r = FogParticle {
                slot: i as u8,
                ..FogParticle::default()
            };
        }
        self.live = 0;
        self.pending_dt = 0;
        self.quads.clear();
    }

    /// Slots currently allocated.
    pub fn allocated(&self) -> usize {
        FOG_POOL_SLOTS - (self.free_top + 1).max(0) as usize
    }

    /// Spawn one particle at tile `(tile_x, tile_z)` - `FUN_801D629C`.
    ///
    /// `rand` is the shared `FUN_80056798` stream; this consumes exactly the
    /// draws retail does on the path it takes (two before the slot pop, three
    /// after), so the emitter's own draws stay aligned around it. `trig` is
    /// the LUT pair behind `_DAT_8007B81C` (sine, [`RotationLut::b`]) and
    /// `_DAT_8007B7F8` (cosine, [`RotationLut::a`]).
    ///
    /// The world-map arm (`_DAT_1F800394 & 1`: a camera-space depth test and
    /// a `-0x28` height bias) is not modelled - the field arm is the one the
    /// port's field scenes reach.
    ///
    /// PORT: FUN_801D629C
    ///
    /// WIRED: [`crate::world::World::tick_cutscene_elements`] calls this for
    /// every tile the ambient emitter names, on both hosts.
    pub fn spawn(
        &mut self,
        tile_x: i32,
        tile_z: i32,
        window: [u8; 4],
        trig: &dyn RotationLut,
        rand: &mut dyn FnMut() -> u32,
    ) -> bool {
        // 0x801D62D0..0x801D6318: inside the walk-region box, half-open.
        let [wx0, wz0, wx1, wz1] = window.map(i32::from);
        if tile_x < wx0 || tile_x >= wx1 || tile_z < wz0 || tile_z >= wz1 {
            return false;
        }
        // 0x801D6320..0x801D63B8: first region containing the tile wins;
        // a disabled hit ends the search without a match.
        let Some(region) = self
            .regions
            .iter()
            .find(|r| r.contains(tile_x, tile_z))
            .filter(|r| r.enabled)
            .copied()
        else {
            return false;
        };
        // 0x801D63C0..0x801D63DC: population under the cap.
        if self.live >= self.cap {
            return false;
        }
        // 0x801D6404..0x801D6440: angle = base*16 + ((rand & 0xFFF) *
        // spread*16 >> 12), masked to the LUT.
        let r1 = rand() & 0xFFF;
        let base = i32::from(region.angle_base) << 4;
        let spread = i32::from(region.angle_spread) << 4;
        let angle = ((base + ((r1 as i32 * spread) >> 12)) & 0xFFF) as usize;
        // 0x801D6444..0x801D6458: height, negative is up.
        let y = -((rand() & 0x7F) as i16);
        // 0x801D647C..0x801D649C: pop a slot; none -> nothing spawns.
        let Some(slot) = crate::cutscene::sprite_stack_pop(&mut self.free_top, &self.free_table)
        else {
            return false;
        };
        let Some(rec) = self.records.get_mut(slot as usize) else {
            return false;
        };
        let grey = (rand() & 0x7F) as u8;
        let rate = ((rand() & 7) + 8) as u16;
        // 0x801D6540: one more draw whose result nothing reads.
        let _ = rand();
        let speed = i32::from(region.speed);
        let s = trig.b(angle as i32);
        let c = trig.a(angle as i32);
        *rec = FogParticle {
            age: 0,
            rate,
            slot: slot as u8,
            alive: true,
            vx: (-((s * speed) >> 14)) as i8,
            vz: (-((c * speed) >> 14)) as i8,
            x: (tile_x << 7) << 4,
            z: (tile_z << 7) << 4,
            y,
            grey,
        };
        true
    }

    /// Return a record's slot to the free stack (`FUN_8001FA68` at
    /// `0x8003F800`).
    fn free_slot(&mut self, slot: u8) {
        crate::cutscene::sprite_stack_push(&mut self.free_top, &mut self.free_table, slot as i16);
    }

    /// One rendered frame of the pool - `FUN_8003F348` walking every slot and
    /// `FUN_8003F3FC` on each live one. Fills [`Self::quads`], updates
    /// [`Self::live`], and frees the records that died.
    ///
    /// The caller has already applied the pass's own gate (`game mode 3` and
    /// `_DAT_8007B854`); this does the walk unconditionally.
    ///
    /// PORT: FUN_8003F348, FUN_8003F3FC
    ///
    /// WIRED: [`crate::world::World::fog_render_step`] on both hosts.
    pub fn render_step(&mut self, view: &FogView, env: &FogFrameEnv) -> &[FogQuad] {
        self.quads.clear();
        let mut live = 0u16;
        let dt = i32::from(env.dt);
        let [wx0, wz0, wx1, wz1] = env.window.map(|b| i32::from(b) << 7);
        for i in 0..self.records.len() {
            let mut rec = self.records[i];
            if !rec.alive {
                continue;
            }
            // 0x8003F440..0x8003F4C8: pre-update position against the box.
            let sx = rec.x >> 4;
            let sz = rec.z >> 4;
            let sy = i32::from(rec.y) - env.y_offset;
            let inside = sx >= wx0 && sx < wx1 && sz >= wz0 && sz < wz1;
            let mut alive = inside;
            let mut rgb = [0u8; 3];
            if alive {
                // 0x8003F4F4..0x8003F5DC: brightness, then the tinted grey.
                let bright = match FogParticle::brightness(rec.age) {
                    Some(b) => b,
                    None => {
                        alive = false;
                        0
                    }
                };
                for (k, c) in rgb.iter_mut().enumerate() {
                    let v = i32::from(rec.grey) * i32::from(env.tint[k]) * bright;
                    *c = (v >> 15) as u8;
                }
                // 0x8003F5E0..0x8003F648: drift + age.
                rec.x += i32::from(rec.vx) * dt;
                rec.z += i32::from(rec.vz) * dt;
                let step = i32::from(rec.rate) * dt;
                let mut age = i32::from(rec.age) + step;
                // 0x8003F64C..0x8003F710: the player's box ages it again,
                // three times more while the d-pad is held.
                let [px, py, pz] = env.player;
                let [bx, by, bz] = FOG_PLAYER_BOX;
                if sx - bx < px
                    && px < sx + bx
                    && sz - bz < pz
                    && pz < sz + bz
                    && sy - by < py
                    && py < sy + by
                {
                    age += step;
                    if env.dpad_held {
                        age += step * 3;
                    }
                }
                rec.age = age as u16;
            }
            if alive {
                // 0x8003F720..0x8003F7DC: the two halves; the particle stays
                // alive while either one is on screen.
                let hw = FogParticle::half_width(rec.age);
                let p = [sx, sy, sz];
                let (l_alive, l_quad) = emit_half(view, p, -2 * hw, 0, FOG_LEFT_UV_ROW, rgb);
                let (r_alive, r_quad) = emit_half(view, p, 0, 2 * hw, FOG_RIGHT_UV_ROW, rgb);
                self.quads.extend(l_quad);
                self.quads.extend(r_quad);
                alive = l_alive || r_alive;
            }
            rec.alive = alive;
            self.records[i] = rec;
            if alive {
                live += 1;
            } else {
                self.free_slot(rec.slot);
            }
        }
        self.live = live;
        &self.quads
    }
}

/// One half-sheet - `FUN_8003F86C`.
///
/// `p` is the particle's pre-update world position; the half spans world X
/// `p.x + dx0 ..= p.x + dx1` and rises [`FOG_SHEET_HEIGHT`] above `p`. The
/// bool is what the routine returns (`1` = keep the particle), the quad is
/// present only when it drew.
///
/// PORT: FUN_8003F86C
fn emit_half(
    view: &FogView,
    p: [i32; 3],
    dx0: i32,
    dx1: i32,
    row: usize,
    rgb: [u8; 3],
) -> (bool, Option<FogQuad>) {
    let top_left = [p[0] + dx0, p[1] - FOG_SHEET_HEIGHT, p[2]];
    let bottom_right = [p[0] + dx1, p[1], p[2]];
    let (Some((x0, y0, _)), Some((x1, y1, z1))) =
        (view.project(top_left), view.project(bottom_right))
    else {
        return (false, None);
    };
    // 0x8003F91C..0x8003F954: both left of -8, or both at/right of 0x148.
    if x0 + 8 <= 0 && x1 + 8 <= 0 {
        return (false, None);
    }
    if x0 >= 0x148 && x1 >= 0x148 {
        return (false, None);
    }
    // 0x8003F9A4..0x8003F9D0: both above the top, or both below 0x190.
    if y0 <= 0 && y1 < 0 {
        return (false, None);
    }
    if y0 >= 0x190 && y1 > 0x190 {
        return (false, None);
    }
    // 0x8003F9D4..0x8003F9E4: below the visible 240 rows - kept, not drawn.
    if y0 >= 0xF0 && y1 > 0xF0 {
        return (true, None);
    }
    // 0x8003FA14..0x8003FA30: NCLIP over (x0,y0) (x1,y1) (x1,y0).
    let mac0 = x0 * y1 + x1 * y0 - x0 * y0 - x1 * y1;
    if (mac0 >> 2) + FOG_NCLIP_TOLERANCE <= 0 {
        return (false, None);
    }
    let words = FOG_UV_ROWS[row];
    let uv = words.map(|w| ((w & 0xFF) as u8, ((w >> 8) & 0xFF) as u8));
    let c = |v: i32| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    (
        true,
        Some(FogQuad {
            xy: [
                (c(x0), c(y0)),
                (c(x1), c(y0)),
                (c(x0), c(y1)),
                (c(x1), c(y1)),
            ],
            uv,
            clut: (words[0] >> 16) as u16,
            tpage: (words[1] >> 16) as u16,
            rgb,
            ot_index: (z1.max(0) >> 5) as u32,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lut() -> &'static crate::action_effect_script::RetailRotationLut {
        crate::action_effect_script::retail_rotation_lut()
    }

    /// A view that looks straight down -Z... expressed as the identity-ish
    /// projection: `sx = 160 + x * 512 / z`, `sy = 120 - y * 512 / z` for a
    /// Y-up render frame (the model negates raw Y before multiplying).
    fn plain_view() -> FogView {
        let h = 512.0f32;
        // column-major: clip.x = h/160 * x; clip.y = h/120 * y; clip.w = z.
        let mut m = [0.0f32; 16];
        m[0] = h / 160.0;
        m[5] = h / 120.0;
        m[11] = 1.0;
        m[10] = 1.0;
        FogView::from_vp(m)
    }

    fn region() -> FogRegion {
        FogRegion {
            enabled: true,
            x0: 0,
            z0: 0,
            x1: 0x7F,
            z1: 0x7F,
            angle_base: 0,
            angle_spread: 0,
            speed: 0x40,
            byte_8: 0,
            flag_index: 0,
        }
    }

    #[test]
    fn region_table_parses_count_prefixed_records() {
        let mut body = vec![2u8];
        body.extend_from_slice(&[1, 2, 3, 40, 50, 0x10, 0x20, 0x30, 0, 0x34, 0x12]);
        body.extend_from_slice(&[0, 5, 6, 7, 8, 0, 0, 0, 0, 0, 0]);
        let regs = parse_fog_regions(&body);
        assert_eq!(regs.len(), 2);
        assert!(regs[0].enabled);
        assert_eq!(
            (regs[0].x0, regs[0].z0, regs[0].x1, regs[0].z1),
            (2, 3, 40, 50)
        );
        assert_eq!(regs[0].flag_index, 0x1234);
        assert!(!regs[1].enabled);
        // Strict on every side.
        assert!(regs[0].contains(3, 4));
        assert!(!regs[0].contains(2, 4));
        assert!(!regs[0].contains(40, 4));
    }

    #[test]
    fn free_stack_pops_the_highest_slot_first_and_refills_lifo() {
        let trig = lut();
        let mut pool = FogPool::new();
        pool.regions = vec![region()];
        let mut rand = || 0u32;
        assert!(pool.spawn(10, 10, [0, 0, 0x7F, 0x7F], trig, &mut rand));
        assert_eq!(pool.allocated(), 1);
        assert!(pool.records[FOG_POOL_SLOTS - 1].alive);
        assert_eq!(pool.records[FOG_POOL_SLOTS - 1].x, 10 << 11);
        assert_eq!(pool.records[FOG_POOL_SLOTS - 1].rate, 8);
    }

    #[test]
    fn spawn_gates_on_window_region_and_cap() {
        let trig = lut();
        let mut pool = FogPool::new();
        let mut rand = || 0u32;
        // No region table at all.
        assert!(!pool.spawn(10, 10, [0, 0, 0x7F, 0x7F], trig, &mut rand));
        pool.regions = vec![FogRegion {
            enabled: false,
            ..region()
        }];
        // A disabled hit ends the search.
        assert!(!pool.spawn(10, 10, [0, 0, 0x7F, 0x7F], trig, &mut rand));
        pool.regions = vec![region()];
        // Outside the walk-region window (half-open upper bound).
        assert!(!pool.spawn(0x7F, 10, [0, 0, 0x7F, 0x7F], trig, &mut rand));
        assert!(!pool.spawn(10, 10, [20, 0, 0x7F, 0x7F], trig, &mut rand));
        // Cap: the spawner reads the last render step's live count.
        pool.live = FOG_CAP_DEFAULT;
        assert!(!pool.spawn(10, 10, [0, 0, 0x7F, 0x7F], trig, &mut rand));
        pool.live = 0;
        assert!(pool.spawn(10, 10, [0, 0, 0x7F, 0x7F], trig, &mut rand));
    }

    #[test]
    fn spawn_consumes_five_random_draws_on_the_full_path() {
        let trig = lut();
        let mut pool = FogPool::new();
        pool.regions = vec![region()];
        let mut n = 0;
        let mut rand = || {
            n += 1;
            0x55u32
        };
        assert!(pool.spawn(10, 10, [0, 0, 0x7F, 0x7F], trig, &mut rand));
        assert_eq!(n, 5);
        // Two draws happen before the window/region gates can be passed and
        // the pop; none when the window rejects the tile.
        let mut m = 0;
        let mut rand2 = || {
            m += 1;
            0u32
        };
        assert!(!pool.spawn(-1, 10, [0, 0, 0x7F, 0x7F], trig, &mut rand2));
        assert_eq!(m, 0);
    }

    #[test]
    fn velocity_follows_the_region_angle_and_speed() {
        let trig = lut();
        let mut pool = FogPool::new();
        pool.regions = vec![FogRegion {
            angle_base: 0x40, // 0x400 = a quarter turn: sin 4096, cos 0
            ..region()
        }];
        let mut rand = || 0u32;
        assert!(pool.spawn(10, 10, [0, 0, 0x7F, 0x7F], trig, &mut rand));
        let r = pool.records[FOG_POOL_SLOTS - 1];
        assert_eq!(r.vx, -((4096 * 0x40) >> 14) as i8);
        assert_eq!(r.vz, 0);
    }

    #[test]
    fn brightness_ramps_and_kills_past_the_tail() {
        assert_eq!(FogParticle::brightness(0), Some(0));
        assert_eq!(FogParticle::brightness(0x3FF), Some(0xFF));
        assert_eq!(FogParticle::brightness(0x800), Some(0xFF));
        assert_eq!(FogParticle::brightness(0xC00), Some(0xFF));
        assert_eq!(FogParticle::brightness(0xC04), Some(0xFE));
        assert_eq!(FogParticle::brightness(0xC00 + 0x3FC), Some(0));
        assert_eq!(FogParticle::brightness(0x1000), None);
    }

    #[test]
    fn half_width_has_no_random_term() {
        assert_eq!(FogParticle::half_width(0), 0xC0);
        assert_eq!(FogParticle::half_width(0x1000), 0xC0 + 0x80);
    }

    fn env() -> FogFrameEnv {
        FogFrameEnv {
            dt: 1,
            player: [-10_000, 0, -10_000],
            dpad_held: false,
            y_offset: 0,
            tint: [0x80; 3],
            window: [0, 0, 0x7F, 0x7F],
        }
    }

    #[test]
    fn render_step_emits_two_quads_per_live_particle_with_retail_packet_fields() {
        let trig = lut();
        let mut pool = FogPool::new();
        pool.regions = vec![region()];
        let mut rand = || 0x7Fu32;
        assert!(pool.spawn(10, 10, [0, 0, 0x7F, 0x7F], trig, &mut rand));
        // Put the particle in front of a camera at the origin looking down
        // +Z: the plain view is `sx = 160 + x*512/z`.
        let slot = FOG_POOL_SLOTS - 1;
        pool.records[slot].x = 0;
        pool.records[slot].z = 2000 << 4;
        pool.records[slot].age = 0x800;
        let quads = pool.render_step(&plain_view(), &env()).to_vec();
        assert_eq!(quads.len(), 2);
        for q in &quads {
            assert_eq!(q.tpage, FOG_TPAGE);
            assert_eq!(q.clut, FOG_CLUT);
            assert_eq!(q.ot_index, 2000 >> 5);
            // Axis-aligned rectangle in POLY_FT4 order.
            assert_eq!(q.xy[0].1, q.xy[1].1);
            assert_eq!(q.xy[2].1, q.xy[3].1);
            assert_eq!(q.xy[0].0, q.xy[2].0);
            assert_eq!(q.xy[1].0, q.xy[3].0);
        }
        // Left half samples row 1 (v 0x58..), right half row 0 (v 0x40..).
        assert_eq!(quads[0].uv[0], (0x00, 0x58));
        assert_eq!(quads[1].uv[0], (0x00, 0x40));
        // Left half ends at the particle's column, right half starts there.
        assert_eq!(quads[0].xy[1].0, 160);
        assert_eq!(quads[1].xy[0].0, 160);
        // Sheet rises above the particle: the top row is smaller than the
        // bottom row in screen space.
        assert!(quads[0].xy[0].1 < quads[0].xy[2].1);
        // Grey 0x7F * tint 0x80 * bright 0xFF >> 15 = 126.
        assert_eq!(quads[0].rgb, [126; 3]);
        assert_eq!(pool.live, 1);
        assert!(pool.records[slot].alive);
        // `rand() == 0x7F` seeds rate `(0x7F & 7) + 8 = 15`.
        assert_eq!(pool.records[slot].rate, 15);
        assert_eq!(pool.records[slot].age, 0x800 + 15);
    }

    #[test]
    fn render_step_kills_off_window_aged_out_and_offscreen_particles() {
        let trig = lut();
        let mut pool = FogPool::new();
        pool.regions = vec![region()];
        let mut rand = || 0u32;
        for _ in 0..3 {
            assert!(pool.spawn(10, 10, [0, 0, 0x7F, 0x7F], trig, &mut rand));
        }
        let a = FOG_POOL_SLOTS - 1;
        let b = FOG_POOL_SLOTS - 2;
        let c = FOG_POOL_SLOTS - 3;
        // a: outside the window (window shrinks to tiles 20..).
        pool.records[a].x = 0;
        pool.records[a].z = 2000 << 4;
        // b: aged past the tail.
        pool.records[b].z = 2000 << 4;
        pool.records[b].x = 30 << 11;
        pool.records[b].age = 0x1010;
        // c: behind the eye.
        pool.records[c].x = 30 << 11;
        pool.records[c].z = -(2000 << 4);
        let mut e = env();
        e.window = [20, 0, 0x7F, 0x7F];
        let quads = pool.render_step(&plain_view(), &e).to_vec();
        assert!(quads.is_empty());
        assert_eq!(pool.live, 0);
        assert_eq!(
            pool.allocated(),
            0,
            "every dead record went back on the stack"
        );
    }

    #[test]
    fn player_proximity_ages_faster_and_the_dpad_more_so() {
        let trig = lut();
        let mut pool = FogPool::new();
        pool.regions = vec![region()];
        let mut rand = || 0u32;
        assert!(pool.spawn(10, 10, [0, 0, 0x7F, 0x7F], trig, &mut rand));
        let slot = FOG_POOL_SLOTS - 1;
        pool.records[slot].x = 0;
        pool.records[slot].z = 2000 << 4;
        let mut e = env();
        e.player = [0, 0, 2000];
        pool.render_step(&plain_view(), &e);
        assert_eq!(pool.records[slot].age, 16);
        e.dpad_held = true;
        pool.render_step(&plain_view(), &e);
        assert_eq!(pool.records[slot].age, 16 + 8 * 5);
    }

    #[test]
    fn below_the_screen_keeps_the_particle_without_drawing() {
        let trig = lut();
        let mut pool = FogPool::new();
        pool.regions = vec![region()];
        let mut rand = || 0u32;
        assert!(pool.spawn(10, 10, [0, 0, 0x7F, 0x7F], trig, &mut rand));
        let slot = FOG_POOL_SLOTS - 1;
        pool.records[slot].x = 0;
        pool.records[slot].z = 2000 << 4;
        // Raw Y-down: +y is down; put it well below the frame but under 0x190.
        pool.records[slot].y = 800;
        let quads = pool.render_step(&plain_view(), &env()).to_vec();
        assert!(quads.is_empty());
        assert_eq!(pool.live, 1);
    }
}
