//! Seru-magic **summon scene-graph driver** (engine stand-in render — see the
//! reconciliation note below; this is **not** the faithful player-summon path).
//!
//! The per-summon stager overlay (extraction PROT 903..=913 = raw TOC
//! `0x389..=0x393`; Gimard *Burning Attack* = 903)
//! carries real move-VM scene-graph part records, recovered by
//! [`legaia_asset::summon_overlay`]. This module drives them: it seeds one
//! move-VM [`ActorState`] per part from its record and ticks every part through
//! the **already-ported move VM** each frame, exactly as the retail spawn helper
//! `FUN_80021B04` stages a part (`actor[+0x48]` = record move-buffer base,
//! `actor[+0x70] = 2` PC → bytecode at `record+4`) and the per-frame actor tick
//! `FUN_80021DF4` runs `FUN_80023070` on it.
//!
//! ## Reconciliation: this is not how retail renders the player summon
//!
//! A live PCSX-Redux trace of a player Gimard *Burning Attack* cast (scenarios
//! `gimard_summon_start` / `_visible` / `gimard_burning_attack`) shows the
//! rendered summon is an **ordinary battle actor**, not this move-VM scene-graph:
//! across all three phases `FUN_801F7088` fired **0×**, the move VM
//! `FUN_80023070` fired only **2-3×** (trace noise, not a per-part driver), and
//! the **battle per-actor draw `FUN_80048A08` fired 35-64×/frame** → the
//! per-object rigid-TRS keyframe decoder `FUN_8004998C` → cluster-A
//! `FUN_80043390`. So the player summon is posed exactly like an enemy monster
//! body (per-object rigid TRS keyframes), and the **faithful render path is the
//! battle TRS-keyframe draw already ported in
//! [`legaia_engine_vm::anim_vm`]** (`FUN_80048A08` / `FUN_8004998C`), *not* this
//! [`SummonScene`].
//!
//! The faithful **player** summon render is already wired: a cast routes through
//! [`crate::world::World::request_summon_spawn`] →
//! [`crate::world::World::take_pending_summon_spawn`], and the host spawns the
//! summon's namesake `battle_data` creature ([`summon_creature_id`]) as an
//! ordinary battle actor — mesh + texture via
//! [`legaia_asset::monster_archive::battle_render_mesh`], animation via
//! [`legaia_asset::monster_archive::idle_animation`] →
//! [`crate::battle_anim::MonsterAnimPlayer`] → the rigid TRS-keyframe draw
//! (`FUN_80048A08` / `FUN_8004998C`, ported in `legaia_engine_vm::anim_vm`). That
//! is the production summon visual.
//!
//! [`SummonScene`] is therefore *not* the production render — it is kept because
//! (1) it is the validated parser/driver for the genuinely-on-disc move-VM stager
//! records (disc-gated `summon_scene_real` — every Gimard part runs the move VM
//! without an unimplemented opcode); (2) it backs the non-battle debug spawn
//! (`play-window` `G` outside battle) that exercises the move-VM driver; and
//! (3) the **enemy** Gimard *Fire Tail* boss move is untraced and may still use
//! the overlay/move-VM path, so this remains its candidate model. The live trace
//! that resolved the player path covers the **player** "Burning Attack" only. See
//! the open-rev-eng-threads "Seru-magic summon visual" row for the full
//! reconciliation.
//!
//! ## What is faithful vs. interpreted (within this stand-in model)
//!
//! - **Faithful:** the per-part animation *computation*. Each part's move-VM
//!   program runs through [`legaia_engine_vm::move_vm`] verbatim — the same
//!   opcode handlers, wait-timer gate, and tween/anim-bank state the retail move
//!   VM produces. (Validated: every Gimard *Tail Fire* part record runs without
//!   hitting an unimplemented opcode.)
//! - **Interpreted (translation glide):** each frame the world position
//!   (`+0x14/16/18`) glides toward the move-VM anim-bank target
//!   (`anim_3c/3e/40`, op `0x00`, `v << 3`) over the time ratio
//!   `+0x9C / +0x9E`, latching exactly on completion. The lerp *shape* reuses
//!   `FUN_801F811C` / `FUN_801DE4C8` mode 1 — but the retail `FUN_801F811C` is
//!   **not a summon-part position update**: a full static decode of PROT 0900
//!   at the correct link base resolved it as the per-frame handler of the 2D
//!   **screen-mask (iris) widget**, whose four tweened channels are screen-rect
//!   edges and whose "4 render quads" are the black border bands
//!   (see [`crate::screen_fx`], the faithful port). Two deviations of this
//!   glide from the retail tween math, kept deliberately for the stand-in:
//!   the engine applies it to move-VM part positions (retail does not), and it
//!   re-lerps **iteratively from the updated current value** each frame,
//!   whereas the retail widget tween re-interpolates from a start value that
//!   stays fixed until the latch.
//! - **Interpreted (rotation):** the part's *orientation*. Within this stand-in
//!   model [`SummonScene::part_draws`] derives Euler angles from the move-VM
//!   rotation banks. This is the engine's interpretation of the scene-graph
//!   model, **not** a faithful port of retail's player-summon orientation —
//!   which the live trace resolved as the per-object TRS keyframes that
//!   `FUN_8004998C` decodes (the battle-actor path above), not a move-VM /
//!   `FUN_801F7088` rotation. (PROT 0900's `RotMatrixX/Y/Z` + camera-angle
//!   `FUN_801F7088` build is the **world-map top-view tile renderer** aliasing
//!   the same `0x801Fxxxx` band, not the battle-summon code.)

use legaia_asset::summon_overlay::{SummonOverlay, SummonPart};
use legaia_engine_vm::move_vm::{self, ActorState, ActorTickOutcome, MoveHost};

/// Per-frame opcode budget for one part's move-VM tick (defensive cap; retail
/// has no explicit limit but breaks on WAIT / HALT / end-of-buffer).
pub const SUMMON_PART_BUDGET: usize = 256;

/// Player Seru-magic spell-id range that resolves to a per-summon overlay at
/// the battle-action cast band (`FUN_801E295C` state `0x29`: `actor[+0x1DF] >=
/// 0x81`). Gimard *Burning Attack* = `0x81` (the enemy boss *Fire Tail* is a
/// different, non-stager path). The `0x82..=0x8B` legs each carry a committed
/// regression oracle (mid-cast loader-B id + slot-B-resident stager;
/// disc+library-gated `summon_binding_base_high`); `0x81` is PCSX-side.
pub const SERU_SUMMON_IDS: std::ops::RangeInclusive<u8> = 0x81..=0x8B;

/// Evolved-Seru player cast block (`0x8C..=0x95`), the contiguous continuation
/// of [`SERU_SUMMON_IDS`] under the *same* `(id - 0x81) + 903` arithmetic
/// (`0x8C → 914 .. 0x95 → 923`, `summon_overlay::EVOLVED_SUMMON_STAGER_PROT`).
/// Every entry is statically confirmed stager-shaped, and the arithmetic is
/// capture-pinned on **both** sides of the gap (`0x8B → 913`, `0x99 → 927`).
/// Three legs are now *individually* capture-pinned too — `0x8C → 914`,
/// `0x8D → 915`, `0x8F → 917` (the `{gola_gola,mushura,barra}_summon_mid_cast`
/// states, loader-B id read mid-cast + the stager byte-resident at slot B;
/// disc+library-gated `evolved_summon_binding`) — so the block is no longer
/// purely predicted; the remaining legs ride the same bracketed run. Two of
/// these stagers carry `0x4000` render-mode nodes (`0x8E → 916`, `0x93 → 921`).
pub const EVOLVED_SUMMON_IDS: std::ops::RangeInclusive<u8> = 0x8C..=0x95;

/// High summon block: Evil Seru Magic (`0x99` — the creature resolves
/// per-cast, e.g. Juggernaut), the Sim-Seru summons Palma / Mule / Horn /
/// Jedo (`0x9A..=0x9D`), and the Ra-Seru summons Meta / Terra / Ozma
/// (`0x9E..=0xA0`). All eight legs each carry a committed regression oracle
/// (mid-cast loader-B id + slot-B-resident stager; disc+library-gated
/// `summon_binding_base_high`).
pub const HIGH_SUMMON_IDS: std::ops::RangeInclusive<u8> = 0x99..=0xA0;

/// PROT entry holding the per-summon stager overlay for a Seru-magic `spell_id`,
/// or `None` if `spell_id` is not a summon. Retail: `FUN_8003EC70(id - 0x79)`
/// loads raw-TOC index `(id - 0x79) + 0x381` — the in-RAM TOC at `0x801C70F0`
/// is raw `PROT.DAT` from byte 0 (header included), so the extraction entry
/// sits 2 below the raw index: `0x81..=0x8B → 903..=913` and
/// `0x99..=0xA0 → 927..=934` (see `docs/formats/prot.md` § index spaces).
///
/// Capture-pinned for **every id in the base and high blocks**: one mid-cast
/// save state per spell holds the battle overlay's loader-B current-id
/// (`0x8007BC4C`) at exactly `spell_id - 0x79` with the predicted entry
/// byte-resident at the slot-B link base (the `<seru>_summon_mid_cast` +
/// `gimard_summon_*` scenarios in `scripts/scenarios.toml`). Titled entries
/// head with the ASCII display name of the summon's attack (0907 Nighto
/// "Hell's Music", 0927 Juggernaut "Dark Eclipse"); the high-block entries
/// otherwise head with a pre-linked slot-B pointer table. The
/// [`EVOLVED_SUMMON_IDS`] block bridging the two is statically stager-shaped
/// with three legs capture-pinned (`0x8C`/`0x8D`/`0x8F`); the rest ride the same
/// bracketed run. The id-`0x96..=0x98` gap is *not* a player summon (those
/// resolve to 924..926 under the enemy `895 + id` formula); it returns `None`.
pub fn summon_stager_prot_entry(spell_id: u8) -> Option<u32> {
    if SERU_SUMMON_IDS.contains(&spell_id) || EVOLVED_SUMMON_IDS.contains(&spell_id) {
        // One contiguous run: 0x81 → 903 .. 0x95 → 923.
        Some(903 + (spell_id - 0x81) as u32)
    } else if HIGH_SUMMON_IDS.contains(&spell_id) {
        Some(927 + (spell_id - 0x99) as u32)
    } else {
        None
    }
}

/// `battle_data` (PROT 867) creature id whose mesh + per-object animation the
/// player Seru-magic summon `spell_id` reuses, or `None` if not a summon / not
/// found. **The player summon spawns the namesake creature** (the Gimard spell
/// summons the Gimard creature, Theeder→Theeder, …), so the faithful render is
/// the ordinary battle TRS-keyframe path applied to that creature's
/// monster-archive block (mesh via [`legaia_asset::monster_archive::battle_render_mesh`],
/// animation via [`legaia_asset::monster_archive::idle_animation`] →
/// [`crate::battle_anim::MonsterAnimPlayer`] → `tmd_to_vram_mesh_posed_rot`) —
/// **not** the move-VM scene-graph the [`SummonScene`] stand-in drives.
///
/// Resolved by matching the spell's display name ([`crate::retail_magic`]) to a
/// `battle_data` record name, so the `"$2"`/`"$3"` higher-level enemy variants
/// (different names) are excluded and the base creature is chosen. Pinned from
/// the fingerprint-verified `gimard_summon_visible` save: the live summon
/// actor's 11-part idle byte-matches `battle_data` id 10 ("Gimard"). The
/// disc-verified map is Gimard `0x81`→10, Theeder `0x82`→25, Vera `0x83`→28,
/// Gizam `0x84`→55, Nighto `0x85`→49, Zenoir `0x86`→64, Viguro `0x87`→74,
/// Swordie `0x88`→86, Orb `0x89`→83, Freed `0x8a`→92, Nova `0x8b`→95.
pub fn summon_creature_id(spell_id: u8, battle_data_entry: &[u8]) -> Option<u16> {
    let name = crate::retail_magic::get(spell_id)?.name;
    let slots = legaia_asset::monster_archive::slot_count(battle_data_entry) as u16;
    (1..=slots).find(|&id| {
        legaia_asset::monster_archive::record(battle_data_entry, id)
            .ok()
            .flatten()
            .is_some_and(|r| r.name == name)
    })
}

/// Upper bound on a `model_sel` that names a real mesh (`DAT_8007C018[model_sel
/// + base]`). The model library is small (~30 entries), so a part whose
/// `model_sel` is `-1` (transform node) or a large sentinel (`0x1000`, `0x4000`,
/// `0x4001` — special render-mode markers, per the summon-overlay decode) binds
/// no mesh. Mesh parts have `0 <= model_sel < MAX_MESH_SEL`.
pub const MAX_MESH_SEL: i16 = 0x100;

/// `true` when `model_sel` names a real library mesh (vs. a transform/pivot node
/// or a special-mode sentinel).
fn is_mesh_sel(model_sel: i16) -> bool {
    (0..MAX_MESH_SEL).contains(&model_sel)
}

/// Runtime state of one staged summon part.
#[derive(Debug, Clone)]
pub struct SummonPartRuntime {
    /// `record[+0]` mesh selector (`-1` = transform/pivot node).
    pub model_sel: i16,
    /// `record[+2]` flags.
    pub flags: u16,
    /// The part's move buffer (its record bytes as a u16 array; PC indexes this).
    pub buf: Vec<u16>,
    /// Live move-VM actor state.
    pub state: ActorState,
    /// `true` once the part's program halted or ran off its buffer.
    pub finished: bool,
}

/// A running summon: every spawned part plus the model-library base the parts'
/// mesh selectors index against.
#[derive(Debug, Clone)]
pub struct SummonScene {
    pub parts: Vec<SummonPartRuntime>,
    /// Pool index a part's `model_sel == 0` resolves to — retail
    /// `DAT_8007C018[model_sel + gp[0x754]]`; in the engine this is the offset
    /// into [`crate::world::World::global_tmd_pool`] (for Gimard, the fire
    /// mesh-set base [`crate::scene::GIMARD_TAIL_FIRE_MODEL_INDEX`]).
    pub model_base: usize,
    /// Summon origin in world units (the cast target).
    pub origin: [i16; 3],
    /// Frames ticked since spawn.
    pub frame: u32,
}

/// One mesh-bearing part's render draw. The transform is the engine's
/// interpretation of the move-VM state (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SummonPartDraw {
    /// Index into [`crate::world::World::global_tmd_pool`].
    pub model_index: usize,
    /// World position (move-VM `world_x/y/z`).
    pub world_pos: [f32; 3],
    /// Euler XYZ rotation in radians (from the move-VM rotation banks).
    pub rot: [f32; 3],
}

impl SummonScene {
    /// Spawn every part of a parsed summon overlay at `origin`, seeding each
    /// part's move-VM state from its record (PC = 2 → bytecode at `record+4`,
    /// mirroring `FUN_80021B04`). `record_bytes` is the stager overlay's raw
    /// bytes (e.g. PROT 0903), the same buffer [`SummonOverlay`] was parsed from.
    pub fn spawn(
        overlay: &SummonOverlay,
        record_bytes: &[u8],
        model_base: usize,
        origin: [i16; 3],
    ) -> Self {
        Self::spawn_parts(&overlay.parts, record_bytes, model_base, origin)
    }

    /// Spawn from an explicit set of [`SummonPart`] records (rather than a whole
    /// [`SummonOverlay`]). Used by the battle move-power effect-FX path, whose
    /// records come from the `0x801f6324` prototype-pointer table
    /// ([`legaia_asset::summon_overlay::parse_records_at`]) rather than the
    /// stager's `jal`-site scan, but stage through the identical
    /// `FUN_80021B04` → move-VM machinery. `record_bytes` is the buffer the parts
    /// were parsed from (the move-FX records live in the battle-action overlay,
    /// PROT 0898); `model_base` is the pool index `model_sel == 0` resolves to
    /// (the captured battle base `gp[0x754] = 3`).
    pub fn spawn_parts(
        parts: &[SummonPart],
        record_bytes: &[u8],
        model_base: usize,
        origin: [i16; 3],
    ) -> Self {
        let parts = parts
            .iter()
            .filter_map(|p| seed_part(p, record_bytes, origin))
            .collect();
        Self {
            parts,
            model_base,
            origin,
            frame: 0,
        }
    }

    /// Advance every live part one frame through the move VM. `frame_delta` is
    /// the wait-timer drain (retail's per-actor anim-speed × frame-rate scalar
    /// product); a typical value keeps the parts on their authored timing.
    ///
    /// After the move-VM step, applies the **render-side translation glide**
    /// (interpreted — see [`apply_translation_update`]'s provenance note; the
    /// retail `FUN_801F811C` is the screen-mask widget handler, faithfully
    /// ported as [`crate::screen_fx::MaskWidget`]): the part's world
    /// position LERPs toward the move-VM anim-bank target (`anim_3c/3e/40`, set
    /// by op `0x00 ANIM_BANK_SET` as `v << 3`) over the time ratio
    /// `+0x9C / +0x9E`, with `+0x9C` advanced by `frame_delta` each frame (the
    /// engine's analog of retail's `DAT_1F800393`) and clamped to `+0x9E`. On
    /// reaching `+0x9E` the position latches exactly to the target and `+0x9E`
    /// is cleared. The anim banks are summon-local offsets, so the engine adds
    /// [`Self::origin`] to seat the part at the cast target. This is why a
    /// summon part animates off the spawn point even though no `WORLD_ADD` op
    /// runs — its motion lives in the anim banks.
    ///
    /// `frame_delta` doubles as the wait-timer drain (retail's per-actor
    /// anim-speed × frame-rate scalar) and the `+0x9C` interpolation advance,
    /// matching retail where both are driven off the same per-frame delta.
    pub fn tick<H: MoveHost + ?Sized>(&mut self, host: &mut H, frame_delta: u16) {
        self.frame = self.frame.wrapping_add(1);
        for part in &mut self.parts {
            if part.finished {
                continue;
            }
            move_vm::decrement_wait_timer(&mut part.state, frame_delta);
            match move_vm::actor_tick(host, &mut part.state, &part.buf, SUMMON_PART_BUDGET) {
                ActorTickOutcome::Halted | ActorTickOutcome::EndOfBuffer { .. } => {
                    part.finished = true;
                }
                _ => {}
            }
            apply_translation_update(&mut part.state, self.origin, frame_delta);
        }
    }

    /// `true` once every part has halted / ended.
    pub fn finished(&self) -> bool {
        self.parts.iter().all(|p| p.finished)
    }

    /// Render draws for the mesh-bearing parts (`0 <= model_sel < MAX_MESH_SEL`). Transform
    /// nodes (`model_sel == -1`) carry no mesh and are skipped. The transform is
    /// the engine's interpretation (see module docs).
    pub fn part_draws(&self) -> Vec<SummonPartDraw> {
        // PSX 12-bit angle (4096 = 360°) → radians.
        const A: f32 = std::f32::consts::TAU / 4096.0;
        self.parts
            .iter()
            .filter(|p| is_mesh_sel(p.model_sel))
            .map(|p| {
                let s = &p.state;
                SummonPartDraw {
                    model_index: self.model_base.wrapping_add(p.model_sel as usize),
                    world_pos: [s.world_x as f32, s.world_y as f32, s.world_z as f32],
                    rot: [
                        (s.render_24 as f32) * A,
                        (s.y_rot.wrapping_add(s.render_26) as f32) * A,
                        (s.render_28 as f32) * A,
                    ],
                }
            })
            .collect()
    }

    /// Number of mesh-bearing parts (`is_mesh_sel`).
    pub fn mesh_part_count(&self) -> usize {
        self.parts
            .iter()
            .filter(|p| is_mesh_sel(p.model_sel))
            .count()
    }
}

/// Seed one part's runtime from its record. Returns `None` if the record offset
/// is past the buffer.
fn seed_part(p: &SummonPart, record_bytes: &[u8], origin: [i16; 3]) -> Option<SummonPartRuntime> {
    let rec = record_bytes.get(p.record_off..)?;
    let buf: Vec<u16> = rec
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let mut state = ActorState::new();
    // FUN_80021B04 stages the move buffer at PC = 2 (u16 units) → the bytecode
    // begins at record+4, just past [model_sel][flags].
    state.pc = 2;
    state.world_x = origin[0];
    state.world_y = origin[1];
    state.world_z = origin[2];
    state.world_y_mirror = origin[1];
    // Negative wait-timer so the gate runs the VM on the first frame.
    state.wait_timer = -1;
    Some(SummonPartRuntime {
        model_sel: p.model_sel,
        flags: p.flags,
        buf,
        state,
        finished: false,
    })
}

/// Per-axis linear interpolation — the `mode == 1` arm of `FUN_801DE4C8`
/// (`(a - b) * t / D + b` with truncating division; returns `a` when `a == b`
/// or `t >= D`). The full multi-mode interpolator (ease modes 2/3/4 included)
/// is ported as [`crate::screen_fx::interp`]; this delegates to it.
// REF: FUN_801DE4C8 (full port: screen_fx::interp)
fn lerp_axis(target: i32, cur: i32, t: i32, d: i32) -> i32 {
    crate::screen_fx::interp(target, cur, t, d, crate::screen_fx::InterpMode::Linear)
}

/// Render-side translation glide for the stand-in scene-graph: each axis
/// tweens toward `origin + anim bank` over `+0x9C / +0x9E`, snapping when no
/// tween is active (`+0x9E == 0`) and latching exactly on completion.
///
/// Provenance note: this glide **reuses the tween shape of `FUN_801F811C`**
/// (advance-clamp-lerp-latch over the `+0x9C/+0x9E` clock with the
/// `FUN_801DE4C8` mode-1 lerp + `FUN_801DE648` sized store), but the retail
/// function is **not** a summon-part position update — it is the screen-mask
/// (iris) widget handler of the PROT-0900 screen-effect family, whose four
/// channels are 2D rect edges and whose quads are the black border bands. The
/// faithful port lives in [`crate::screen_fx::MaskWidget`]. As an engine
/// interpretation this glide also diverges from the retail tween math in one
/// respect: it re-lerps from the **updated** current position each frame
/// (a slightly eased trajectory), where the retail widget re-interpolates
/// from a start value held fixed until the latch.
///
/// The anim banks are summon-local offsets, so the engine adds `origin` (the
/// cast target) to the lerp endpoint to seat the part in world space.
/// `frame_delta` plays the role of retail's per-frame `DAT_1F800393`.
// REF: FUN_801F811C (faithful port: screen_fx::MaskWidget)
// REF: FUN_801DE648 (sized store of the lerp result; here a plain field write)
fn apply_translation_update(state: &mut ActorState, origin: [i16; 3], frame_delta: u16) {
    // Retail reads `*(i16)(actor+0x9C)` (low half of the i32 field_9c) and
    // `*(i16)(actor+0x9E)` (field_9e). Targets are summon-local anim banks +
    // origin (the cast target).
    let target = [
        origin[0].wrapping_add(state.anim_3c),
        origin[1].wrapping_add(state.anim_3e),
        origin[2].wrapping_add(state.anim_40),
    ];

    let duration = state.field_9e as i16;
    if duration == 0 {
        // No active tween → snap to target (the `+0x9E == 0` arm).
        state.world_x = target[0];
        state.world_y = target[1];
        state.world_z = target[2];
        state.world_y_mirror = state.world_y;
        return;
    }

    // Advance the keyframe cursor by the per-frame delta and clamp to duration.
    let mut t = (state.field_9c & 0xFFFF) as i16;
    t = t.wrapping_add(frame_delta as i16);
    if duration < t {
        t = duration;
    }
    state.field_9c = (state.field_9c & !0xFFFF) | (t as u16 as i32);

    // Per-axis LERP toward the (origin + anim-bank) target. Skips an axis whose
    // current value already equals the target (the `target != cur` guard in
    // retail), matching `FUN_801DE4C8`'s early `a == b` return.
    let t_i = t as i32;
    let d_i = duration as i32;
    let cur = [state.world_x, state.world_y, state.world_z];
    for (i, &c) in cur.iter().enumerate() {
        if target[i] != c {
            let v = lerp_axis(target[i] as i32, c as i32, t_i, d_i) as i16;
            match i {
                0 => state.world_x = v,
                1 => state.world_y = v,
                _ => state.world_z = v,
            }
        }
    }
    state.world_y_mirror = state.world_y;

    // Reached the endpoint → latch exactly to the target and clear duration
    // (`+0x9E == 0` means "no active tween" on the next frame).
    if t == duration {
        state.world_x = target[0];
        state.world_y = target[1];
        state.world_z = target[2];
        state.world_y_mirror = state.world_y;
        state.field_9e = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::summon_overlay::{SummonOverlay, SummonPart};

    /// A synthetic overlay: one transform node + one mesh part with a tiny
    /// move-VM program (`0x00 ANIM_BANK_SET 1,2,3` then `0x08 HALT`) — the
    /// anim banks are the per-part position the render-side latch reads.
    fn synthetic() -> (Vec<u8>, SummonOverlay) {
        // Record layout: [i16 model_sel][u16 flags][u16 move-VM bytecode...].
        // Build two records back-to-back in one byte buffer.
        let mut bytes = Vec::new();
        // record 0 @ 0x00: model_sel = -1 (transform node)
        let r0 = bytes.len();
        bytes.extend_from_slice(&(-1i16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes()); // flags
        bytes.extend_from_slice(&0x08u16.to_le_bytes()); // HALT
        bytes.extend_from_slice(&0u16.to_le_bytes()); // pad to even record
        // record 1 @ next: model_sel = 0 (mesh), program: ANIM_BANK_SET 1,2,3 ; HALT.
        // Op 0x00 sets anim_3c/3e/40 = v << 3 -> (8, 16, 24).
        let r1 = bytes.len();
        bytes.extend_from_slice(&0i16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0x00u16.to_le_bytes()); // ANIM_BANK_SET
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&3u16.to_le_bytes());
        bytes.extend_from_slice(&0x08u16.to_le_bytes()); // HALT

        let overlay = SummonOverlay {
            link_base: 0x801F_69D8,
            spawn_sites: 2,
            parts: vec![
                SummonPart {
                    record_off: r0,
                    model_sel: -1,
                    flags: 0,
                    bytecode: (r0 + 4)..r1,
                },
                SummonPart {
                    record_off: r1,
                    model_sel: 0,
                    flags: 0,
                    bytecode: (r1 + 4)..bytes.len(),
                },
            ],
        };
        (bytes, overlay)
    }

    struct H;
    impl MoveHost for H {}

    #[test]
    fn spawns_one_state_per_part() {
        let (bytes, ov) = synthetic();
        let scene = SummonScene::spawn(&ov, &bytes, 26, [100, 200, 300]);
        assert_eq!(scene.parts.len(), 2);
        assert_eq!(scene.mesh_part_count(), 1, "one mesh part (model_sel >= 0)");
        // Each part seeded at the origin with PC = 2.
        for part in &scene.parts {
            assert_eq!(part.state.pc, 2);
            assert_eq!(part.state.world_x, 100);
        }
    }

    #[test]
    fn tick_latches_anim_banks_into_world_pos_plus_origin() {
        let (bytes, ov) = synthetic();
        let mut scene = SummonScene::spawn(&ov, &bytes, 26, [100, 200, 300]);
        let mut host = H;
        scene.tick(&mut host, 0x1000);
        // The mesh part ran ANIM_BANK_SET 1,2,3 (-> anim = 8,16,24). With the
        // keyframe gate held (field_9c == field_9e == 0), the render-side latch
        // overwrites world pos with origin + anim bank.
        let mesh = scene.parts.iter().find(|p| p.model_sel == 0).unwrap();
        assert_eq!(
            (mesh.state.anim_3c, mesh.state.anim_3e, mesh.state.anim_40),
            (8, 16, 24),
            "op 0x00 set the anim banks to v << 3"
        );
        assert_eq!(
            (mesh.state.world_x, mesh.state.world_y, mesh.state.world_z),
            (108, 216, 324),
            "latch: world = origin + anim bank"
        );
        // Both tiny programs HALT on the first frame.
        scene.tick(&mut host, 0x1000);
        assert!(scene.finished(), "both parts halted");
    }

    #[test]
    fn lerp_axis_matches_fun_801de4c8_mode1() {
        // Port of `FUN_801DE4C8(a, b, t, D, 1)`: returns `b + (a-b)*t/D` with
        // integer truncating div, and `a` exactly when `t >= D` or `a == b`.
        assert_eq!(lerp_axis(100, 0, 0, 10), 0, "t=0 → start");
        assert_eq!(lerp_axis(100, 0, 5, 10), 50, "midpoint");
        assert_eq!(lerp_axis(100, 0, 10, 10), 100, "t=D → exact target");
        assert_eq!(lerp_axis(100, 0, 11, 10), 100, "t>D → clamped to target");
        assert_eq!(lerp_axis(50, 50, 3, 10), 50, "a==b → target");
        // Truncation toward zero (retail `div`): (10-0)*3/10 = 3.
        assert_eq!(lerp_axis(10, 0, 3, 10), 3);
        // Negative direction truncates toward zero too: (0-10)*3/10 = -3.
        assert_eq!(lerp_axis(0, 10, 3, 10), 7);
    }

    #[test]
    fn tick_drives_the_lerp_update_for_a_live_part() {
        // The integration path: one `tick` runs the move VM (which sets the
        // anim banks to 8/16/24) and then `apply_translation_update`. With a
        // fresh part (`+0x9E == 0`), that is the snap-to-target arm.
        let (bytes, ov) = synthetic();
        let mut scene = SummonScene::spawn(&ov, &bytes, 26, [100, 200, 300]);
        let mut host = H;
        scene.tick(&mut host, 0x1000);
        let mesh = scene.parts.iter().find(|p| p.model_sel == 0).unwrap();
        assert_eq!(
            (mesh.state.world_x, mesh.state.world_y, mesh.state.world_z),
            (108, 216, 324),
            "tick wires apply_translation_update (snap arm: origin + anim bank)"
        );
    }

    #[test]
    fn lerp_update_glides_toward_target_and_lands_exactly_on_completion() {
        // Drive `apply_translation_update` (the FUN_801F811C-shaped glide) across
        // several frames with an active interpolation window (+0x9E = duration)
        // and assert the world position moves monotonically toward the
        // (origin + anim-bank) target and reaches it exactly on completion.
        let origin = [100_i16, 200, 300];
        let mut state = ActorState::new();
        // Anim banks (summon-local target offsets); the move VM op 0x00 would
        // have set these to v << 3 = (8, 16, 24).
        state.anim_3c = 8;
        state.anim_3e = 16;
        state.anim_40 = 24;
        // Start at the bare origin, with a 40-tick interpolation window.
        state.world_x = origin[0];
        state.world_y = origin[1];
        state.world_z = origin[2];
        state.world_y_mirror = origin[1];
        state.field_9c = 0;
        state.field_9e = 40;

        let target = [108_i16, 216, 324]; // origin + (8, 16, 24)

        // One step (t advances 0 → 10, D = 40): x = 100 + (108-100)*10/40 = 102.
        apply_translation_update(&mut state, origin, 10);
        assert_eq!(
            (state.world_x, state.world_y, state.world_z),
            (102, 204, 306),
            "first lerp step is partial, not a snap"
        );
        assert_ne!(state.field_9e, 0, "still mid-tween");
        assert_eq!(state.world_y_mirror, state.world_y, "y mirror tracks y");

        // Continue ticking; assert monotonic approach and exact landing.
        let mut prev = [state.world_x, state.world_y, state.world_z];
        let mut reached = false;
        for step in 0..6 {
            apply_translation_update(&mut state, origin, 10);
            let pos = [state.world_x, state.world_y, state.world_z];
            for axis in 0..3 {
                assert!(
                    pos[axis] >= prev[axis] && pos[axis] <= target[axis],
                    "axis {axis} step {step}: {} not in [{}, {}]",
                    pos[axis],
                    prev[axis],
                    target[axis]
                );
            }
            if pos == target {
                reached = true;
                assert_eq!(state.field_9e, 0, "duration cleared on completion");
                break;
            }
            prev = pos;
        }
        assert!(reached, "part reached the target exactly on completion");

        // After completion, +0x9E == 0, so the next update is the snap arm and
        // the part stays pinned exactly on the target.
        apply_translation_update(&mut state, origin, 10);
        assert_eq!(
            (state.world_x, state.world_y, state.world_z),
            (target[0], target[1], target[2]),
            "post-completion snap holds on target"
        );
    }

    #[test]
    fn latch_holds_part_at_origin_when_anim_banks_are_zero() {
        // The transform-node part (no anim ops) stays at the origin after the
        // latch (origin + 0).
        let (bytes, ov) = synthetic();
        let mut scene = SummonScene::spawn(&ov, &bytes, 26, [40, 50, 60]);
        let mut host = H;
        scene.tick(&mut host, 0x1000);
        let node = &scene.parts[0];
        assert_eq!(
            (node.state.world_x, node.state.world_y, node.state.world_z),
            (40, 50, 60)
        );
    }

    #[test]
    fn summon_prot_entry_maps_the_seru_block() {
        assert_eq!(summon_stager_prot_entry(0x81), Some(903)); // Gimard Burning Attack
        assert_eq!(summon_stager_prot_entry(0x8B), Some(913)); // last base-block summon
        assert_eq!(summon_stager_prot_entry(0x80), None); // below the block
        assert_eq!(summon_stager_prot_entry(0x27), None); // a monster attack id
    }

    #[test]
    fn summon_prot_entry_maps_the_evolved_block() {
        // The evolved-Seru block bridges the base and high blocks on one
        // continuous arithmetic run; stager-shaped, three legs capture-pinned.
        assert_eq!(summon_stager_prot_entry(0x8C), Some(914)); // Gola Gola (capture-pinned)
        assert_eq!(summon_stager_prot_entry(0x8D), Some(915)); // Mushura (capture-pinned)
        assert_eq!(summon_stager_prot_entry(0x8E), Some(916)); // carries 0x4000 render-mode nodes
        assert_eq!(summon_stager_prot_entry(0x8F), Some(917)); // Barra (capture-pinned)
        assert_eq!(summon_stager_prot_entry(0x93), Some(921)); // carries 0x4000 render-mode nodes
        assert_eq!(summon_stager_prot_entry(0x95), Some(923)); // last evolved-Seru leg
        // The id-0x96..0x98 gap is the enemy 895+id block, not a player summon.
        assert_eq!(summon_stager_prot_entry(0x96), None);
        assert_eq!(summon_stager_prot_entry(0x98), None);
    }

    #[test]
    fn summon_prot_entry_maps_the_high_block() {
        assert_eq!(summon_stager_prot_entry(0x99), Some(927)); // Evil Seru Magic (Juggernaut cast)
        assert_eq!(summon_stager_prot_entry(0x9A), Some(928)); // Palma
        assert_eq!(summon_stager_prot_entry(0x9D), Some(931)); // Jedo
        assert_eq!(summon_stager_prot_entry(0x9E), Some(932)); // Meta
        assert_eq!(summon_stager_prot_entry(0xA0), Some(934)); // Ozma
        assert_eq!(summon_stager_prot_entry(0x98), None); // below the high block
        assert_eq!(summon_stager_prot_entry(0xA1), None); // above the high block
    }

    #[test]
    fn part_draws_map_model_sel_against_the_base() {
        let (bytes, ov) = synthetic();
        let scene = SummonScene::spawn(&ov, &bytes, 26, [0, 0, 0]);
        let draws = scene.part_draws();
        assert_eq!(draws.len(), 1, "only the mesh part draws");
        assert_eq!(draws[0].model_index, 26, "model_sel 0 + base 26");
    }
}
