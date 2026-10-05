//! The Muscle Dome's **3D arena surface**: the arena shell, the retail
//! battle ground grid, the lead's assembled battle form and the ladder's
//! monster, posed from their own clip banks and framed by one camera - the
//! one kernel every dome host draws, the way
//! [`crate::baka_duel_scene::BakaDuelSurface`] serves the duel.
//!
//! What goes into the frame:
//!
//! - The **fighter** is the character's assembled battle form (the
//!   `battle_char_assembly` chain the native battles and the arts viewer
//!   use): equipment-id sections spliced at their section defaults (the dome
//!   forbids equipment), TSB/CBA relocated to runtime band 0, posed from the
//!   player battle file's own record-0 action streams and per-command swing
//!   records (slots `0xC..=0xF` are the card ids themselves).
//! - The **opponent** is the monster archive's mesh (PROT 867), relocated to
//!   battle texture slot 0 exactly as the battle loader does
//!   (`battle_render_mesh`), posed from its action table by tag.
//! - The **arena shell** is the tail `scene_tmd_stream` of the `other6`
//!   block (PROT 1225, see `docs/subsystems/minigame-muscle-dome.md`), less
//!   its wall-base dust-decal object; its two TIM pages land at their own
//!   framebuffer addresses.
//! - The **ground** is the retail battle ground grid, sampling the arena's
//!   `(832, 0)` page.
//!
//! A dome leg is an ordinary battle (`FUN_801D1510` hands the round to
//! battle init through game mode `0x14`, and the round driver
//! `FUN_801D0748` is the battle overlay's), so the two bodies stand on the
//! battle formation seats - the lone party seat `(0, -800)` and the lone
//! monster seat `(0, 800)` (`crate::battle_seats`) - at the battle world
//! scale, and the frame is filmed by the **battle camera script**
//! (`legaia_engine_vm::battle_cam_script`, `FUN_801D5854`), the same one an
//! ordinary fight runs: the far framing with its idle orbit on the prompts,
//! the case-0 over-the-shoulder close-up on the fighter's command surfaces,
//! case 1 on the attack cursor, case 6 on the approach, case 7 on the
//! strikes, case 8 on the done tail and the battle-over arm on a win.
//!
//! The choreography is the battle's shape on the port's clock: a leg's first
//! acting play walks its attacker in from the seat ([`APPROACH_TICKS`],
//! there is no walk home), each play swings for [`PLAY_CADENCE_TICKS`] (the
//! defender flinching [`FLINCH_DELAY_TICKS`] into a connecting one, falling
//! on the knockout), and each attacker's string closes on the done band's
//! [`DONE_TAIL_TICKS`]. [`turn_timeline`] is that schedule, shared by the
//! surface, the world's playback hold and its tally readout.

use std::sync::Arc;

use legaia_asset::battle_char_assembly as bca;
use legaia_asset::monster_archive::{self, MonsterAnimation};
use legaia_asset::scene_tmd_stream;
use legaia_engine_vm::battle_cam_script as cam;
use legaia_tmd::mesh::VramMesh;

use crate::muscle_dome::{DomeContest, DomePlay, MuscleDomeSession, MusclePhase};

/// The monster archive (PROT 867).
pub const MONSTER_ARCHIVE_PROT_INDEX: usize = 867;
/// The dome arena overlay (PROT 0977) - the course ladder's carrier.
pub const ARENA_OVERLAY_PROT_INDEX: usize = crate::muscle_dome::ARENA_OVERLAY_PROT_INDEX;
/// The arena backdrop stream (PROT 1225).
pub const ARENA_BACKDROP_PROT_INDEX: usize = 1225;
/// The battle-action overlay (PROT 0898): the per-character camera height
/// table `0x801F4D2C` the close-ups read.
pub const BATTLE_ACTION_PROT_INDEX: usize = 898;

/// Ticks one swing of a resolved turn plays for.
pub const PLAY_CADENCE_TICKS: u32 = 34;
/// Ticks into a connecting swing at which the defender flinches.
pub const FLINCH_DELAY_TICKS: u32 = 12;
/// Ticks the leg's first acting play walks its attacker in from the seat.
/// Retail's approach is walk-clip root motion at about twenty raw units a
/// vsync (`engine-core::world::battle::locomotion`); the seats stand 1600
/// apart, so the walk closes in a little under fifty frames.
pub const APPROACH_TICKS: u32 = 48;
/// Ticks the done band holds after an attacker's string: the action SM's
/// `0x50` arm seeds the tail timer `ctx[+0x6D8] = 0x3C` and `0x51` counts it
/// down under case 8 for an attack.
pub const DONE_TAIL_TICKS: u32 = 0x3C;
/// Retail's battle world scale (base matrix `0x8007BF10 = 16384*I`): every
/// battle draw class - bodies, backdrop and ground - carries it, and the
/// camera's translation trio is authored against it.
pub const BATTLE_WORLD_SCALE: f32 = 4.0;
/// The seat body `i` stands on at leg open, `(x, z)`: the lone party seat
/// and the lone monster seat (`0x800775C8` / `0x80077608` row 0).
pub fn seat_xz(i: usize) -> [f32; 2] {
    let s = if i == 0 {
        crate::battle_seats::party_seat(1, 0)
    } else {
        crate::battle_seats::monster_seat(1, 0, false)
    };
    [f32::from(s.x), f32::from(s.z)]
}
/// Battle headings `+0x46`: the fighter faces `+Z`, the monster `-Z`.
pub const FACINGS: [i32; 2] = [0, 0x800];

/// What one stretch of a turn's playback is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeatKind {
    /// The leg's first acting play walking its attacker in.
    Approach,
    /// One play's swing.
    Swing,
    /// The done band closing an attacker's string.
    Done,
}

/// One stretch of [`turn_timeline`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beat {
    pub kind: BeatKind,
    /// The play it belongs to (the first play of the string for
    /// [`BeatKind::Approach`], the last for [`BeatKind::Done`]).
    pub play: usize,
    /// The acting fighter slot.
    pub attacker: usize,
    /// Tick the beat starts at, from the start of the playback.
    pub start: u32,
    pub len: u32,
}

/// A resolved turn's playback schedule: an approach ahead of the first play
/// when `closes_in` ([`MuscleDomeSession::last_turn_closes_in`]), one swing
/// per play, and a done tail after each attacker's last play.
pub fn turn_timeline(plays: &[DomePlay], closes_in: bool) -> Vec<Beat> {
    let mut out = Vec::new();
    let mut t = 0;
    let mut push = |out: &mut Vec<Beat>, kind, play, attacker, len| {
        out.push(Beat {
            kind,
            play,
            attacker,
            start: t,
            len,
        });
        t += len;
    };
    if closes_in && let Some(first) = plays.first() {
        push(
            &mut out,
            BeatKind::Approach,
            0,
            first.attacker.min(1),
            APPROACH_TICKS,
        );
    }
    for (i, p) in plays.iter().enumerate() {
        let a = p.attacker.min(1);
        push(&mut out, BeatKind::Swing, i, a, PLAY_CADENCE_TICKS);
        let string_ends = plays.get(i + 1).is_none_or(|n| n.attacker.min(1) != a);
        if string_ends {
            push(&mut out, BeatKind::Done, i, a, DONE_TAIL_TICKS);
        }
    }
    out
}

/// How long a resolved turn plays out ([`turn_timeline`]'s span). A turn with
/// no plays (an empty queue on both sides) has nothing to show and holds for
/// nothing.
pub fn turn_playback_ticks(plays: &[DomePlay], closes_in: bool) -> u32 {
    turn_timeline(plays, closes_in)
        .last()
        .map_or(0, |b| b.start + b.len)
}

/// The beat `elapsed` ticks into the playback, if any.
pub fn beat_at(plays: &[DomePlay], closes_in: bool, elapsed: u32) -> Option<Beat> {
    turn_timeline(plays, closes_in)
        .into_iter()
        .find(|b| (b.start..b.start + b.len).contains(&elapsed))
}

/// Player battle-form clip slots the dome plays.
const P_IDLE: u32 = 0;
const P_HIT: u32 = 2;
const P_KO: u32 = 4;
/// The walk / approach cycle - action tag `1` in both the player and the
/// monster files (`docs/formats/monster-animation.md`).
const WALK_TAG: u8 = 1;
const P_SWINGS: [u32; 4] = [0xC, 0xD, 0xE, 0xF];

/// One decoded clip: per (frame, part) `[tx, ty, tz, rx, ry, rz]`.
#[derive(Debug, Clone)]
pub struct DomeClip {
    frames: Vec<Vec<[i32; 6]>>,
    /// Keyframes advanced per 16 ticks (the retail `rate` byte, doubled).
    rate: u32,
}

impl DomeClip {
    fn from_anim(anim: &MonsterAnimation) -> Option<Self> {
        if anim.frame_count == 0 || anim.frames.is_empty() {
            return None;
        }
        let frames = anim
            .frames
            .iter()
            .map(|f| {
                f.iter()
                    .map(|t| {
                        [
                            i32::from(t.tx),
                            i32::from(t.ty),
                            i32::from(t.tz),
                            i32::from(t.rx),
                            i32::from(t.ry),
                            i32::from(t.rz),
                        ]
                    })
                    .collect()
            })
            .collect();
        Some(Self {
            frames,
            rate: u32::from(anim.rate.max(1)) * 2,
        })
    }

    /// Keyframe count.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

/// One body's clip set.
#[derive(Debug, Clone)]
struct BodyClips {
    idle: DomeClip,
    /// The walk / approach cycle (record tag `1`), idle when absent.
    walk: DomeClip,
    hit: DomeClip,
    ko: DomeClip,
    /// The player's per-command swings (`0xC..=0xF`, index `cmd - 0xC`), or
    /// the monster's one attack clip in slot 0.
    swings: [Option<DomeClip>; 4],
}

impl BodyClips {
    fn swing(&self, cmd: u8) -> &DomeClip {
        let i = usize::from(cmd.wrapping_sub(0xC));
        self.swings
            .get(i)
            .and_then(Option::as_ref)
            .or(self.swings[0].as_ref())
            .unwrap_or(&self.idle)
    }
}

/// One posable body: its rest mesh, per-vertex object ids and clips.
#[derive(Debug, Clone)]
struct DomeBody {
    mesh: VramMesh,
    oids: Vec<u32>,
    clips: BodyClips,
}

/// Which clip a body plays and since when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipKind {
    Idle,
    Walk,
    Hit,
    Ko,
    Swing(u8),
}

#[derive(Debug, Clone, Copy)]
struct Act {
    kind: ClipKind,
    start: u32,
    looped: bool,
    hold: bool,
}

impl Act {
    fn idle(start: u32) -> Self {
        Self {
            kind: ClipKind::Idle,
            start,
            looped: true,
            hold: false,
        }
    }
}

/// The dome's camera: one pose of the battle camera script, projected the
/// way the battle hosts project a stage draw - [`cam::battle_vp`] at the
/// battle world scale over the stage model `diag(4, -4, 4)` - so both hosts
/// take the one matrix for raw Y-down world vertices, depth `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DomeCamera {
    pub pose: cam::BattleCamPose,
}

impl DomeCamera {
    /// The column-major view-projection a host multiplies a raw (Y-down)
    /// world vertex by. Both hosts upload exactly this.
    pub fn vp_raw(&self, aspect: f32) -> [f32; 16] {
        let k = BATTLE_WORLD_SCALE;
        let stage: [f32; 16] = [
            k, 0.0, 0.0, 0.0, //
            0.0, -k, 0.0, 0.0, //
            0.0, 0.0, k, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ];
        cam::mat_mul(&cam::battle_vp(&self.pose, k, aspect), &stage)
    }
}

/// The decoded, seated dome: the static buffers and the VRAM for one
/// `(monster, character)` pair.
pub struct MuscleDomeAssets {
    key: (u16, u32),
    bodies: [DomeBody; 2],
    /// Everything that never moves (arena shell + ground grid, or the
    /// fallback floor), appended after the two bodies.
    statics: VramMesh,
    statics_flat: Vec<u8>,
    vram: legaia_tim::Vram,
    arena: bool,
    /// The monster record's size class (`+0x1F`), which sizes the action
    /// framing's depth (`FUN_801F0348`).
    size_class: u8,
    /// The per-character close-up heights (`0x801F4D2C`).
    heights: Option<legaia_asset::battle_camera_table::BattleCameraHeights>,
}

impl std::fmt::Debug for MuscleDomeAssets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MuscleDomeAssets")
            .field("key", &self.key)
            .field("arena", &self.arena)
            .finish_non_exhaustive()
    }
}

fn fighter_build(raw: &[u8]) -> Option<(bca::AssembledCharacter, VramMesh, Vec<u32>)> {
    let pack = legaia_asset::battle_data_pack::parse(raw).ok()?;
    let mut asm = bca::assemble_character(raw, &pack, &[0u8; 5]).ok()?;
    bca::relocate_tsb_cba(&mut asm.tmd, 0).ok()?;
    let tmd = legaia_tmd::parse(&asm.tmd).ok()?;
    let (mesh, oids) = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, &asm.tmd);
    (!mesh.indices.is_empty()).then_some((asm, mesh, oids))
}

/// One battle-form clip by runtime action slot, expanded per assembled
/// object: slot `0` the record-0 idle loop, `0xC..=0xF` the per-command
/// swing records, any other slot the record-0 action table by tag (the
/// party hit-reaction family `[2, 3, 4, 5, 0xB]`).
pub fn fighter_clip(
    raw: &[u8],
    asm: &bca::AssembledCharacter,
    slot: u32,
) -> Option<MonsterAnimation> {
    let anim = if (0xC..=0xF).contains(&slot) {
        let pack = legaia_asset::battle_data_pack::parse(raw).ok()?;
        bca::swing_battle_animations(raw, &pack, &[0u8; 5])
            .ok()?
            .into_iter()
            .find(|s| u32::from(s.slot) == slot)?
            .anim
    } else if slot == 0 {
        bca::idle_battle_animation(raw).ok()??
    } else {
        bca::battle_animations(raw)
            .ok()?
            .into_iter()
            .find(|a| u32::from(a.action_id) == slot)?
    };
    Some(bca::expand_animation_for_objects(&anim, &asm.anm_bones))
}

/// The monster clip indices the dome plays: idle (tag 0), the attack (tag
/// `0x21`, else `0x20`, else the first `>= 0x20`, else entry 1), the hit
/// (tag 2, else 3) and the knockdown (tag 4, else the hit).
pub fn pick_monster_clips(anims: &[MonsterAnimation]) -> [usize; 4] {
    let by_tag = |t: u8| anims.iter().position(|a| a.action_id == t);
    let idle = by_tag(0).unwrap_or(0);
    let attack = by_tag(0x21)
        .or_else(|| by_tag(0x20))
        .or_else(|| anims.iter().position(|a| a.action_id >= 0x20))
        .unwrap_or(if anims.len() > 1 { 1 } else { idle });
    let hit = by_tag(2).or_else(|| by_tag(3));
    let ko = by_tag(4).or(hit).unwrap_or(idle);
    [idle, attack, hit.unwrap_or(idle), ko]
}

/// The arena shell as a hybrid VRAM mesh plus its packet-colour stream,
/// with TMD object 1 (the wall-base dust decal) dropped: its texels are
/// genuinely bright, and the retail match capture shows a mist-free
/// interior, so retail's backdrop path does not draw it as static geometry.
pub fn arena_hybrid(buf: &[u8]) -> Option<(VramMesh, Vec<u8>)> {
    let stream = scene_tmd_stream::detect(buf)?;
    let tmd_bytes = buf.get(stream.tmd_range())?;
    let tmd = legaia_tmd::parse(tmd_bytes).ok()?;
    let (mesh, oids, shading) = legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, tmd_bytes);
    let flat = crate::packet_color::hybrid(&mesh, &shading);
    if !oids.iter().any(|&o| o != 0) {
        return Some((mesh, flat));
    }
    let keep: Vec<bool> = oids.iter().map(|&o| o == 0).collect();
    Some(filter_mesh(&mesh, &flat, &keep))
}

/// The arena as retail draws it: [`arena_hybrid`]'s half-shell twice.
/// `FUN_800513F0` registers the backdrop TMD once and spawns two backdrop
/// actors from it - copy A at raw coordinates, copy B under
/// [`ARENA_SECOND_COPY`] - which closes the half-stage into the full ring.
/// One copy alone leaves the whole `-X` half of the arena open. Every dome
/// host draws this mesh.
pub fn arena_ring(buf: &[u8]) -> Option<(VramMesh, Vec<u8>)> {
    let (half, flat) = arena_hybrid(buf)?;
    let mut ring = half.clone();
    let mut ring_flat = flat.clone();
    append(
        &mut ring,
        &mut ring_flat,
        &second_copy(&half, ARENA_SECOND_COPY),
        &flat,
    );
    Some((ring, ring_flat))
}

fn filter_mesh(mesh: &VramMesh, flat: &[u8], keep: &[bool]) -> (VramMesh, Vec<u8>) {
    let mut out = empty_mesh();
    let mut remap = vec![u32::MAX; keep.len()];
    let mut flat2 = Vec::new();
    for (i, &k) in keep.iter().enumerate() {
        if !k {
            continue;
        }
        remap[i] = out.positions.len() as u32;
        out.positions.push(mesh.positions[i]);
        out.uvs.push(mesh.uvs[i]);
        out.cba_tsb.push(mesh.cba_tsb[i]);
        out.normals.push(mesh.normals[i]);
        out.colors.push(mesh.colors[i]);
        flat2.extend_from_slice(&flat[i * 4..i * 4 + 4]);
    }
    for t in mesh.indices.as_chunks::<3>().0 {
        let [a, b, c] = t.map(|i| remap[i as usize]);
        if a != u32::MAX && b != u32::MAX && c != u32::MAX {
            out.indices.extend_from_slice(&[a, b, c]);
        }
    }
    (out, flat2)
}

/// The transform the arena's second backdrop copy takes.
///
/// Retail picks it from the `SCUS_942.54` mirror list at `DAT_80078B50`,
/// keyed by `word[0x80084540] + byte[0x8007BD60]`. The dome contest leaves
/// both at `3` (the retail `minigame_muscle_dome` state), and backdrop id `6`
/// is not on the list, so the copy takes the default half turn. The shell
/// is open toward `-X` and only about a third of it is symmetric in `z`, so
/// the two transforms put the furniture in visibly different places.
pub const ARENA_SECOND_COPY: legaia_asset::battle_backdrop::SecondCopy =
    legaia_asset::battle_backdrop::SecondCopy::HalfTurn;

/// `mesh` under a backdrop second-copy transform, its winding restored when
/// the transform reflects.
fn second_copy(mesh: &VramMesh, copy: legaia_asset::battle_backdrop::SecondCopy) -> VramMesh {
    let k = copy.scale();
    let mut out = mesh.clone();
    for p in &mut out.positions {
        *p = [p[0] * k[0], p[1] * k[1], p[2] * k[2]];
    }
    for n in &mut out.normals {
        *n = [n[0] * k[0], n[1] * k[1], n[2] * k[2]];
    }
    if copy.flips_winding() {
        for t in out.indices.as_chunks_mut::<3>().0 {
            t.swap(1, 2);
        }
    }
    out
}

/// Append `src` (with its `flat` stream) onto `dst`.
fn append(dst: &mut VramMesh, dst_flat: &mut Vec<u8>, src: &VramMesh, flat: &[u8]) {
    let base = dst.positions.len() as u32;
    dst.positions.extend_from_slice(&src.positions);
    dst.uvs.extend_from_slice(&src.uvs);
    dst.cba_tsb.extend_from_slice(&src.cba_tsb);
    dst.normals.extend_from_slice(&src.normals);
    dst.colors.extend_from_slice(&src.colors);
    dst.indices.extend(src.indices.iter().map(|i| i + base));
    dst_flat.extend_from_slice(flat);
}

fn empty_mesh() -> VramMesh {
    VramMesh {
        positions: Vec::new(),
        uvs: Vec::new(),
        cba_tsb: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
        indices: Vec::new(),
    }
}

/// Quads onto a mesh: `[x0, z0, x1, z1]` at floor height 0, one UV rect and
/// one `[cba, tsb]` pair, `[r, g, b, textured]` fill.
fn push_floor_quad(
    m: &mut VramMesh,
    flat: &mut Vec<u8>,
    rect: [f32; 4],
    uv: [u8; 4],
    ct: [u16; 2],
    rgba: [u8; 4],
) {
    let base = m.positions.len() as u32;
    let [x0, z0, x1, z1] = rect;
    let [u0, v0, u1, v1] = uv;
    m.positions
        .extend_from_slice(&[[x0, 0.0, z0], [x1, 0.0, z0], [x1, 0.0, z1], [x0, 0.0, z1]]);
    m.uvs
        .extend_from_slice(&[[u0, v0], [u1, v0], [u1, v1], [u0, v1]]);
    for _ in 0..4 {
        m.cba_tsb.push(ct);
        m.normals.push([0.0, -1.0, 0.0]);
        m.colors.push([rgba[0], rgba[1], rgba[2]]);
        flat.extend_from_slice(&rgba);
    }
    m.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// The retail battle ground grid: 28x28 cells of `0x200`, each four `0x100`
/// sub-quads over the 32x32 texel windows at `(192, 192)` of the `(832, 0)`
/// page, CLUT `(0, 479)`.
fn ground_grid(m: &mut VramMesh, flat: &mut Vec<u8>) {
    const CELL: f32 = 512.0;
    const SUB: f32 = 256.0;
    const N: i32 = 14;
    for cz in -N..N {
        for cx in -N..N {
            for sr in 0..2u8 {
                for sc in 0..2u8 {
                    let x0 = cx as f32 * CELL + f32::from(sc) * SUB;
                    let z0 = cz as f32 * CELL + f32::from(sr) * SUB;
                    let u0 = 192 + sc * 32;
                    let v0 = 192 + sr * 32;
                    push_floor_quad(
                        m,
                        flat,
                        [x0, z0, x0 + SUB, z0 + SUB],
                        [u0, v0, u0 + 31, v0 + 31],
                        [0x77C0, 0x000D],
                        [128, 128, 128, 255],
                    );
                }
            }
        }
    }
}

/// The fallback floor without an arena: a dark checkerboard sized off the
/// fighters' spacing.
fn checker_floor(m: &mut VramMesh, flat: &mut Vec<u8>, extent: f32) {
    let t = (extent / 4.0).round().max(160.0);
    const N: i32 = 12;
    for iz in -N..N {
        for ix in -N..N {
            let c = if (ix + iz) & 1 == 0 {
                [34, 36, 44, 0]
            } else {
                [48, 52, 62, 0]
            };
            let x0 = ix as f32 * t;
            let z0 = iz as f32 * t;
            push_floor_quad(m, flat, [x0, z0, x0 + t, z0 + t], [0; 4], [0, 0], c);
        }
    }
}

impl MuscleDomeAssets {
    /// Decode the dome for `(monster_id, char_slot)`. `None` when the
    /// character's battle form does not assemble or the monster's mesh or
    /// idle does not decode on this image.
    pub fn load(
        read_prot: &impl Fn(usize) -> Option<Arc<Vec<u8>>>,
        monster_id: u16,
        char_slot: u32,
    ) -> Option<Self> {
        let player_file = read_prot(
            crate::battle_party_form::PLAYER_BATTLE_FILE_BASE as usize + char_slot.min(2) as usize,
        )?;
        let raw = player_file.as_slice();
        let (asm, fmesh, foids) = fighter_build(raw)?;
        let clip = |slot: u32| fighter_clip(raw, &asm, slot).and_then(|a| DomeClip::from_anim(&a));
        let p_idle = clip(P_IDLE)?;
        let p_hit = clip(P_HIT).unwrap_or_else(|| p_idle.clone());
        let p_ko = clip(P_KO).unwrap_or_else(|| p_hit.clone());
        let p_walk = clip(u32::from(WALK_TAG)).unwrap_or_else(|| p_idle.clone());
        let fighter = DomeBody {
            mesh: fmesh,
            oids: foids,
            clips: BodyClips {
                walk: p_walk,
                idle: p_idle,
                hit: p_hit,
                ko: p_ko,
                swings: P_SWINGS.map(clip),
            },
        };

        let archive = read_prot(MONSTER_ARCHIVE_PROT_INDEX)?;
        let mut vram = legaia_tim::Vram::new();
        // The character's band-0 texture pool + battle palette.
        if let Ok(pack) = legaia_asset::battle_data_pack::parse(raw) {
            if let Ok(uploads) = bca::character_texture_uploads(raw, &pack, &[0u8; 5], 0) {
                for u in &uploads {
                    vram.write_block(u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
                    if !u.clut.is_empty() {
                        vram.write_clut_row(u.clut_x, u.clut_row(), &u.clut_bytes());
                    }
                }
            }
            let mut rows: Vec<u16> = fighter
                .mesh
                .cba_tsb
                .iter()
                .map(|c| (c[0] >> 6) & 0x1FF)
                .collect();
            rows.sort_unstable();
            rows.dedup();
            let mut cols: Vec<u16> = fighter
                .mesh
                .cba_tsb
                .iter()
                .map(|c| (c[0] & 0x3F) * 16)
                .collect();
            cols.sort_unstable();
            cols.dedup();
            // Vahn = the byte-exact fixed-stride record parse; the others =
            // the equipment-robust collector.
            let pal = if char_slot == 0 {
                legaia_asset::battle_char_palette::find_record0(raw).and_then(|rec0| {
                    legaia_asset::battle_char_palette::parse_record(raw, rec0).ok()
                })
            } else {
                legaia_asset::battle_char_palette::collect_palette(raw, 0, &cols).ok()
            };
            if let Some(pal) = pal {
                for &row in &rows {
                    for band in &pal.bands {
                        let bytes: Vec<u8> = band
                            .vram_words()
                            .iter()
                            .flat_map(|w| w.to_le_bytes())
                            .collect();
                        vram.write_clut_row(band.base, row, &bytes);
                    }
                }
            }
        }
        let monster_mesh = monster_archive::mesh(&archive, monster_id).ok()??;
        // Injects the monster's pool at slot 0's CLUT row + page origin.
        let mmesh = monster_mesh.battle_render_mesh(0, &mut vram)?;
        let mtmd = legaia_tmd::parse(monster_mesh.tmd_bytes()).ok()?;
        let moids =
            legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&mtmd, monster_mesh.tmd_bytes()).1;
        let anims = monster_archive::animations(&archive, monster_id).ok()??;
        let [mi, ma, mh, mk] = pick_monster_clips(&anims);
        let mclip = |i: usize| anims.get(i).and_then(DomeClip::from_anim);
        let m_idle = mclip(mi)?;
        let m_walk = anims
            .iter()
            .find(|a| a.action_id == WALK_TAG)
            .and_then(DomeClip::from_anim)
            .unwrap_or_else(|| m_idle.clone());
        let size_class = monster_archive::record(&archive, monster_id)
            .ok()
            .flatten()
            .map_or(0, |r| r.size_class);
        let monster = DomeBody {
            mesh: mmesh,
            oids: moids,
            clips: BodyClips {
                walk: m_walk,
                hit: mclip(mh).unwrap_or_else(|| m_idle.clone()),
                ko: mclip(mk).unwrap_or_else(|| m_idle.clone()),
                swings: [mclip(ma), None, None, None],
                idle: m_idle,
            },
        };

        let mut statics = empty_mesh();
        let mut statics_flat = Vec::new();
        let arena_buf = read_prot(ARENA_BACKDROP_PROT_INDEX);
        let arena = arena_buf.as_deref().and_then(|b| arena_ring(b));
        if let Some(buf) = arena_buf.as_deref()
            && arena.is_some()
        {
            for chunk in scene_tmd_stream::battle_tim_chunks(buf) {
                if let Some(bytes) = buf.get(chunk.payload_offset..)
                    && let Ok(tim) = legaia_tim::parse(bytes)
                {
                    vram.upload_tim(&tim);
                }
            }
        }
        let has_arena = arena.is_some();
        if let Some((m, flat)) = arena {
            append(&mut statics, &mut statics_flat, &m, &flat);
            ground_grid(&mut statics, &mut statics_flat);
        }
        let heights = read_prot(BATTLE_ACTION_PROT_INDEX)
            .and_then(|raw| legaia_asset::battle_camera_table::parse(&raw));
        let mut assets = Self {
            key: (monster_id, char_slot),
            bodies: [fighter, monster],
            statics,
            statics_flat,
            vram,
            arena: has_arena,
            size_class,
            heights,
        };
        if !has_arena {
            let reach = assets.reach();
            checker_floor(&mut assets.statics, &mut assets.statics_flat, reach);
        }
        Some(assets)
    }

    /// The seated `(monster_id, char_slot)`.
    pub fn key(&self) -> (u16, u32) {
        self.key
    }

    /// The dome VRAM (character pool + palette, monster pool, arena pages).
    pub fn vram(&self) -> &legaia_tim::Vram {
        &self.vram
    }

    /// Rest-pose half-width and height of body `i`.
    fn extent(&self, i: usize) -> (f32, f32) {
        let b = &self.bodies[i];
        let mut out = b.mesh.positions.clone();
        pose_into(
            &mut out,
            &b.mesh.positions,
            &b.oids,
            &b.clips.idle,
            0,
            [0.0; 3],
        );
        let (mut lo, mut hi, mut top) = (f32::INFINITY, f32::NEG_INFINITY, 0.0f32);
        for p in &out {
            lo = lo.min(p[0]);
            hi = hi.max(p[0]);
            top = top.max(-p[1]);
        }
        let half = (hi - lo) / 2.0;
        (
            if half.is_finite() && half != 0.0 {
                half
            } else {
                200.0
            },
            if top != 0.0 { top } else { 400.0 },
        )
    }

    /// The distance a closing approach stops at between the two bodies'
    /// origins: both rest half-widths with a margin, which puts a swing's
    /// reach on the defender.
    fn reach(&self) -> f32 {
        let (hp, _) = self.extent(0);
        let (hm, _) = self.extent(1);
        (hp + hm) * 1.5 + 120.0
    }
}

/// Pose `base` into `out` from `clip` frame `frame`: per object `Rz Ry Rx v
/// + T`, then the world yaw about Y and the `(dx, dz)` floor offset
/// (`place = [dx, yaw, dz]`).
fn pose_into(
    out: &mut [[f32; 3]],
    base: &[[f32; 3]],
    oids: &[u32],
    clip: &DomeClip,
    frame: usize,
    place: [f32; 3],
) {
    let a2r = std::f32::consts::TAU / 4096.0;
    let n = clip.frames.len().max(1);
    let keys = &clip.frames[frame % n];
    let rots: Vec<([f32; 3], [f32; 3], [f32; 3])> = keys
        .iter()
        .map(|k| {
            let (sx, cx) = (k[3] as f32 * a2r).sin_cos();
            let (sy, cy) = (k[4] as f32 * a2r).sin_cos();
            let (sz, cz) = (k[5] as f32 * a2r).sin_cos();
            (
                [sx, sy, sz],
                [cx, cy, cz],
                [k[0] as f32, k[1] as f32, k[2] as f32],
            )
        })
        .collect();
    let [dx, yaw, dz] = place;
    let (ws, wc) = yaw.sin_cos();
    for (v, (o, b)) in out.iter_mut().zip(base.iter()).enumerate() {
        let [mut x, mut y, mut z] = *b;
        if let Some((s, c, t)) = oids.get(v).and_then(|&o| rots.get(o as usize)) {
            let (ny, nz) = (y * c[0] - z * s[0], y * s[0] + z * c[0]);
            y = ny;
            z = nz;
            let (nx, nz) = (x * c[1] + z * s[1], -x * s[1] + z * c[1]);
            x = nx;
            z = nz;
            let (nx, ny) = (x * c[2] - y * s[2], x * s[2] + y * c[2]);
            x = nx + t[0];
            y = ny + t[1];
            z += t[2];
        }
        *o = [x * wc + z * ws + dx, y, -x * ws + z * wc + dz];
    }
}

/// The posed dome for one frame.
#[derive(Debug, Clone)]
pub struct MuscleDomeScene {
    /// Posed positions, raw retail world coordinates (Y down): the fighter,
    /// then the monster, then the statics.
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[u8; 2]>,
    pub cba_tsb: Vec<[u16; 2]>,
    /// Per-vertex packet colour (the textured modulation / untextured fill).
    pub colors: Vec<[u8; 3]>,
    /// Per-vertex `[r, g, b, textured]` - the browser renderer's stream.
    pub flat_rgba: Vec<u8>,
    /// Every triangle.
    pub indices: Vec<u32>,
    /// The triangles whose first vertex is textured / untextured - the
    /// native window's two pipelines.
    pub textured_indices: Vec<u32>,
    pub untextured_indices: Vec<u32>,
    /// The camera this frame draws under.
    pub camera: DomeCamera,
    /// Vertex offset of each body: `[fighter, monster, statics]`.
    pub bases: [usize; 3],
}

/// The per-host cache every dome host drives once a frame, as
/// [`crate::baka_duel_scene::BakaDuelSurface`] is for the duel.
#[derive(Debug, Default)]
pub struct MuscleDomeSurface {
    assets: Option<Arc<MuscleDomeAssets>>,
    /// A `(monster, character)` pair that failed to decode, so a missing
    /// asset is not re-decoded every frame.
    failed: Option<(u16, u32)>,
    ladder: Option<Option<Vec<crate::muscle_dome::DomeCourse>>>,
    scene: Option<MuscleDomeScene>,
    generation: u32,
    tick: u32,
    act: [Option<Act>; 2],
    /// The resolved turn playing out, if any.
    playback: Option<Playback>,
    /// Each body's floor position `(x, z)`, raw battle units.
    pos: [[f32; 2]; 2],
    /// The session turn last seen, so a fresh leg re-seats the pair.
    last_turn: Option<u32>,
    /// The battle camera script's state for this leg.
    camera: Option<cam::BattleCamera>,
    /// The Battle Camera option word `0x800846C0`
    /// ([`Self::set_camera_option`]).
    camera_option: u8,
    /// A host-named selection framing ([`Self::set_select_framing`]).
    select_framing: Option<SelectFraming>,
}

/// The selection-screen framing a host names for the dome camera, in the
/// battle's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectFraming {
    /// The round prompt and the `Begin | Reselect` confirm: case 9, the far
    /// framing with its idle orbit.
    Far,
    /// The ring, the direction entry and its review, the Ra-Seru list:
    /// case 0, the fighter's over-the-shoulder close-up.
    Member,
    /// The `Auto | Command` prompt and its cursor: case 1, turned toward
    /// the opponent.
    Target,
}

/// A resolved turn being played out on the surface.
#[derive(Debug, Clone)]
struct Playback {
    start: u32,
    plays: Vec<DomePlay>,
    closes_in: bool,
    /// The approach's start and stop points, fixed on its first tick.
    walk: Option<([f32; 2], [f32; 2])>,
}

impl Playback {
    fn elapsed(&self, tick: u32) -> u32 {
        tick.wrapping_sub(self.start)
    }

    fn beat(&self, tick: u32) -> Option<Beat> {
        beat_at(&self.plays, self.closes_in, self.elapsed(tick))
    }
}

impl MuscleDomeSurface {
    /// The monster the contest's current rung seats: the course ladder's
    /// `(course, round)` row, the round clamped to the course's last.
    fn seated_monster(
        &mut self,
        read_prot: &impl Fn(usize) -> Option<Arc<Vec<u8>>>,
        contest: Option<&DomeContest>,
    ) -> Option<u16> {
        let ladder = self.ladder.get_or_insert_with(|| {
            read_prot(ARENA_OVERLAY_PROT_INDEX)
                .and_then(|raw| crate::muscle_dome::parse_course_ladder(&raw))
        });
        let (course, round) = contest.map_or((0, 0), |c| (c.course(), c.round()));
        let rounds = &ladder.as_ref()?.get(course)?.rounds;
        let n = (round as usize).min(rounds.len().checked_sub(1)?);
        Some(rounds.get(n)?.monster_id as u16)
    }

    /// One frame. `None` - and the posed buffers dropped - when no dome
    /// session is live or its scene does not decode. The assets load once
    /// per seated `(monster, character)` pair through `read_prot`, which
    /// bumps [`Self::generation`]; the choreography steps and the pose runs
    /// every call.
    pub fn frame(
        &mut self,
        read_prot: impl Fn(usize) -> Option<Arc<Vec<u8>>>,
        session: Option<&MuscleDomeSession>,
        contest: Option<&DomeContest>,
        char_slot: u32,
    ) -> Option<&MuscleDomeScene> {
        let Some(session) = session else {
            self.reset();
            return None;
        };
        let monster = self.seated_monster(&read_prot, contest)?;
        self.frame_seated(read_prot, session, monster, char_slot)
    }

    /// [`Self::frame`] for an opponent the host names itself rather than the
    /// contest's ladder rung - the minigames page's foe picker.
    pub fn frame_seated(
        &mut self,
        read_prot: impl Fn(usize) -> Option<Arc<Vec<u8>>>,
        session: &MuscleDomeSession,
        monster: u16,
        char_slot: u32,
    ) -> Option<&MuscleDomeScene> {
        let key = (monster, char_slot);
        if self.assets.as_ref().map(|a| a.key()) != Some(key) {
            if self.failed == Some(key) {
                return None;
            }
            match MuscleDomeAssets::load(&read_prot, monster, char_slot) {
                Some(a) => {
                    self.assets = Some(Arc::new(a));
                    self.failed = None;
                    self.generation = self.generation.wrapping_add(1);
                    self.reseat();
                }
                None => {
                    self.failed = Some(key);
                    self.scene = None;
                    return None;
                }
            }
        }
        let assets = self.assets.clone()?;
        if self.last_turn.is_some_and(|t| session.turn() < t) {
            // A new leg against the same opponent: back to the seats.
            self.reseat();
        }
        // A turn resolved since the last frame: its plays act out. Keyed on
        // the turn counter rather than on a `Resolve` frame, because a host
        // that resolves inside one call (the minigames page) never shows the
        // surface one.
        let resolved = self.last_turn.is_some_and(|t| session.turn() > t);
        self.last_turn = Some(session.turn());
        self.step(session, &assets, resolved);
        self.pose(&assets);
        self.drive_camera(session, &assets);
        self.scene.as_ref()
    }

    /// The framing a host that drives its own selection flow asks for (the
    /// minigames page, which commits through the session's card calls rather
    /// than through its [`crate::muscle_dome::DomeMenu`]). `None` - every
    /// world host - derives it from the session's own menu.
    pub fn set_select_framing(&mut self, framing: Option<SelectFraming>) {
        self.select_framing = framing;
    }

    /// The playback beat on screen now, and the ticks into it - the schedule
    /// a host that times its own play-out readouts follows
    /// ([`turn_timeline`]).
    pub fn playback_beat(&self) -> Option<(Beat, u32)> {
        let pb = self.playback.as_ref()?;
        let beat = pb.beat(self.tick)?;
        Some((beat, pb.elapsed(self.tick) - beat.start))
    }

    /// The Battle Camera option the framings run under (`0x800846C0`:
    /// `0` Close, `1` Normal, `2` Far) - the world's
    /// `toggles.battle_camera`, which both hosts pass in.
    pub fn set_camera_option(&mut self, option: u8) {
        self.camera_option = option;
    }

    /// Back to leg open: both bodies on their seats, idling, the camera
    /// re-created on its next drive.
    fn reseat(&mut self) {
        self.act = [None; 2];
        self.playback = None;
        self.pos = [seat_xz(0), seat_xz(1)];
        self.camera = None;
    }

    fn reset(&mut self) {
        if self.scene.take().is_some() {
            self.generation = self.generation.wrapping_add(1);
        }
        self.reseat();
        self.last_turn = None;
    }

    /// The choreography: a resolved turn (the session leaving `Resolve`)
    /// starts its [`turn_timeline`]; each tick plays the beat it is on - the
    /// closing walk, a swing with the defender's flinch (its knockdown on
    /// the play that ends the leg), the done tail.
    fn step(&mut self, session: &MuscleDomeSession, assets: &MuscleDomeAssets, resolved: bool) {
        self.tick = self.tick.wrapping_add(1);
        let tick = self.tick;
        if resolved {
            self.playback = Some(Playback {
                start: tick,
                plays: session.last_turn_plays().to_vec(),
                closes_in: session.last_turn_closes_in(),
                walk: None,
            });
        }
        let Some(pb) = self.playback.as_mut() else {
            return;
        };
        let Some(beat) = pb.beat(tick) else {
            self.playback = None;
            return;
        };
        let at = pb.elapsed(tick) - beat.start;
        let a = beat.attacker;
        let d = a ^ 1;
        match beat.kind {
            BeatKind::Approach => {
                let pos = self.pos;
                let (from, to) = *pb.walk.get_or_insert_with(|| {
                    let from = pos[a];
                    let (dx, dz) = (pos[d][0] - from[0], pos[d][1] - from[1]);
                    let len = (dx * dx + dz * dz).sqrt().max(1e-3);
                    let stop = (len - assets.reach()).max(0.0) / len;
                    (from, [from[0] + dx * stop, from[1] + dz * stop])
                });
                let t = (at + 1) as f32 / beat.len.max(1) as f32;
                self.pos[a] = [
                    from[0] + (to[0] - from[0]) * t,
                    from[1] + (to[1] - from[1]) * t,
                ];
                if at == 0 {
                    self.act[a] = Some(Act {
                        kind: ClipKind::Walk,
                        start: tick,
                        looped: true,
                        hold: false,
                    });
                }
                if at + 1 == beat.len {
                    self.act[a] = Some(Act::idle(tick));
                }
            }
            BeatKind::Swing => {
                let play = pb.plays[beat.play];
                if at == 0 {
                    self.act[a] = Some(Act {
                        kind: ClipKind::Swing(play.cmd),
                        start: tick,
                        looped: false,
                        hold: false,
                    });
                }
                if at == FLINCH_DELAY_TICKS && play.damage > 0 {
                    let down = play.hp_after.get(d).is_some_and(|&hp| hp <= 0);
                    self.act[d] = Some(Act {
                        kind: if down { ClipKind::Ko } else { ClipKind::Hit },
                        start: tick,
                        looped: false,
                        hold: down,
                    });
                }
            }
            BeatKind::Done => {}
        }
    }

    fn pose(&mut self, assets: &MuscleDomeAssets) {
        let scene = self.scene.get_or_insert_with(|| {
            let mut m = empty_mesh();
            let mut flat = Vec::new();
            for b in &assets.bodies {
                append(
                    &mut m,
                    &mut flat,
                    &b.mesh,
                    &crate::packet_color::textured(&b.mesh),
                );
            }
            let bases = [0, assets.bodies[0].mesh.positions.len(), m.positions.len()];
            append(&mut m, &mut flat, &assets.statics, &assets.statics_flat);
            let (mut tex, mut untex) = (Vec::new(), Vec::new());
            for t in m.indices.as_chunks::<3>().0 {
                let textured = flat.get(t[0] as usize * 4 + 3).is_some_and(|&a| a != 0);
                if textured { &mut tex } else { &mut untex }.extend_from_slice(t);
            }
            MuscleDomeScene {
                positions: m.positions,
                uvs: m.uvs,
                cba_tsb: m.cba_tsb,
                colors: m.colors,
                flat_rgba: flat,
                indices: m.indices,
                textured_indices: tex,
                untextured_indices: untex,
                camera: DomeCamera {
                    pose: cam::BOOT_POSE,
                },
                bases,
            }
        });
        for (i, (body, facing)) in assets.bodies.iter().zip(FACINGS).enumerate() {
            let mut act = self.act[i].unwrap_or(Act::idle(self.tick));
            let elapsed = self.tick.wrapping_sub(act.start);
            let mut clip = clip_for(&body.clips, act.kind);
            let mut frame = (elapsed * clip.rate / 16) as usize;
            if act.looped {
                frame %= clip.frame_count().max(1);
            } else if frame >= clip.frame_count() {
                if act.hold {
                    frame = clip.frame_count().saturating_sub(1);
                } else {
                    act = Act::idle(self.tick);
                    clip = &body.clips.idle;
                    frame = 0;
                }
            }
            self.act[i] = Some(act);
            let base = scene.bases[i];
            let n = body.mesh.positions.len();
            let yaw = facing as f32 / 4096.0 * std::f32::consts::TAU;
            pose_into(
                &mut scene.positions[base..base + n],
                &body.mesh.positions,
                &body.oids,
                clip,
                frame,
                [self.pos[i][0], yaw, self.pos[i][1]],
            );
        }
    }

    /// One battle-camera actor record for body `i`.
    fn cam_actor(&self, i: usize, assets: &MuscleDomeAssets) -> cam::BattleCamActor {
        let char_id = assets.key.1 as u8 + 1;
        cam::BattleCamActor {
            facing: FACINGS[i],
            world: [self.pos[i][0], 0.0, self.pos[i][1]],
            height: (i == 0)
                .then(|| {
                    assets
                        .heights
                        .as_ref()
                        .and_then(|h| h.height_for_char_id(char_id))
                        .map(f32::from)
                })
                .flatten(),
        }
    }

    /// Drive the battle camera script one frame over the dome's state - the
    /// phase each moment of a leg is in a battle, the acting body, its
    /// target and the formation - and store the pose on the scene.
    fn drive_camera(&mut self, session: &MuscleDomeSession, assets: &MuscleDomeAssets) {
        use crate::battle_input::CommandPhase;
        use crate::muscle_dome::DomeMenu;
        let beat = self.playback.as_ref().and_then(|p| p.beat(self.tick));
        let acting = beat.map_or(0, |b| b.attacker);
        let target = acting ^ 1;
        let monster_pos = self.pos[1];
        // (phase, the action-SM state its arm sits in, the cursor)
        let (phase, action_state, cursor) = match (beat, session.phase()) {
            (Some(b), _) => match b.kind {
                // The approach: case 6 from the seed on (`0x14`).
                BeatKind::Approach => (cam::BattleCamPhase::Action, 0x14, None),
                // The strike loop arms case 7 on every pass (`0x1E`).
                BeatKind::Swing => (cam::BattleCamPhase::Recover, 0x1E, None),
                // The done band: case 8 for an attack (`0x50`).
                BeatKind::Done => (
                    cam::done_band_phase(cam::DoneBandInputs {
                        category: cam::DONE_CATEGORY_ATTACK,
                        party_slot: acting == 0,
                        target_dead: session.hp(target) <= 0,
                        ..Default::default()
                    }),
                    0x50,
                    None,
                ),
            },
            // The battle-open hold: the far framing (retail's flow `0x0A` /
            // `0x0B` arms no close-up).
            (None, MusclePhase::Select) if session.intro_up() => {
                (cam::BattleCamPhase::Menu, 0, None)
            }
            (None, MusclePhase::Select) if self.select_framing.is_some() => {
                match self.select_framing {
                    Some(SelectFraming::Member) => (cam::BattleCamPhase::Submenu, 0, None),
                    Some(SelectFraming::Target) => (
                        cam::BattleCamPhase::TargetEnemy,
                        0,
                        Some(cam::CursorFraming::Enemy {
                            target: monster_pos,
                        }),
                    ),
                    _ => (cam::BattleCamPhase::Menu, 0, None),
                }
            }
            (None, MusclePhase::Select) => match session.menu() {
                DomeMenu::Command(c) => match c.phase {
                    // The round prompt and the Begin / Reselect confirm: the
                    // far framing with its idle orbit (case 9).
                    CommandPhase::RoundPrompt { .. }
                    | CommandPhase::CommitConfirm { .. }
                    | CommandPhase::BeginRound => (cam::BattleCamPhase::Menu, 0, None),
                    // The Attack chip's Auto | Command prompt and its cursor
                    // turn the member toward the opponent (case 1).
                    CommandPhase::AttackMode { .. } | CommandPhase::Targeting { .. } => (
                        cam::BattleCamPhase::TargetEnemy,
                        0,
                        Some(cam::CursorFraming::Enemy {
                            target: monster_pos,
                        }),
                    ),
                    // The ring: the member's close-up (case 0).
                    _ => (cam::BattleCamPhase::Submenu, 0, None),
                },
                // The direction entry, its review and the Ra-Seru list are
                // the member's own surfaces (case 0).
                DomeMenu::Input(_) | DomeMenu::Magic => (cam::BattleCamPhase::Submenu, 0, None),
            },
            // A won leg ends on case 6's battle-over arm behind the fighter.
            (None, MusclePhase::Won) => (cam::BattleCamPhase::Action, 0, None),
            // Between turns and on a lost leg the far framing holds.
            (None, _) => (cam::BattleCamPhase::Menu, 0, None),
        };
        let won = beat.is_none() && session.phase() == MusclePhase::Won;
        let slot_of = |i: usize| if i == 0 { 0u8 } else { 3 };
        let depth = legaia_engine_vm::battle_formulas::camera_height_for_frame(
            slot_of(acting),
            slot_of(target),
            legaia_engine_vm::battle_formulas::RETAIL_MONSTER_SLOT_BASE,
            |_| assets.size_class,
        );
        let mut formation = None;
        for p in &self.pos {
            cam::FormationBox::extend(&mut formation, p[0], p[1]);
        }
        let t = self.cam_actor(target, assets);
        let inputs = cam::BattleCamInputs {
            phase,
            acting: Some(self.cam_actor(acting, assets)),
            target: Some(cam::PostActionTarget {
                world: t.world,
                display_y: 0.0,
                party: target == 0,
                height: t.height,
                monster_id: if target == 1 {
                    assets.key.0.min(0xFF) as u8
                } else {
                    0
                },
                animating: false,
                live: session.hp(target) > 0,
                facing: t.facing,
                node_gone: false,
            }),
            formation,
            action: cam::ActionFraming {
                party_slot: acting == 0,
                battle_over: won,
                depth_raw: i32::from(depth),
                char_id: if acting == 0 {
                    assets.key.1 as u8 + 1
                } else {
                    0
                },
                ..Default::default()
            },
            entry_yaw: cam::battle_entry_yaw(0),
            action_state,
            camera_option: self.camera_option,
            cursor,
            ..Default::default()
        };
        cam::drive(&mut self.camera, true, inputs, u64::from(self.tick), None);
        if let (Some(scene), Some(c)) = (self.scene.as_mut(), self.camera.as_ref()) {
            scene.camera = DomeCamera { pose: c.pose() };
        }
    }

    /// Bumped whenever the static buffers or the VRAM a host holds are stale.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// The live scene, as the last [`Self::frame`] posed it.
    pub fn scene(&self) -> Option<&MuscleDomeScene> {
        self.scene.as_ref()
    }

    /// The seated dome's VRAM.
    pub fn vram(&self) -> Option<&legaia_tim::Vram> {
        self.scene.as_ref()?;
        Some(self.assets.as_ref()?.vram())
    }
}

fn clip_for(c: &BodyClips, kind: ClipKind) -> &DomeClip {
    match kind {
        ClipKind::Idle => &c.idle,
        ClipKind::Walk => &c.walk,
        ClipKind::Hit => &c.hit,
        ClipKind::Ko => &c.ko,
        ClipKind::Swing(cmd) => c.swing(cmd),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(frames: usize, rate: u32) -> DomeClip {
        DomeClip {
            frames: vec![vec![[0; 6]]; frames],
            rate,
        }
    }

    #[test]
    fn monster_clip_pick_prefers_the_close_in_attack() {
        let mk = |t: u8| MonsterAnimation {
            action_id: t,
            rate: 1,
            attach_key: 0,
            solo_flag: 0,
            impact_class: 0,
            part_count: 1,
            frame_count: 1,
            frames: vec![vec![]],
            effect_script: Vec::new(),
        };
        let anims = vec![mk(0), mk(0x20), mk(3), mk(0x21), mk(4)];
        assert_eq!(pick_monster_clips(&anims), [0, 3, 2, 4]);
        let bare = vec![mk(0), mk(7)];
        assert_eq!(pick_monster_clips(&bare), [0, 1, 0, 0]);
    }

    #[test]
    fn pose_applies_the_part_translation_then_the_world_place() {
        let mut c = clip(1, 2);
        c.frames[0][0] = [10, 20, 30, 0, 0, 0];
        let base = [[1.0, 2.0, 3.0]];
        let mut out = [[0.0; 3]];
        pose_into(&mut out, &base, &[0], &c, 0, [100.0, 0.0, -5.0]);
        assert_eq!(out[0], [111.0, 22.0, 28.0]);
        // A half-turn world yaw mirrors X and Z about the body's origin.
        pose_into(
            &mut out,
            &base,
            &[0],
            &c,
            0,
            [0.0, std::f32::consts::PI, 0.0],
        );
        assert!((out[0][0] + 11.0).abs() < 1e-3 && (out[0][2] + 33.0).abs() < 1e-3);
    }

    #[test]
    fn the_camera_looks_at_its_focus() {
        let cam = DomeCamera {
            pose: cam::BattleCamPose {
                pitch: 32.0,
                yaw: 700.0,
                tr: [0.0, 0.0, 5000.0],
                focus: [260.0, 0.0, -300.0],
            },
        };
        let m = cam.vp_raw(4.0 / 3.0);
        let p = [260.0f32, 0.0, -300.0, 1.0];
        let mut clip = [0.0f32; 4];
        for (r, o) in clip.iter_mut().enumerate() {
            *o = (0..4).map(|c| m[c * 4 + r] * p[c]).sum();
        }
        assert!(clip[3] > 0.0);
        assert!(
            (clip[0] / clip[3]).abs() < 1e-4,
            "the focus is centred in X"
        );
        let z = clip[2] / clip[3];
        assert!((0.0..=1.0).contains(&z), "depth {z} in [0, 1]");
    }

    fn play(attacker: usize) -> DomePlay {
        DomePlay {
            attacker,
            cmd: 0xC,
            power: 1,
            damage: 1,
            hp_after: [10, 10],
        }
    }

    #[test]
    fn a_turn_walks_in_once_swings_each_play_and_closes_each_string() {
        let plays = [play(0), play(0), play(1)];
        let beats = turn_timeline(&plays, true);
        let kinds: Vec<_> = beats.iter().map(|b| (b.kind, b.attacker)).collect();
        use BeatKind::*;
        assert_eq!(
            kinds,
            [
                (Approach, 0),
                (Swing, 0),
                (Swing, 0),
                (Done, 0),
                (Swing, 1),
                (Done, 1)
            ]
        );
        let total = APPROACH_TICKS + 3 * PLAY_CADENCE_TICKS + 2 * DONE_TAIL_TICKS;
        assert_eq!(turn_playback_ticks(&plays, true), total);
        assert_eq!(
            turn_playback_ticks(&plays, false),
            total - APPROACH_TICKS,
            "a later turn swings from where the pair stands"
        );
        assert_eq!(turn_playback_ticks(&[], true), 0);
        // Beats tile the span with no gap.
        for w in beats.windows(2) {
            assert_eq!(w[0].start + w[0].len, w[1].start);
        }
        assert_eq!(beat_at(&plays, true, 0).map(|b| b.kind), Some(Approach));
        assert_eq!(beat_at(&plays, true, total), None);
    }

    #[test]
    fn the_dome_camera_projects_the_seats_through_the_battle_frame() {
        // The far framing over the two seats: both on screen, apart in X.
        let mut formation = None;
        for i in 0..2 {
            let [x, z] = seat_xz(i);
            cam::FormationBox::extend(&mut formation, x, z);
        }
        let pose = cam::menu_framing(formation, 1024.0);
        let m = DomeCamera { pose }.vp_raw(4.0 / 3.0);
        let ndc = |p: [f32; 3]| {
            let c: Vec<f32> = (0..4)
                .map(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
                .collect();
            assert!(c[3] > 0.0, "in front of the eye");
            [c[0] / c[3], c[1] / c[3], c[2] / c[3]]
        };
        let [fx, fz] = seat_xz(0);
        let [mx, mz] = seat_xz(1);
        let f = ndc([fx, -200.0, fz]);
        let g = ndc([mx, -200.0, mz]);
        for p in [f, g] {
            assert!(p[0].abs() < 1.0 && p[1].abs() < 1.0, "on screen: {p:?}");
            assert!((0.0..=1.0).contains(&p[2]), "depth in [0, 1]: {p:?}");
        }
        assert!((f[0] - g[0]).abs() > 0.1, "apart: {f:?} {g:?}");
    }

    #[test]
    fn a_swing_plays_out_then_idles() {
        let clips = BodyClips {
            idle: clip(4, 2),
            walk: clip(5, 2),
            hit: clip(3, 2),
            ko: clip(3, 2),
            swings: [Some(clip(2, 16)), None, None, None],
        };
        assert_eq!(clips.swing(0xD).frame_count(), 2, "missing swing -> slot 0");
        assert_eq!(clip_for(&clips, ClipKind::Ko).frame_count(), 3);
    }
}
