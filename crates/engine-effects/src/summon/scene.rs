//! `SummonScene`: seeding the parts off the overlay and ticking them through the move VM.

use super::*;

/// A running summon: every spawned part plus the model-library base the parts'
/// mesh selectors index against.
#[derive(Debug, Clone)]
pub struct SummonScene {
    pub parts: Vec<SummonPartRuntime>,
    /// Pool index a part's `model_sel == 0` resolves to - retail
    /// `DAT_8007C018[model_sel + gp[0x754]]`; in the engine this is the offset
    /// into [`crate::world::World::global_tmd_pool`] (for Gimard, the fire
    /// mesh-set base [`crate::scene::GIMARD_TAIL_FIRE_MODEL_INDEX`]).
    pub model_base: usize,
    /// Summon origin in world units (the cast target).
    pub origin: [i16; 3],
    /// Frames ticked since spawn.
    pub frame: u32,
    /// The retail per-frame speed product the part tick's channel block
    /// scales its rates by - `DAT_1F800393 * DAT_1F80037D`
    /// ([`move_vm::integrate_draw_channels`]). Kept apart from `tick`'s
    /// `frame_delta`, which is the engine's own wait-timer drain convention
    /// ([`crate::world::EFFECT_SCENE_GRAPH_STEP`]) and not retail's scale.
    /// Defaults to [`RETAIL_CHANNEL_DELTA`]; the world re-stamps it from its
    /// live frame step before each tick.
    pub channel_delta: u16,
    /// Whether the parts' wait timers drain at retail's own rate - the
    /// [`Self::channel_delta`] product, which is what `FUN_80021DF4` takes
    /// off `+0x54` every frame (`0x80021E2C..0x80021E4C`) - rather than at
    /// `tick`'s `frame_delta`. Set for parts a cast module seats arm by arm
    /// ([`Self::push_parts`]), whose programs have to outlive the module's
    /// own countdown-paced arms: PROT 0903's fire tunnel, seated in arm 6,
    /// is still mid-program when the creature walks in (arm 11), and drained
    /// at `frame_delta` it would be gone within a few frames.
    pub retail_wait_drain: bool,
    /// The `frame_delta` the last [`Self::tick`] ran with - the drain a part
    /// [`Self::seat_run`] seats after that tick takes on its first frame.
    pub last_frame_delta: u16,
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
            channel_delta: RETAIL_CHANNEL_DELTA,
            retail_wait_drain: false,
            last_frame_delta: RETAIL_CHANNEL_DELTA,
        }
    }

    /// Seat more parts into a running scene - one `FUN_80021B04(pos, rot,
    /// record, 0x1000)` call per part, made on a later frame than the scene's
    /// first. `pos` seeds `+0x14..+0x18` and `rot` the render banks
    /// `+0x24..+0x28`, as the spawn copies its first two arguments. The
    /// record bytes may come from another image than the scene's first parts
    /// (a slot-B module spawning a battle-overlay prototype): each part keeps
    /// its own buffer.
    pub fn push_parts(
        &mut self,
        parts: &[SummonPart],
        record_bytes: &[u8],
        pos: [i16; 3],
        rot: [i16; 3],
    ) {
        self.retail_wait_drain = true;
        self.parts.extend(parts.iter().filter_map(|p| {
            let mut part = seed_part(p, record_bytes, pos)?;
            part.origin = Some(pos);
            part.state.render_24 = rot[0];
            part.state.render_26 = rot[1];
            part.state.render_28 = rot[2];
            // `andi v0,v0,0xfff` / `sh v0,0x16(a3)` at `0x80021D78`: the
            // seater also takes `rot[1]` as the part's heading `+0x96`, the
            // direction the motion block's `+0x98` speed runs along.
            part.state.tween_scale_x = rot[1] & 0xFFF;
            Some(part)
        }));
    }

    /// Pose every part the way `FUN_80021B04` seats its arguments: `rot`
    /// into the render banks `+0x24..+0x28`, `rot[1] & 0xFFF` into the
    /// heading `+0x96` (`0x80021D78`), `scale` into `+0x72` (`0x80021DAC`) -
    /// for a scene [`Self::spawn_parts`] seated at zeroed angles and unit
    /// scale.
    pub fn pose_parts(&mut self, rot: [i16; 3], scale: u16) {
        for part in &mut self.parts {
            part.state.render_24 = rot[0];
            part.state.render_26 = rot[1];
            part.state.render_28 = rot[2];
            part.state.tween_scale_x = rot[1] & 0xFFF;
            part.state.field_72 = scale;
        }
    }

    /// Run the move VM once for every part no tick has run yet - the call
    /// retail's seater makes before it returns (`FUN_80021B04`,
    /// `li v0,0x1` / `jal 0x80023070` at `0x80021DBC..0x80021DC0`). A seated
    /// part's program has therefore already set its colour word, scale and
    /// first wait by the time the frame draws it; drawn unrun, a part shows
    /// its bare mesh at full opacity for a frame (`freed_summon_mid_cast`
    /// caught the cast close-up's additive ray burst on that frame, opaque).
    ///
    /// The part tick then follows on the **same** frame: the seats come from
    /// list-0 callbacks (the action SM `FUN_80046A20` driving the module
    /// tick, the anim tick `FUN_80047430` walking the effect script), and the
    /// frame's list walk `FUN_80016444` runs list 0 (`lw a0,0x4(s0)` /
    /// `jal 0x8002519C` at `0x800165A4`) before list 1, where the parts live
    /// (`lw a0,0x8(s0)` at `0x800165C0`). Every host routes its seats after
    /// the frame's scene-graph tick, so the parts this seats get that tick
    /// here; returns which ones did.
    pub fn seat_run<H: MoveHost + ?Sized>(&mut self, host: &mut H) -> Vec<usize> {
        let mut seated = Vec::new();
        for i in 0..self.parts.len() {
            if self.seat_part(i, host) {
                self.tick_part(i, host, self.last_frame_delta);
                seated.push(i);
            }
        }
        seated
    }

    /// The seater's own VM run for part `i` if no tick has run it yet;
    /// `true` when it ran.
    fn seat_part<H: MoveHost + ?Sized>(&mut self, i: usize, host: &mut H) -> bool {
        let part = &mut self.parts[i];
        let unrun = !part.finished && part.state.pc == 2 && part.state.wait_timer == -1;
        if !unrun {
            return false;
        }
        match move_vm::actor_tick(host, &mut part.state, &part.buf, SUMMON_PART_BUDGET) {
            ActorTickOutcome::Halted | ActorTickOutcome::EndOfBuffer { .. } => {
                part.finished = true;
            }
            _ => {}
        }
        true
    }

    /// Advance every live part one frame through the move VM. `frame_delta` is
    /// the wait-timer drain (retail's per-actor anim-speed × frame-rate scalar
    /// product); a typical value keeps the parts on their authored timing.
    ///
    /// After the move-VM step, applies the **render-side translation glide**
    /// (interpreted - see `apply_translation_update`'s provenance note; the
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
    /// runs - its motion lives in the anim banks.
    ///
    /// `frame_delta` doubles as the wait-timer drain (retail's per-actor
    /// anim-speed × frame-rate scalar) and the `+0x9C` interpolation advance,
    /// matching retail where both are driven off the same per-frame delta.
    pub fn tick<H: MoveHost + ?Sized>(&mut self, host: &mut H, frame_delta: u16) {
        self.frame = self.frame.wrapping_add(1);
        self.last_frame_delta = frame_delta;
        for i in 0..self.parts.len() {
            // A part seated earlier this frame (a module arm's spawn in the
            // world tick) gets the seater's VM run first, then this frame's
            // part tick - the order retail's list walk gives it.
            self.seat_part(i, host);
            self.tick_part(i, host, frame_delta);
        }
    }

    /// One frame of the part tick `FUN_80021DF4` for part `i`.
    fn tick_part<H: MoveHost + ?Sized>(&mut self, i: usize, host: &mut H, frame_delta: u16) {
        let (channel_delta, scene_origin) = (self.channel_delta, self.origin);
        let drain = if self.retail_wait_drain {
            channel_delta
        } else {
            frame_delta
        };
        let part = &mut self.parts[i];
        {
            if part.finished {
                return;
            }
            move_vm::decrement_wait_timer(&mut part.state, drain);
            // The mode-2/6 channel block runs next in retail's part tick,
            // ahead of the move-VM call (`0x80021E78` vs `jal 0x80023070` at
            // `0x80022BA4`); it is what grows a ribbon node's `+0xC8` total.
            move_vm::integrate_draw_channels(&mut part.state, channel_delta);
            let camera_locked = is_camera_locked(&part.state);
            // A part a module arm seated on its own spawn point
            // ([`Self::push_parts`]) holds a real `+0x14..+0x18`, exactly as
            // retail's `FUN_80021B04` left it, so it moves the way retail
            // moves it: the motion block's velocity integration. The glide
            // below re-seats a part on `origin + anim bank`, which only means
            // something for the whole-set staging's summon-local offsets
            // (`vera_summon_mid_cast`: arm 0's four glows fall `+0x3E = 24`
            // per step from `y = -0x280` to retail's `-418`; the glide parked
            // them at `-616`, off the top of the frame).
            let seated = part.origin.is_some();
            if (camera_locked || seated) && crate::part_motion::runs_motion_block(&part.state) {
                // Retail's motion block (`0x800228A0..0x80022B90`), ahead of
                // the VM call: `+0x3C..+0x40` are velocities integrated into
                // `+0x14..+0x18`, never offsets from a spawn origin.
                crate::part_motion::motion_block(&mut part.state, channel_delta);
            } else if crate::part_motion::runs_motion_block(&part.state) {
                // The block's rotation / scale / level channels run for
                // every part; only the position terms stay with the glide
                // below.
                crate::part_motion::level_block(&mut part.state, channel_delta);
            }
            match move_vm::actor_tick(host, &mut part.state, &part.buf, SUMMON_PART_BUDGET) {
                ActorTickOutcome::Halted | ActorTickOutcome::EndOfBuffer { .. } => {
                    part.finished = true;
                }
                _ => {}
            }
            if seated || is_camera_locked(&part.state) {
                // The `+0x52 & 0x400` arm of `FUN_8001CF50` reads `+0x14`
                // as an eye-space offset (`+0x2C = S_b * (+0x14)`,
                // `FUN_8003D344`), so the part keeps the position its own
                // program wrote (op `0x07` WORLD_SET / `0x01` WORLD_ADD) plus
                // the motion block's integration. The glide below would move
                // it to `origin + anim bank` - the cast target's world
                // coordinates, which then lock to the eye as if they were an
                // offset.
                crate::part_motion::clamp_levels(&mut part.state);
            } else {
                crate::part_motion::clamp_levels(&mut part.state);
                apply_translation_update(
                    &mut part.state,
                    part.origin.unwrap_or(scene_origin),
                    frame_delta,
                );
            }
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
        // A halted part (`+0x10 |= 8`, op `0x08`) is killed by retail's list
        // walk and never draws again: `gimard_burning_attack` holds no node
        // for PROT 0903's breath once its program halts, where the engine
        // kept drawing the fully grown flame until the tunnel parts ended.
        self.parts
            .iter()
            .filter(|p| !p.finished && is_mesh_sel(p.model_sel))
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
                    flags_52: s.field_52,
                    morph: PartMorph::of(s),
                    scale: s.field_72,
                    colour: legaia_render_kernels::sprite_arm::ColourWord::of(s),
                }
            })
            .collect()
    }

    /// The draw-kind-4 ribbon nodes (move-VM op `0x42`) this scene holds, each
    /// rebuilt from its current state ([`crate::effect_ribbon::ribbon_mesh_for_actor`]).
    /// Ribbon carriers are transform nodes (`model_sel == -1`), so
    /// [`Self::part_draws`] never lists them: retail draws them because the
    /// render dispatcher points the actor's model list at the emitter's own
    /// output (`FUN_8001ADA4` case 4, `0x8001B08C..0x8001B0B4`).
    pub fn ribbon_draws(&self) -> Vec<crate::effect_ribbon::RibbonDraw> {
        const A: f32 = std::f32::consts::TAU / 4096.0;
        self.parts
            .iter()
            .filter(|p| !p.finished)
            .filter_map(|p| {
                let s = &p.state;
                let mesh = crate::effect_ribbon::ribbon_mesh_for_actor(s)?;
                (!mesh.indices.is_empty()).then(|| crate::effect_ribbon::RibbonDraw {
                    mesh,
                    world_pos: [s.world_x as f32, s.world_y as f32, s.world_z as f32],
                    rot: [
                        (s.render_24 as f32) * A,
                        (s.y_rot.wrapping_add(s.render_26) as f32) * A,
                        (s.render_28 as f32) * A,
                    ],
                    flags_52: s.field_52,
                })
            })
            .collect()
    }

    /// The live `0x4000` sprite-arm nodes (move-VM op `0x23`) this scene
    /// holds, each as its one-quad mesh
    /// ([`crate::effect_sprite_arm::sprite_arm_draws`]). Like ribbons they are
    /// transform nodes, so [`Self::part_draws`] never lists them.
    pub fn sprite_arm_draws(&self) -> Vec<crate::effect_ribbon::RibbonDraw> {
        self.parts
            .iter()
            .filter(|p| !p.finished)
            .flat_map(|p| crate::effect_sprite_arm::sprite_arm_draws(&p.state))
            .collect()
    }

    /// The live default-arm draw-kind-4 nodes (move-VM op `0x13`) this
    /// scene holds, each as the procedural mesh `FUN_80028158` builds
    /// ([`crate::effect_default_arm::default_arm_draw`]).
    pub fn default_arm_draws(&self) -> Vec<crate::effect_ribbon::RibbonDraw> {
        self.parts
            .iter()
            .filter(|p| !p.finished)
            .filter_map(|p| crate::effect_default_arm::default_arm_draw(&p.state))
            .collect()
    }

    /// Number of mesh-bearing parts (`is_mesh_sel`).
    pub fn mesh_part_count(&self) -> usize {
        self.parts
            .iter()
            .filter(|p| is_mesh_sel(p.model_sel))
            .count()
    }

    /// The live special render-mode nodes (`0x4000` particle / `0x4001` sound),
    /// classified for the renderer / audio host. These carry a sentinel
    /// `model_sel`, so they are absent from [`Self::part_draws`] (which only emits
    /// `is_mesh_sel` parts); this is the channel a host consumes them through -
    /// e.g. spawning a particle for [`RenderMode::Particle`] or a positional SE
    /// for [`RenderMode::SoundEmitter`]. Mirrors the `FUN_80021DF4` `+0x5A`
    /// dispatch's split of these nodes away from the mesh draw path.
    pub fn special_render_nodes(&self) -> Vec<SpecialRenderNode> {
        self.parts
            .iter()
            .filter(|p| !p.finished)
            .filter_map(|p| {
                RenderMode::from_model_sel(p.model_sel).map(|mode| SpecialRenderNode {
                    mode,
                    world_pos: [
                        p.state.world_x as f32,
                        p.state.world_y as f32,
                        p.state.world_z as f32,
                    ],
                })
            })
            .collect()
    }
}

/// The render scale (`+0x72`) a battle stager's spawn call seats a part at:
/// `0x1000` = 1.0. The few call sites that pass another immediate (`0x400`,
/// `0x800`, `0x2000`, ...) or forward a parent's `+0x72` are not carried by
/// the record set, which is keyed by record rather than by call.
pub const SPAWN_RENDER_SCALE: u16 = 0x1000;

/// Seed one part's runtime from its record. Returns `None` if the record offset
/// is past the buffer.
fn seed_part(p: &SummonPart, record_bytes: &[u8], origin: [i16; 3]) -> Option<SummonPartRuntime> {
    let rec = record_bytes.get(p.record_off..)?;
    let buf: Vec<u16> = rec
        .as_chunks::<2>()
        .0
        .iter()
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
    // The render scale `+0x72`: `FUN_80021B04` stores its fourth argument
    // there (`sh s4,0x72(s0)` at `0x80021DAC`) before the part's first
    // move-VM run, and the pool wrapper `FUN_80050ED4` forwards its own
    // `$a3` unchanged. The battle stagers' spawn calls load `li a3,0x1000`
    // at all but a handful of sites (the slot-B band 0903..0966 and PROT
    // 0898's effect-prototype spawns alike). Left at zero, a draw-kind-4
    // sprite node (`effect_sprite_arm`, op `0x23`) collapses to a point and
    // a default-arm mesh (`effect_default_arm`) to nothing.
    state.field_72 = SPAWN_RENDER_SCALE;
    Some(SummonPartRuntime {
        model_sel: p.model_sel,
        reserved: p.reserved,
        buf,
        state,
        finished: false,
        origin: None,
        tag: None,
    })
}

/// Per-axis linear interpolation - the `mode == 1` arm of `FUN_801DE4C8`
/// (`(a - b) * t / D + b` with truncating division; returns `a` when `a == b`
/// or `t >= D`). The full multi-mode interpolator (ease modes 2/3/4 included)
/// is ported as [`crate::screen_fx::interp`]; this delegates to it.
// REF: FUN_801DE4C8 (full port: screen_fx::interp)
pub(super) fn lerp_axis(target: i32, cur: i32, t: i32, d: i32) -> i32 {
    crate::screen_fx::interp(target, cur, t, d, crate::screen_fx::InterpMode::Linear)
}
