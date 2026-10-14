//! The rod actor and the fishing line: bend, transform, face build and the per-frame drive.

use super::*;

// --- The rod actor (FUN_801D1C5C) and the fishing line ---------------------

/// Scene-bank index of rod `0` in the venue scene (`other1`).
///
/// The cast arm spawns the rod actor from the template at `0x801D8FDC` after
/// writing its model word as `_DAT_8007B6F8 + _DAT_80084454 + 0x19`
/// (`0x801CFC34..0x801CFC4C`): the scene bank's base plus the persistent rod
/// index plus this constant, so rod `r` is scene model `0x19 + r`.
pub const ROD_MODEL_BASE: i16 = 0x19;

/// Vertex of the rod mesh's object 0 that is the line's rod end.
///
/// `FUN_801D1C5C` projects `+0x128` into the object's staged vertex array
/// (`addiu a0,s0,0x128` in the delay slot at `0x801D1FB8`), eight bytes a
/// vertex: vertex 37, the centre of the rod's last ring.
pub const ROD_TIP_VERTEX: usize = 0x128 / 8;

/// The VDF sub-entry the rod bends by: the actor's one morph slot names it
/// (`sb zero,0xb0(s1)` at `0x801D1EDC`, slot count `1` at `+0x6C`) and the
/// stager `FUN_8001C604` is asked for TMD group `0` (`clear a1` at
/// `0x801D1F98`).
pub const ROD_BEND_VDF_ENTRY: u8 = 0;

/// The rod actor's `+0x14 / +0x16 / +0x18` position, written at spawn
/// (`0x801CFC60..0x801CFC80`). It is a **view-space** point: the actor
/// composes its own matrix rather than the scene camera's.
pub const ROD_ACTOR_POS: [i16; 3] = [0, 0x46, 0x64];

/// The diagonal of the matrix the rod composes on: `FUN_801D1C5C` copies the
/// per-mode base matrix `0x8007BF10` to the scratchpad and stores `0x6000` into
/// its three diagonal halfwords (`0x801D1E28..0x801D1E38`). The copied base
/// is itself `0x6000 * I` with a zero translation in the fishing mode (read
/// off the `minigame_fishing` save state), so the rotation is a pure 6x scale.
pub const ROD_BASE_SCALE: i32 = 0x6000;

/// GTE `H` the rod projects under (`li a0,0xdc` into `FUN_8003D254` at
/// `0x801D1FA4`); the scene's own `_DAT_8007B6F4` is restored after the draw.
pub const ROD_PROJECTION_H: u16 = 0xDC;

/// The GTE screen centre `(OFX, OFY)` in whole pixels - global for the whole
/// game ([`legaia_engine_camera::projection::GTE_OFY`]); the fishing save
/// state carries the same pair.
pub const GTE_SCREEN_CENTRE: (i32, i32) = (160, 114);

/// The draw-window bounds [`clip_segment_2d`] reads for the line: the
/// scratchpad halfwords `0x1F800388..0x1F80038E`, read off the
/// `minigame_fishing` save state. Retail's drawing area is `320 x 224` from
/// row `4`, which is where the `4` / `0xE4` come from.
pub const LINE_CLIP_RECT: ClipRect = ClipRect {
    x_min: 0,
    y_min: 4,
    x_max: 0x140,
    y_max: 0xE4,
};

/// The line packet's colour at its **fish** end (`+0x04`, the low three bytes
/// of `0x50303030` at `0x801D3A28`). The packet is `LINE_G2` (`0x50`): a
/// Gouraud line, opaque.
pub const LINE_FISH_RGB: [u8; 3] = [0x30, 0x30, 0x30];

/// The line packet's colour at its **rod** end (`+0x0C..+0x0E`, three `0x80`
/// byte stores at `0x801D3A50..0x801D3A58`).
pub const LINE_ROD_RGB: [u8; 3] = [0x80, 0x80, 0x80];

/// The ordering-table shift the line links at: the rod tip's projected depth
/// `>> (0x1F8003A4 + 2)` (`0x801D3D18..0x801D3D24`), the scratchpad byte
/// being `3` in the fishing save state.
pub const LINE_OT_SHIFT: u32 = 3 + 2;

/// Per-frame-delta bob step of the rod's cast / recover swing (`sll v1,v1,0x6`
/// at `0x801D1D00` / `0x801D1D40`).
pub const ROD_BOB_STEP: i32 = 0x40;

/// Where the cast swing stops (`slti v0,v0,-0x2bc` at `0x801D1D14`).
pub const ROD_BOB_FLOOR: i16 = -0x2BC;

/// Where the recover swing stops (`slti v0,v0,0x401` at `0x801D1D54`).
pub const ROD_BOB_CEILING: i16 = 0x400;

/// The bend the cast lock seeds (`li v0,0x1000` stored to `0x801D9150` at
/// `0x801CFC2C`).
pub const ROD_BEND_AT_CAST: i32 = 0x1000;

/// The rod actor's sub-state (`0x801D91AC`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RodSwing {
    /// `1`: the cast swing, dipping the rod to [`ROD_BOB_FLOOR`].
    Cast,
    /// `2`: holding the cast pose while the line is out.
    Hold,
    /// `10`: the recover swing back up to [`ROD_BOB_CEILING`] after a landed
    /// catch or a snapped line (`0x801D3CB4` / `0x801D3C44`).
    Recover,
    /// `0x14`: the recover finished; the actor retires (`ori v0,v0,0x8`).
    Done,
}

/// One primitive of a rod model's object 0: the untextured flat / Gouraud
/// triangles and quads the three rods are built from (TMD groups of kinds
/// `12..=15`, the `FUN_80043390` bank-0 handlers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RodPrim {
    /// Vertex indices into object 0's (staged) vertex array; a triangle
    /// repeats its third index in the fourth slot.
    pub verts: [u16; 4],
    /// `true` for a quad (the group's `flags` bit 1).
    pub quad: bool,
    /// Per-vertex packet colour, in [`Self::verts`] order. A flat primitive
    /// carries its one colour word on every vertex.
    pub rgb: [[u8; 3]; 4],
}

/// The rod geometry the line's rod end is a point of, lifted off the venue
/// scene's bank.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RodMesh {
    /// Object 0's vertex bytes (eight per vertex) of rods `0..=2`, scene
    /// models `0x19..=0x1B`.
    pub rods: [Option<Vec<u8>>; 3],
    /// The bend: VDF sub-entry [`ROD_BEND_VDF_ENTRY`] of the scene's
    /// type-7 buffer.
    pub bend: Option<Vec<u8>>,
    /// Object 0's primitives of rods `0..=2`, in packet order - what
    /// `FUN_801D1C5C` hands the per-primitive dispatcher `FUN_80043390`
    /// (`jal` at `0x801D1FF4`). Empty for a rod whose model did not resolve
    /// or carried no untextured primitive.
    pub prims: [Vec<RodPrim>; 3],
}

/// The rod's primitives off a parsed model: object 0's untextured groups.
/// A textured group (none of the venue's three rods carries one) is skipped
/// rather than drawn without its texture.
fn rod_prims(bytes: &[u8], tmd: &legaia_tmd::Tmd) -> Vec<RodPrim> {
    let Some(obj) = tmd.objects.first() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for g in legaia_tmd::legaia_prims::iter_groups_lenient(
        bytes,
        obj.primitives_byte_offset,
        obj.primitives_byte_size,
    ) {
        let quad = g.header.n_vertices() == 4;
        for p in &g.prims {
            if !p.uvs.is_empty() {
                continue;
            }
            let idx = p.vertex_indices();
            let n = idx.len().min(4);
            if n < 3 || p.colors.len() < n {
                continue;
            }
            let mut verts = [0u16; 4];
            let mut rgb = [[0u8; 3]; 4];
            for i in 0..4 {
                let k = i.min(n - 1);
                verts[i] = idx[k];
                rgb[i] = p.colors[k];
            }
            out.push(RodPrim {
                verts,
                quad: quad && n == 4,
                rgb,
            });
        }
    }
    out
}

impl RodMesh {
    /// Lift the three rods out of their TMD bytes (scene models
    /// [`ROD_MODEL_BASE`]`..+3`, in rod order) and take the bend as given
    /// (VDF sub-entry [`ROD_BEND_VDF_ENTRY`] of the venue's type-7 buffer).
    /// `None` when not one rod model resolves. `engine-core`'s
    /// `fishing_actors::rod_mesh_from_scene` gathers both out of a loaded
    /// venue scene.
    pub fn from_models(models: [Option<Vec<u8>>; 3], bend: Option<Vec<u8>>) -> Option<Self> {
        let mut rods: [Option<Vec<u8>>; 3] = Default::default();
        let mut prims: [Vec<RodPrim>; 3] = Default::default();
        for (r, (slot, model)) in rods.iter_mut().zip(models).enumerate() {
            let Some(bytes) = model else {
                continue;
            };
            let Ok(tmd) = legaia_tmd::parse(&bytes) else {
                continue;
            };
            prims[r] = rod_prims(&bytes, &tmd);
            *slot = tmd.objects.first().map(|o| {
                o.vertices
                    .iter()
                    .flat_map(|v| {
                        [v.x, v.y, v.z, v._pad]
                            .into_iter()
                            .flat_map(i16::to_le_bytes)
                    })
                    .collect()
            });
        }
        if rods.iter().all(Option::is_none) {
            return None;
        }
        Some(Self { rods, bend, prims })
    }

    /// The tip vertex of rod `rod` bent by `weight` - the morph stager
    /// `FUN_8001C604` over group 0 at that slot weight, then vertex
    /// [`ROD_TIP_VERTEX`] of the staged array.
    pub fn tip(&self, rod: usize, weight: i16) -> Option<[i16; 3]> {
        self.staged(rod, weight)?.get(ROD_TIP_VERTEX).copied()
    }

    /// Every vertex of rod `rod`'s object 0 after the morph stager
    /// `FUN_8001C604` bent it by `weight` - the array the draw and the tip
    /// projection both read.
    pub fn staged(&self, rod: usize, weight: i16) -> Option<Vec<[i16; 3]>> {
        let rest = self.rods.get(rod)?.as_deref()?;
        let staged = match self.bend.as_deref() {
            Some(entry) => {
                legaia_scene_formats::vdf_morph::stage_group_morph(rest, 0, &[(entry, weight)])
            }
            None => rest.to_vec(),
        };
        let h = |v: &[u8; 8], i: usize| i16::from_le_bytes([v[i], v[i + 1]]);
        Some(
            staged
                .as_chunks::<8>()
                .0
                .iter()
                .map(|v| [h(v, 0), h(v, 2), h(v, 4)])
                .collect(),
        )
    }
}

/// The projected rod tip: the `0x801D9194` screen pair and the depth
/// `FUN_8003D368` returns (`IR3`, kept at `0x801D913C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RodTip {
    pub sxy: (i16, i16),
    pub depth: i32,
}

/// One `MVMVA` (`sf = 1`, `lm = 0`, `cv` = a zeroed `TR`): `m * v >> 12`,
/// each component saturated to `i16`.
fn mvmva(m: &[[i32; 3]; 3], v: [i32; 3]) -> [i32; 3] {
    let row = |r: usize| {
        let mac = (m[r][0] as i64 * v[0] as i64
            + m[r][1] as i64 * v[1] as i64
            + m[r][2] as i64 * v[2] as i64)
            >> 12;
        mac.clamp(-0x8000, 0x7FFF) as i32
    };
    [row(0), row(1), row(2)]
}

/// Post-multiply `m` by one axis rotation the way the SCUS builders
/// `FUN_800461A4` / `FUN_8004629C` / `FUN_8004638C` (`RotMatrixX / Y / Z`) do:
/// each new column is one `MVMVA` of the current matrix against that column
/// of the axis rotation, read out of the retail sine table.
fn rotate(m: &mut [[i32; 3]; 3], axis: usize, angle: i32) {
    use legaia_minigame_formats::minigame_slot_scene::{cos_4096, sin_4096};
    let a = angle & 0xFFF;
    let (s, c) = (sin_4096(a), cos_4096(a));
    let one = 0x1000;
    let cols: [[i32; 3]; 3] = match axis {
        0 => [[one, 0, 0], [0, c, s], [0, -s, c]],
        1 => [[c, 0, -s], [0, one, 0], [s, 0, c]],
        _ => [[c, s, 0], [-s, c, 0], [0, 0, one]],
    };
    let out = cols.map(|col| mvmva(m, col));
    for (ci, col) in out.iter().enumerate() {
        for r in 0..3 {
            m[r][ci] = col[r];
        }
    }
}

/// The rod's GTE state for one frame: the rotation `FUN_801D1C5C` loads
/// (`SetRotMatrix` at `0x801D1EB4`) and its translation.
///
/// The matrix is the 6x base rotated by `RotMatrixX(rot_x)`, `RotMatrixY(0)`,
/// `RotMatrixZ(rot_z)` and `RotMatrixY(-yaw)` in that order
/// (`0x801D1E80..0x801D1EA8`); the translation is [`ROD_ACTOR_POS`] pushed
/// through the base (`FUN_8003D344` at `0x801D1E40`, then `SetTransMatrix`).
fn rod_transform(rot_x: i16, rot_z: i16, yaw: i32) -> ([[i32; 3]; 3], [i32; 3]) {
    let s = ROD_BASE_SCALE;
    let base = [[s, 0, 0], [0, s, 0], [0, 0, s]];
    let tr = mvmva(&base, ROD_ACTOR_POS.map(i32::from));
    let mut m = base;
    rotate(&mut m, 0, rot_x as i32);
    rotate(&mut m, 1, 0);
    rotate(&mut m, 2, rot_z as i32);
    rotate(&mut m, 1, yaw.wrapping_neg());
    (m, tr)
}

/// One `RTPS` (`sf = 1`, `lm = 0`) under [`ROD_PROJECTION_H`] with the UNR
/// divide: the screen pair, `IR3` and `SZ3`.
fn rod_rtps(m: &[[i32; 3]; 3], tr: [i32; 3], v: [i16; 3]) -> ((i16, i16), i32, i32) {
    // MAC = (TR << 12 + R * V) >> 12.
    let mac = |r: usize| {
        ((tr[r] as i64) << 12)
            + m[r][0] as i64 * v[0] as i64
            + m[r][1] as i64 * v[1] as i64
            + m[r][2] as i64 * v[2] as i64
    };
    let ir = |r: usize| (mac(r) >> 12).clamp(-0x8000, 0x7FFF);
    let sz3 = (mac(2) >> 12).clamp(0, 0xFFFF) as u16;
    let (div, _) = legaia_render_kernels::gte::divide::gte_divide(ROD_PROJECTION_H, sz3);
    let screen =
        |ir: i64, of: i32| (((div * ir) + ((of as i64) << 16)) >> 16).clamp(-0x400, 0x3FF) as i16;
    (
        (
            screen(ir(0), GTE_SCREEN_CENTRE.0),
            screen(ir(1), GTE_SCREEN_CENTRE.1),
        ),
        ir(2) as i32,
        i32::from(sz3),
    )
}

/// Project the rod-tip vertex `v` the way `FUN_801D1C5C` does: the rod's
/// matrix ([`rod_transform`]), then `RTPS` under [`ROD_PROJECTION_H`] with
/// the UNR divide.
pub fn rod_tip_screen(v: [i16; 3], rot_x: i16, rot_z: i16, yaw: i32) -> RodTip {
    let (m, tr) = rod_transform(rot_x, rot_z, yaw);
    let (sxy, depth, _) = rod_rtps(&m, tr, v);
    RodTip { sxy, depth }
}

/// The pose one rod-actor tick drew with: the two rotation terms it built
/// this frame, the yaw it read, and the morph weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RodPose {
    /// `RotMatrixX`'s angle: the swing plus the lift / bend pitch.
    pub rot_x: i16,
    /// `RotMatrixZ`'s angle: twice the lean.
    pub rot_z: i16,
    /// `0x801D911C`, negated into the last `RotMatrixY`.
    pub yaw: i32,
    /// The morph slot's weight - the bend's low halfword.
    pub weight: i16,
}

/// `ZSF3` the rod's triangles average under: the dispatcher loads
/// `0x555 >> _DAT_1F8003A4` (`0x80043568..0x8004357C`), the scratch byte
/// being `3` in the `minigame_fishing` save state.
pub const ROD_ZSF3: i32 = 0x555 >> 3;

/// `ZSF4` the rod's quads average under (`0x400 >> _DAT_1F8003A4`, same
/// load). With it the ordering-table bucket `OTZ >> 2` is the mean depth
/// `/ 32` - the scale the line's `IR3 >> 5` links at.
pub const ROD_ZSF4: i32 = 0x400 >> 3;

/// The handlers' near cutoff: a primitive whose `OTZ` falls below the
/// scratch halfword `0x1F80037E` is dropped (`sub s1,s2,t4; bltz` at
/// `0x80043870` / `0x80043720`). `0x10` in the `minigame_fishing` state.
pub const ROD_NEAR_OTZ: i32 = 0x10;

/// One rod primitive as retail links it: screen corners, per-corner colour
/// and the ordering-table bucket. A triangle repeats its third corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RodFace {
    pub xy: [(i16, i16); 4],
    pub rgb: [[u8; 3]; 4],
    pub quad: bool,
    /// `OTZ >> 2` - the word index `(OTZ & 0xFFFC)` addresses in the OT.
    pub ot: u32,
}

/// `NCLIP`: the signed area of the screen triangle `(a, b, c)`.
fn nclip(a: (i16, i16), b: (i16, i16), c: (i16, i16)) -> i64 {
    let (ax, ay) = (a.0 as i64, a.1 as i64);
    let (bx, by) = (b.0 as i64, b.1 as i64);
    let (cx, cy) = (c.0 as i64, c.1 as i64);
    ax * by + bx * cy + cx * ay - ax * cy - bx * ay - cx * by
}

/// The rod model drawn: object 0 of rod `rod`, bent by the pose's morph
/// weight, every vertex through the rod's matrix under `H = 0xDC`, then each
/// primitive through the bank-0 handler of its kind the way
/// `FUN_80043390` runs them for this call (the rod's `+0x74` is the
/// allocator's `0x00808080` - `sw v1,0x74(s0)` at `0x80020F3C` - and its
/// `+0x78` is `0`, so no blend bank, no depth cue and the single-sided
/// cull mask `0xFFFFFFFF`):
///
/// - a **triangle** (kinds 12 / 14, `0x80043658` / `0x80043B58`) is culled
///   when `NCLIP < 0` (`bltz` at `0x80043700`);
/// - a **quad** (kinds 13 / 15, `0x80043768` / `0x80043C6C`) is kept when
///   `NCLIP(v0, v1, v2) > 0`, else only when the second `NCLIP` over
///   `(v1, v2, v3)` after the fourth `RTPS` is negative
///   (`blez` at `0x80043818`, `bgez` at `0x8004384C`);
/// - `AVSZ3` / `AVSZ4` under [`ROD_ZSF3`] / [`ROD_ZSF4`] give `OTZ`, a
///   primitive below [`ROD_NEAR_OTZ`] is dropped, and the packet links at
///   `OTZ >> 2`.
///
/// Faces come back in packet order - the order the handlers `AddPrim` them.
///
/// PORT: FUN_801d1c5c (the model draw: the `FUN_80043390` call at
/// `0x801D1FF4` over the staged object 0)
pub fn rod_faces(mesh: &RodMesh, rod: usize, pose: RodPose) -> Vec<RodFace> {
    let Some(verts) = mesh.staged(rod, pose.weight) else {
        return Vec::new();
    };
    let Some(prims) = mesh.prims.get(rod) else {
        return Vec::new();
    };
    let (m, tr) = rod_transform(pose.rot_x, pose.rot_z, pose.yaw);
    let projected: Vec<((i16, i16), i32)> = verts
        .iter()
        .map(|&v| {
            let (sxy, _, sz) = rod_rtps(&m, tr, v);
            (sxy, sz)
        })
        .collect();
    let mut out = Vec::with_capacity(prims.len());
    for p in prims {
        let Some(c) = p
            .verts
            .iter()
            .map(|&i| projected.get(usize::from(i)).copied())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let xy = [c[0].0, c[1].0, c[2].0, c[3].0];
        let otz = if p.quad {
            let front = nclip(xy[0], xy[1], xy[2]) > 0 || nclip(xy[1], xy[2], xy[3]) < 0;
            if !front {
                continue;
            }
            ((c[0].1 + c[1].1 + c[2].1 + c[3].1) as i64 * ROD_ZSF4 as i64) >> 12
        } else {
            if nclip(xy[0], xy[1], xy[2]) < 0 {
                continue;
            }
            ((c[0].1 + c[1].1 + c[2].1) as i64 * ROD_ZSF3 as i64) >> 12
        };
        let otz = otz.clamp(0, 0xFFFF) as i32;
        if otz < ROD_NEAR_OTZ {
            continue;
        }
        out.push(RodFace {
            xy,
            rgb: p.rgb,
            quad: p.quad,
            ot: (otz >> 2) as u32,
        });
    }
    out
}

/// The rod-side inputs one in-water frame of the lure tick feeds the rod
/// (`FUN_801D26CC` state 2), off the held packed pad `_DAT_8007B850`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RodDrive {
    /// A fish is on (`_DAT_801D91B4`) as the frame starts.
    pub hooked: bool,
    /// The held packed pad word.
    pub held: u32,
}

/// The fishing rod as an advancing object: `FUN_801D1C5C` per frame, plus the
/// rod globals the lure tick writes. Spawned by the cast lock; retired when
/// the recover swing finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RodActor {
    /// `0x801D91AC`.
    pub swing: RodSwing,
    /// `0x801D9134`, the swing's pitch term.
    pub bob: i16,
    /// `0x801D9150`: the bend, which is both a pitch term and the morph
    /// slot's weight.
    pub bend: i32,
    /// `0x801D914C`: the D-pad-down pitch lift.
    pub lift: i32,
    /// `0x801D9140`: the roll toward the held D-pad side.
    pub lean: i32,
    /// `0x801D9144`: the roll's target.
    pub lean_target: i32,
    /// `0x801D911C`: the yaw toward the fish, `3 * (tip.x - fish.x)`.
    pub yaw: i32,
    /// This frame's projected tip, when the rod mesh resolved.
    pub tip: Option<RodTip>,
    /// The pose this frame's tick drew the model with ([`rod_faces`]);
    /// `None` until the first tick.
    pub pose: Option<RodPose>,
}

impl RodActor {
    /// The rod the cast lock spawns: the cast swing, the seeded bend, every
    /// other term at the run-loop init's zero.
    pub fn cast() -> Self {
        Self {
            swing: RodSwing::Cast,
            bob: 0,
            bend: ROD_BEND_AT_CAST,
            lift: 0,
            lean: 0,
            lean_target: 0,
            yaw: 0,
            tip: None,
            pose: None,
        }
    }

    /// Start the recover swing (a landed catch or a snapped line).
    pub fn recover(&mut self) {
        self.swing = RodSwing::Recover;
    }

    /// Whether the actor has retired.
    pub fn retired(&self) -> bool {
        self.swing == RodSwing::Done
    }

    /// One frame of the rod actor: the swing, the pose off this frame's
    /// globals, the tip projection, then the bend and lift bleeding off.
    ///
    /// PORT: FUN_801d1c5c
    pub fn tick(&mut self, mesh: Option<&RodMesh>, rod: usize, frame_step: i32) {
        let fs = frame_step.max(1);
        let step = (ROD_BOB_STEP * fs) as i16;
        match self.swing {
            RodSwing::Cast => {
                self.bob = self.bob.wrapping_sub(step);
                if self.bob < ROD_BOB_FLOOR {
                    self.bob = ROD_BOB_FLOOR;
                    self.swing = RodSwing::Hold;
                }
            }
            RodSwing::Recover => {
                self.bob = self.bob.wrapping_add(step);
                if self.bob > ROD_BOB_CEILING {
                    self.bob = ROD_BOB_CEILING;
                    self.swing = RodSwing::Done;
                }
            }
            RodSwing::Hold | RodSwing::Done => {}
        }
        // Pitch: the swing plus a sixteenth of (lift + bend / 2), both
        // divisions truncating toward zero (`0x801D1D7C..0x801D1DBC`).
        let half = (self.bend + ((self.bend as u32) >> 31) as i32) >> 1;
        let mut sum = self.lift.wrapping_add(half);
        if sum < 0 {
            sum += 0xF;
        }
        let rot_x = (self.bob as i32).wrapping_add(sum >> 4) as i16;
        let rot_z = (self.lean << 1) as i16;
        // The morph slot's weight is the bend's low halfword (`lhu` into
        // `+0xA0` at `0x801D1EE0`).
        let weight = self.bend as u16 as i16;
        self.pose = Some(RodPose {
            rot_x,
            rot_z,
            yaw: self.yaw,
            weight,
        });
        self.tip = mesh
            .and_then(|m| m.tip(rod, weight))
            .map(|v| rod_tip_screen(v, rot_x, rot_z, self.yaw));
        // The lift bleeds toward zero at `0x20` a frame delta, the bend down to
        // zero at `0x60` (`0x801D1EEC..0x801D1F7C`).
        let lift_step = 0x20 * fs;
        if self.lift < 0 {
            self.lift = (self.lift + lift_step).min(0);
        } else if self.lift > 0 {
            self.lift = (self.lift - lift_step).max(0);
        }
        if self.bend > 0 {
            self.bend = (self.bend - 0x60 * fs).max(0);
        }
    }

    /// The lure tick's rod writes for one in-water frame (`FUN_801D26CC`
    /// state 2): D-pad down lifts the rod (`0x801D2AB4..0x801D2AFC`), a held
    /// reel or a fish on bends it (`0x801D2B04..0x801D2C30`), and the D-pad
    /// sides roll it (`0x801D382C..0x801D395C`).
    ///
    /// The bend clamps read `_DAT_801D90F4`, which is `0` from the landing to
    /// the snap, so they are the bare `0x1000` / `0x1800`. Returns whether a
    /// hooked rod's bend ran past its `0x1800` cap this frame - the gate of
    /// the creak cue (`0x801D2B84..0x801D2BAC`), whose timer is the
    /// session's ([`crate::fishing::PondSession::take_rod_creaks`]).
    ///
    /// PORT: FUN_801d26cc (state 2: the rod lift, bend and roll writes)
    pub fn drive(&mut self, d: RodDrive, frame_step: i32) -> bool {
        let mut over_cap = false;
        let fs = frame_step.max(1);
        if d.held & 0x4000 != 0 {
            self.lift = (self.lift + 0x60 * fs).min(0x1000);
        }
        if d.held & 0xC0 != 0 || d.hooked {
            self.bend += 0x100 * fs;
            if d.held & 0x80 != 0 {
                self.bend += 0x80 * fs;
            }
            let cap = if d.hooked { 0x1800 } else { 0x1000 };
            if self.bend > cap {
                over_cap = d.hooked;
                self.bend = cap;
            }
        }
        let decay = 4 * fs;
        if self.lean_target > 0 {
            self.lean_target = (self.lean_target - decay).max(0);
        } else if self.lean_target < 0 {
            self.lean_target = (self.lean_target + decay).min(0);
        }
        if d.held & 0x2000 != 0 {
            self.lean_target = 0x100;
        }
        if d.held & 0x8000 != 0 {
            self.lean_target = -0x100;
        }
        let ease = 0x10 * fs;
        if self.lean < self.lean_target {
            self.lean = (self.lean + ease).min(self.lean_target);
        } else if self.lean > self.lean_target {
            self.lean = (self.lean - ease).max(self.lean_target);
        }
        over_cap
    }
}

/// One frame's fishing line, clipped: retail's `LINE_G2` packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FishingLine {
    /// The fish end (`+0x08`), in retail 320x240 screen space.
    pub fish: (i16, i16),
    /// The rod end (`+0x10`).
    pub rod: (i16, i16),
    /// [`LINE_FISH_RGB`].
    pub fish_rgb: [u8; 3],
    /// [`LINE_ROD_RGB`].
    pub rod_rgb: [u8; 3],
    /// The ordering-table bucket, off the rod tip's depth.
    pub ot: u32,
}

/// Build one frame's line from the fish's projected point and the rod tip:
/// the packet's two endpoints through [`clip_segment_2d`] against
/// [`LINE_CLIP_RECT`] (`jal 0x801D56E4` at `0x801D3D00`), linked at the tip's
/// depth bucket.
pub fn fishing_line(fish: (i16, i16), tip: RodTip) -> FishingLine {
    let (mut p, mut q) = (fish, tip.sxy);
    clip_segment_2d(&mut p, &mut q, LINE_CLIP_RECT);
    FishingLine {
        fish: p,
        rod: q,
        fish_rgb: LINE_FISH_RGB,
        rod_rgb: LINE_ROD_RGB,
        ot: (tip.depth.max(0) as u32) >> LINE_OT_SHIFT,
    }
}
