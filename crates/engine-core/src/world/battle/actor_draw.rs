//! The battle per-actor draw's per-object colour rule, resolved against
//! live world state for the two hosts' battle actor passes.
//!
//! The kernel is [`legaia_engine_battle_vm::battle_actor_draw`] (the port of
//! `FUN_80048A08`); this block supplies what retail reads through its
//! globals: the seat (`+0x5A`), the seated actor's status word (`+0x16E`),
//! the present-party roster id (`0x8007BD10[seat]`) and the render node's
//! colour word (`+0x74`).

use super::*;
use legaia_engine_mode::mode::SceneMode;

use legaia_engine_battle_vm::battle_actor_draw::LimbDimPlan;
use legaia_engine_battle_vm::battle_actor_tick as tick;
use legaia_engine_battle_vm::battle_actor_tint as tint;
use legaia_engine_camera::projection::BattleCamPose;

/// The body radius a party seat carries at `*(actor[+0x22C]) + 0x58`:
/// `640` on 212 of the 213 party seats read across the catalogued battle
/// states (the tint pass's depth-cue half-range is `radius / 2`). A monster
/// seat's is its record size class `<< 5`.
pub const PARTY_BODY_RADIUS: i16 = 640;

/// The pool slot retail seats a summon creature in (`actor_table[7]`, the
/// cast module's creature seat - `docs/subsystems/cast-module.md`).
pub const SUMMON_CREATURE_SEAT: i16 = 7;

/// How far above the floor (retail units, `-Y`) the port lays the battle
/// ground shadow, so a host's depth test draws it over the stage floor it is
/// coplanar with. Retail has no depth buffer to tie; clear of the other
/// coplanar lifts (the field drop shadow's six units).
pub const BATTLE_SHADOW_DEPTH_LIFT: f32 = 2.5;

/// Where a battle body's ground shadow sits off its node position: the
/// drawn pose's centroid, turned onto the ground.
///
/// Retail does not centre the disc on the node. `FUN_80048A08` loads the
/// actor's view rotation and, as translation, the node position `+0x14`
/// with its height `+0x16` zeroed (`0x80049044..0x800490AC`), then runs
/// the GTE `MVMVA` (`0x80049104`) on the vector `(pool[0x120], 0,
/// pool[0x124])` (`0x800490B0..0x800490F8`), where `pool = gp[0xA0C] +
/// 0x6F4` is the pose pool the decoder `FUN_8004998C` just filled and its
/// `+0x120 / +0x124` the pose's `(x, z)` centroid
/// (`0x8004A3DC..0x8004A42C`). The result is the shadow's translation:
/// the point under the body pair, recomputed from this draw's pose. A clip
/// that carries its body off the node (Vahn knocked back by Tail Fire on
/// `battle_gimard_tail_fire_a`) leaves its shadow under the body, not
/// under the node. The rotation is the facing turn the body-pair store
/// makes ([`super::locomotion::body_offset`]); the port's battle bodies
/// carry no pitch, so the centroid's ground projection has no height.
// PORT: FUN_80048A08 (`0x80049034..0x8004914C`, the shadow's translation)
fn battle_shadow_offset(actor: &crate::world::Actor) -> (i16, i16) {
    super::locomotion::stream_pose(actor)
        .map(super::locomotion::pose_centroid_xz)
        .map(|(cx, cz)| super::locomotion::body_offset(actor.battle.facing_angle, cx, cz, 0x1000))
        .unwrap_or((0, 0))
}

/// One battle body's draw decision for this frame - the retail tint pass
/// (`FUN_8004A908`) and the draw tick that gates it (`FUN_800480D8`), run
/// over live world state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BattleActorDrawPlan {
    /// Whether retail draws the body this frame: the render dispatcher's
    /// near-plane gate (view depth `>= 0xA1`) and the draw tick's colour /
    /// grey-gate verdict.
    pub drawn: bool,
    /// The tint pass's writes (`+0x74` / `+0x78`).
    pub tint: tint::BattleTint,
    /// The colour word the draw sees - the tint's, or the grey stamp.
    pub draw_colour: u32,
    /// The view depth the tint was computed at.
    pub view_z: i32,
    /// The body radius (render node `+0x58`) the tint and the ground shadow
    /// read - [`PARTY_BODY_RADIUS`] for a party seat, the record size class
    /// `<< 5` for a monster.
    pub radius: i16,
    /// The body's retail pool slot `+0x5A`: the party seat, `3 + k` for
    /// monster `k`, `7` for the summon creature.
    pub seat: i16,
    /// The shatter the body draws under this frame - its `+0x1BA` word over
    /// the deformation row its node's `+0x6D` names - or `None` while the
    /// word is zero and the body takes the plain dispatcher
    /// ([`World::battle_actor_shatter`]).
    pub shatter: Option<legaia_render_kernels::shatter::ShatterInput>,
}

impl BattleActorDrawPlan {
    /// The GTE far colour as the hosts' `DrawCue.far` (display `0..1`).
    pub fn cue_far(&self) -> [f32; 3] {
        let c = self.draw_colour;
        [
            f32::from(c as u8) / 255.0,
            f32::from((c >> 8) as u8) / 255.0,
            f32::from((c >> 16) as u8) / 255.0,
        ]
    }

    /// The PSX blend mode the body draws with when its colour word's mode
    /// byte raises semi-transparency (bit 31; bits 24/25 the rule) - `3`
    /// (`B + F/4`) for a body the near-camera ghost pass `FUN_8004DC68`
    /// flagged. `None` for an opaque body.
    pub fn semi_mode(&self) -> Option<u8> {
        legaia_engine_battle::battle_body_blend::draw_colour_semi_mode(self.draw_colour)
    }

    /// Apply this body's whole-mesh semi-transparency
    /// ([`legaia_engine_battle::battle_body_blend::apply_body_blend`]) to a `[cba, tsb]`
    /// stream. Every stream the body draws takes it - a posed rebuild and
    /// the rest mesh alike - since retail's dispatcher ORs the word into
    /// every packet whatever pose the object is in. Both hosts call this:
    /// the native window on the per-frame posed mesh and on the rest mesh's
    /// blended override, the browser page on its blend re-upload.
    pub fn apply_body_blend(&self, cba_tsb: &mut [[u16; 2]]) {
        legaia_engine_battle::battle_body_blend::apply_body_blend(cba_tsb, self.semi_mode());
    }

    /// Whether the body takes the tint pass's cue (`cue_far` / `cue_ir0`)
    /// this frame, given its render flag. The target cursor's two flags
    /// carry their own cue (`battle_action::cursor_cue`); the capture /
    /// defeat fade (`render_flag == 2`) draws additive, so it takes the cue
    /// only while its word raises ABE - un-cued it is never an opaque black
    /// silhouette. The blend rides every stream the body draws
    /// ([`Self::apply_body_blend`]), so the answer does not depend on
    /// whether the body is posed.
    pub fn tint_cue_applies(&self, render_flag: u8) -> bool {
        use legaia_engine_battle_vm::battle_action as ba;
        !matches!(
            render_flag,
            ba::CURSOR_FLAG_SELECTED | ba::CURSOR_FLAG_DIMMED
        ) && (render_flag != 2 || self.semi_mode().is_some())
    }

    /// The GTE `IR0` as the hosts' `DrawCue.max_ir0` (`0x1000` = `1.0`).
    pub fn cue_ir0(&self) -> f32 {
        f32::from(self.tint.weight) / 4096.0
    }

    /// The constant per-draw cue a battle body takes this frame, as
    /// `(far colour, max IR0)`, `None` for no cue - the one precedence both
    /// hosts stage: the tint pass's word when it applies
    /// ([`Self::tint_cue_applies`]), else the target cursor's pulse
    /// (`battle_action::cursor_cue`) on the cursor's two flags. `plan` is the
    /// body's [`World::battle_actor_draw_plan`], `None` with no plan (the
    /// cursor still pulses). The native window used to stage the cursor and
    /// then overwrite it, the page to test the tint first and fall through:
    /// the same answer by two routes.
    pub fn body_cue(
        plan: Option<&Self>,
        render_flag: u8,
        display_frames: u64,
    ) -> Option<([f32; 3], f32)> {
        match plan {
            Some(p) if p.tint_cue_applies(render_flag) => Some((p.cue_far(), p.cue_ir0())),
            _ => legaia_engine_battle_vm::battle_action::cursor_cue(render_flag, display_frames),
        }
    }

    /// The body's NCLIP word for the hosts' per-draw backface rule
    /// (`Renderer::set_draw_nclip`, the page's placement `nclip`):
    /// [`BATTLE_BODY_NCLIP`] - back faces culled - unless the colour word
    /// carries the double-sided bit, which draws both sides (`0`).
    ///
    /// The prim dispatcher `FUN_80043390` stages the winding mask
    /// `0x1F80003C` as `0xFFFFFFFF` and lowers it to `0x7FFFFFFF` only for a
    /// word with bit `0x08000000` (`0x80043520..0x80043540`); every textured
    /// leaf then skips a primitive whose masked `NCLIP` is negative (the
    /// bank-2 triangle leaf at `0x8004586C`, the quad leaves' two-half test
    /// at `0x80045A38..0x80045A6C`). So a battle body is single-sided. Both
    /// sides drawn is invisible on an opaque body, whose depth test hides
    /// the far side, but a semi-transparent one adds both shells: the
    /// near-camera ghost (`B + F/4`, `theeder_summon_mid_cast`'s Gilium)
    /// drew about twice retail's brightness.
    ///
    /// A shattering body takes neither: the shatter renderer `FUN_80029DD8`
    /// emits every triangle with no `NCLIP`, so both sides draw
    /// ([`legaia_render_kernels::shatter::SHATTER_NCLIP`]).
    pub fn nclip_mode(&self) -> u32 {
        if self.shatter.is_some() {
            legaia_render_kernels::shatter::SHATTER_NCLIP
        } else if self.draw_colour & DOUBLE_SIDED_BIT != 0 {
            0
        } else {
            BATTLE_BODY_NCLIP
        }
    }
}

/// The colour-word bit that makes `FUN_80043390` draw a body double-sided
/// (`lui s1,0x800` / `and` at `0x80043520..0x80043524`).
pub const DOUBLE_SIDED_BIT: u32 = 0x0800_0000;

/// The NCLIP word (`Renderer::set_backface_cull` encoding) that keeps a
/// battle body's retail front faces: both hosts draw the battle bodies under
/// the same reflection parity as the field pass, so it is the field's `2`
/// ([`legaia_engine_field::camera_frame::nclip_cull_mode`]).
pub const BATTLE_BODY_NCLIP: u32 = 2;

impl crate::world::BattleGhostDraw {
    /// The ghost's NCLIP word for the hosts' per-draw backface rule, from
    /// its draw word ([`legaia_engine_battle::battle_afterimage::ghost_draw_word`])
    /// by the live body's rule ([`BattleActorDrawPlan::nclip_mode`]):
    /// the walk's word never carries the double-sided bit, so a ghost keeps
    /// only its front faces. Drawn both-sided, an additive ghost adds its
    /// far shell over its near one and reads about twice retail's
    /// (`battle_melee_hit_spark`'s Vahn).
    pub fn nclip_mode(&self) -> u32 {
        let word = legaia_engine_battle::battle_afterimage::ghost_draw_word(self.color);
        if word & DOUBLE_SIDED_BIT != 0 {
            0
        } else {
            BATTLE_BODY_NCLIP
        }
    }
}

impl World {
    /// The Rot limb dimming battle actor `actor_idx` draws with this frame,
    /// for a mesh of `object_count` objects - `None` when nothing on it dims
    /// (not a party seat, no Rot limb bit set, or no table installed).
    ///
    /// Both hosts call this from their battle actor pass: the native window
    /// re-colours the per-frame posed mesh, the browser play page re-uploads
    /// the actor's packet-colour stream when [`LimbDimPlan::key`] changes.
    ///
    /// The colour word is the render node's `+0x74`, which the tint pass
    /// `FUN_8004A908` packs from the actor's `+0x04` lanes (`>> 2` each). The
    /// engine's resting `render_color` is `0` where retail's is the neutral
    /// `0x20080200`, so a zero word reads as neutral here. An active tint is
    /// the host's per-draw cue on the whole mesh; retail replaces it on a
    /// dimmed object instead, so a limb dimmed during a tint flash also takes
    /// the flash in the port.
    pub fn battle_limb_dim_plan(
        &self,
        actor_idx: usize,
        object_count: usize,
    ) -> Option<LimbDimPlan> {
        let table = self.tables.rot_limb_table.as_ref()?;
        if self.mode != SceneMode::Battle || actor_idx >= usize::from(self.party.party_count) {
            return None;
        }
        let actor = self.actors.get(actor_idx)?;
        if actor.battle_monster_id.is_some() {
            return None;
        }
        let roster_id = u8::try_from(self.party_roster_slot(actor_idx) + 1).ok()?;
        let status = self.battle.status_effects.display_flags(actor_idx as u8);
        let word = match actor.battle.render_color {
            0 => legaia_engine_battle_vm::battle_impact_fx::IMPACT_NEUTRAL_STATE,
            w => w,
        };
        let [r, g, b] = legaia_engine_battle_vm::battle_impact_fx::unpack_actor_state_rgb(word);
        let colour = u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16);
        LimbDimPlan::resolve(
            colour,
            actor_idx as i16,
            status,
            table.row(roster_id),
            object_count,
        )
    }

    /// This frame's draw decision for battle body `actor_idx` - `None` for an
    /// actor that is not a battle combatant (party seat or seated monster).
    ///
    /// `pose` is the battle camera the host projects with (`None` for the
    /// monster-framing fallback outside a stage-dome battle, where the body is
    /// judged at retail's parked depth instead); `outdoor` is the stage's
    /// `DAT_80078C1C` outdoor-table membership. Both hosts call this per body
    /// per frame: the native window's battle actor pass and the browser play
    /// page's `play_battle_actor_transforms` (draw gate) and
    /// `play_battle_actor_cursor` (cue).
    pub fn battle_actor_draw_plan(
        &self,
        actor_idx: usize,
        pose: Option<&BattleCamPose>,
        world_scale: f32,
        outdoor: bool,
    ) -> Option<BattleActorDrawPlan> {
        if self.mode != SceneMode::Battle {
            return None;
        }
        let actor = self.actors.get(actor_idx)?;
        let party_count = usize::from(self.party.party_count);
        let (seat, radius, formation_cell) = match actor.battle_monster_id {
            Some(id) => {
                let slot = actor_idx.checked_sub(party_count)?;
                let size = self
                    .tables
                    .monster_catalog
                    .get(id)
                    .map_or(0, |def| def.size_class);
                (3 + slot as i16, i16::from(size) << 5, id as u8)
            }
            None if actor_idx < party_count => (actor_idx as i16, PARTY_BODY_RADIUS, 0),
            // The summon creature: retail seats it in pool slot 7, one of the
            // eight bodies the tint pass and the presentation SM walk, so its
            // `+0x21C` arm reaches the pixel like any combatant's - PROT
            // 0903's arm 9 raises it to `3` and the creature breathes red.
            // The engine seats the creature in a free high slot instead, and
            // this arm hands that slot retail's seat.
            None if self.casting.summon_actor_slot == Some(actor_idx as u8) => {
                let creature = self
                    .casting
                    .summon_stager
                    .as_ref()
                    .and_then(|st| self.summon_creature_def(st.spell_id));
                (
                    SUMMON_CREATURE_SEAT,
                    creature.map_or(PARTY_BODY_RADIUS, |d| i16::from(d.size_class) << 5),
                    creature.map_or(0, |d| d.id as u8),
                )
            }
            None => return None,
        };
        let b = &actor.battle;
        let view_z = match pose {
            Some(p) => legaia_engine_camera::projection::battle_view_depth(
                p,
                world_scale,
                [
                    f32::from(actor.move_state.world_x),
                    f32::from(actor.move_state.world_y),
                    f32::from(actor.move_state.world_z),
                ],
            ) as i32,
            None => tick::PARKED_VIEW_DEPTH,
        };
        // The engine's resting colour word is `0` where retail's is the
        // neutral `0x20080200`; a genuine zero only exists under a
        // presentation arm (the capture / defeat fade ramps the lanes down
        // with `+0x21C` raised).
        let lanes = match (b.render_color, b.render_flag) {
            (0, 0) => legaia_engine_battle_vm::battle_impact_fx::IMPACT_NEUTRAL_STATE,
            (w, _) => w,
        };
        let t = tint::battle_actor_tint(&tint::BattleTintInputs {
            lanes,
            // `+0x8`'s top byte: the near-camera ghost pass's mode bits.
            top: b.flag_word & 0xFF00_0000,
            blend: b.render_blend,
            render_flag: b.render_flag,
            // The whole `+0x16E` word the pass reads (`0x8004AC8C`): the
            // typed ailments and the raw bits only the raw side carries -
            // the Rage arm's `0x380` delegation group among them.
            status: self.raw_status_word(actor_idx as u8),
            fade: 0,
            radius,
            view_z,
            seat,
            formation_cell,
            outdoor,
            ctx_243: self.battle_ctx.gauge_rearm_latch != 0,
            prev_colour: 0,
            prev_weight: 0,
        });
        let verdict = tick::battle_actor_tick(
            &tick::BattleActorView {
                flags: 0,
                colour: t.colour,
                seat,
            },
            &tick::BattleTickGates {
                no_escape: self.battle.no_escape,
                second_monster: self.battle_monster_slots().len() > 1,
                seat_state: b.render_flag,
            },
        );
        let near_ok = pose.is_none() || view_z >= tick::NEAR_REJECT_DEPTH;
        Some(BattleActorDrawPlan {
            drawn: near_ok && verdict.drawn(),
            tint: t,
            draw_colour: verdict.colour,
            view_z,
            radius,
            seat,
            shatter: self.battle_actor_shatter(actor_idx),
        })
    }

    /// Battle body `actor_idx`'s shatter this frame: its `+0x1BA` word,
    /// which `FUN_80047430` copies into the render node's `+0x7A`
    /// (`0x800480A4`), over the deformation-table row the node's `+0x6D`
    /// names. `FUN_80048A08` routes every object of a body whose `+0x7A` is
    /// non-zero to the shatter renderer `FUN_80029888` with
    /// `+0x7A | row[+8] << 16` (`0x80048EE0..0x80048FC0`). `None` while the
    /// word is zero. A battle node's `+0x6D` reads `0xFF` in every capture,
    /// whose row reads zero, so a battle shatter takes the default spin and
    /// push; a row the Jette hook stages comes out of the field's table
    /// ([`World::shatter_row`]), global as retail's is.
    // REF: FUN_80048A08, FUN_80047430
    pub fn battle_actor_shatter(
        &self,
        actor_idx: usize,
    ) -> Option<legaia_render_kernels::shatter::ShatterInput> {
        let b = &self.actors.get(actor_idx)?.battle;
        let row = b
            .shatter_row
            .map_or_else(Default::default, |id| self.shatter_row(id));
        legaia_render_kernels::shatter::ShatterInput::new(b.shatter as i16, row)
    }

    /// The body-to-view map battle body `actor_idx`'s object matrices carry
    /// above each object's pose - the battle world scale (the base matrix
    /// `0x8007BF10 = 16384 * I`), the camera rotation `Rx(pitch) * Ry(yaw)`
    /// and the body's facing `+0x46` - which the shatter renderer's sweep
    /// arms measure a triangle's centroid through (`0x8002A000`), less the
    /// node's view translation. Hand it to
    /// [`legaia_render_kernels::shatter::bone_sweep_frame`]. Without a
    /// camera pose the body is measured unturned by the camera.
    // REF: FUN_80029DD8, FUN_80048A08
    pub fn battle_shatter_view(
        &self,
        actor_idx: usize,
        pose: Option<&BattleCamPose>,
        world_scale: f32,
    ) -> [[f32; 3]; 3] {
        let to_rad = |units: f32| units / 4096.0 * std::f32::consts::TAU;
        // `Ry(b)` in the camera kernel's convention: `+Z` turns onto
        // `(sin b, 0, cos b)`, the same turn a body's facing makes.
        let ry = |b: f32| {
            let (s, c) = b.sin_cos();
            [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]]
        };
        let mul = |a: [[f32; 3]; 3], b: [[f32; 3]; 3]| -> [[f32; 3]; 3] {
            std::array::from_fn(|r| {
                std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum())
            })
        };
        let facing = self
            .actors
            .get(actor_idx)
            .map_or(0.0, |a| f32::from(a.battle.facing_angle & 0xFFF));
        let mut m = ry(to_rad(facing));
        if let Some(p) = pose {
            let (sa, ca) = to_rad(p.pitch).sin_cos();
            let rx = [[1.0, 0.0, 0.0], [0.0, ca, -sa], [0.0, sa, ca]];
            m = mul(mul(rx, ry(to_rad(p.yaw))), m);
        }
        m.map(|row| row.map(|x| x * world_scale))
    }

    /// Whether battle body `actor_idx` is off screen under a hide: its
    /// `+0x21C` raised to `0xFF` **and** its colour word `+0x04` zero.
    /// Retail's hide is the colour word - the summon band writes
    /// `sw zero,0x4` beside `sb 0xFF,0x21C` (`0x801E4B30..0x801E4B6C`), and
    /// a zero word sends the tint pass `FUN_8004A908` down its no-lanes arm,
    /// whose zero colour the draw tick `FUN_800480D8` skips. The `0xFF` byte
    /// alone hides nothing: PROT 0912's arm 7 raises it with `+0x04 =
    /// 0x3FFFFFFF` on the creature whose shards re-form, and its arm `0x10`
    /// with `+0x04 = 0x3FF00000` on the monsters frozen inside the ice, and
    /// retail draws both. The engine's zero word reads as neutral only while
    /// the flag is down, so a zero word under a raised flag is retail's zero.
    // REF: FUN_8004A908, FUN_800480D8
    pub fn battle_body_hidden(&self, actor_idx: usize) -> bool {
        self.actors.get(actor_idx).is_some_and(|a| {
            a.battle.render_flag == legaia_engine_battle_vm::battle_target_group::RENDER_FLAG_HIDDEN
                && a.battle.render_color == 0
        })
    }

    /// Battle body `actor_idx`'s **ground shadow** this frame, as a mesh
    /// composed like an effect part - `None` when the body is not drawn
    /// (`plan.drawn`), or the shadow gate or its pitch / roll test drops it.
    ///
    /// Retail draws it at the end of the per-actor draw `FUN_80048A08`
    /// (`0x80049034..0x800492BC`): the point under the actor (its height
    /// `+0x16` zeroed), a 24-column disc of radius `+0x58 * 4 / 10` built by
    /// `FUN_80028158` in mode `1` ([`legaia_engine_effects::effect_default_arm::ground_shadow_mesh`]),
    /// subtracted from the frame (flag word `0x8A000000`). The colours are
    /// `legaia_engine_battle_vm::battle_actor_draw::shadow_plan` over the draw
    /// colour word. The skip flag `+0x6A` is not modelled (no engine actor
    /// carries it), so every drawn body casts one.
    ///
    /// The disc sits [`BATTLE_SHADOW_DEPTH_LIFT`] above the floor so both
    /// hosts' depth tests draw it over the stage floor it lies on.
    pub fn battle_ground_shadow(
        &self,
        actor_idx: usize,
        plan: &BattleActorDrawPlan,
    ) -> Option<legaia_engine_effects::effect_ribbon::RibbonDraw> {
        if self.mode != SceneMode::Battle || !plan.drawn {
            return None;
        }
        let actor = self.actors.get(actor_idx)?;
        let m = &actor.move_state;
        // The main draw's `+0x6A`: the retail pool slot is the body's
        // `+0x5A` (the plan's seat, which hands the summon creature retail's
        // slot `7`), and the after-image ghosts (`battle_afterimage`) cast
        // none.
        let skip = legaia_engine_battle_vm::battle_actor_draw::body_shadow_skip(plan.seat, false);
        let shadow = legaia_engine_battle_vm::battle_actor_draw::shadow_plan(
            skip,
            plan.draw_colour,
            m.world_y,
            plan.radius,
            m.render_24,
            m.render_28,
        )?;
        if !shadow.drawn {
            return None;
        }
        let mesh = legaia_engine_effects::effect_default_arm::ground_shadow_mesh(
            shadow.inner,
            shadow.outer,
            shadow.radius,
        );
        let (dx, dz) = battle_shadow_offset(actor);
        Some(legaia_engine_effects::effect_ribbon::RibbonDraw {
            mesh,
            world_pos: [
                f32::from(m.world_x.wrapping_add(dx)),
                -BATTLE_SHADOW_DEPTH_LIFT,
                f32::from(m.world_z.wrapping_add(dz)),
            ],
            rot: [0.0; 3],
            // A battle body's ground shadow is not a move-VM node; it draws
            // under the full camera (`FUN_80048A08`).
            flags_52: 0,
        })
    }

    /// The clip a battle body draws under while its node's `+0x42` is raised
    /// ([`legaia_engine_battle_vm::battle_action::BattleActor::floor_clip`]),
    /// in the body's **mesh** space - the posed mesh both hosts upload, in
    /// retail's actor-local units. `None` for a body that draws unclipped.
    ///
    /// Retail's per-actor draw `FUN_80048A08` tests the node's `+0x42` per
    /// object (`lh v0,0x42(s0)` at `0x80048E84`): raised, it runs
    /// `FUN_8001C204` over the object's posed transform and hands the object
    /// to the clipping renderer `FUN_8002735C` (`0x80048E94..0x80048FE4`)
    /// instead of the prim dispatcher. The clip is object-effect row
    /// `+0x42 - 1`, which battle setup re-seeds as angles `0`,
    /// `lo = -0x7FFF`, `hi = 0`
    /// ([`legaia_engine_effects::object_effect::BATTLE_ROW0`]): the half
    /// space at and above the stage floor. The one battle writer is the
    /// defeat fade's sink (`World::tick_battle_defeat_sink`), so a killed
    /// monster goes down through the floor and is gone once its mesh is
    /// under it - `player_steal_skeleton_banner` holds its skeleton `183`
    /// units down with no packet of it left on the ordering table.
    ///
    /// The effect point is `s * Rrow * (Ractor * v + pos)` over the posed
    /// vertex `v`, the node position `pos` (the body's live trio) and the
    /// render scale `s = +0x72`; the actor rotation is the facing yaw, which
    /// the floor row's vertical normal never reads.
    // REF: FUN_80048A08, FUN_8001C204, FUN_80027F00
    pub fn battle_actor_floor_clip(
        &self,
        actor_idx: usize,
    ) -> Option<legaia_engine_effects::object_effect::MeshClip> {
        if self.mode != SceneMode::Battle {
            return None;
        }
        let actor = self.actors.get(actor_idx)?;
        if !actor.battle.floor_clip {
            return None;
        }
        // The summon creature's node may name another row (PROT 0927 raises
        // `+0x42 = 2`); every other clipped body is on the floor row.
        let kind = match self.casting.module_creature_clip_kind {
            k if k > 1 && self.casting.summon_actor_slot == Some(actor_idx as u8) => k,
            _ => 1,
        };
        let clip = self
            .object_effect
            .clip_for(kind, &self.sin_lut, &self.cos_lut)?;
        let m = &actor.move_state;
        let yaw = f32::from(actor.battle.facing_angle & 0xFFF) / 4096.0 * std::f32::consts::TAU;
        let (s, c) = yaw.sin_cos();
        let rows = [
            [c, 0.0, s, f32::from(m.world_x)],
            [0.0, 1.0, 0.0, f32::from(m.world_y)],
            [-s, 0.0, c, f32::from(m.world_z)],
        ];
        let scale = match m.field_72 {
            0 => 1.0,
            v => f32::from(v) / 4096.0,
        };
        Some(clip.in_mesh_space(rows, scale))
    }

    /// Every battle body's [`Self::battle_ground_shadow`], each judged by the
    /// host's own draw plan (`plan_of`, the same call its actor pass makes).
    /// Both hosts' battle FX passes draw this list after the draw-kind-4
    /// list.
    pub fn battle_ground_shadows(
        &self,
        plan_of: impl Fn(usize) -> Option<BattleActorDrawPlan>,
    ) -> Vec<legaia_engine_effects::effect_ribbon::RibbonDraw> {
        if self.mode != SceneMode::Battle {
            return Vec::new();
        }
        (0..self.actors.len())
            .filter(|&i| self.actors[i].active)
            .filter_map(|i| self.battle_ground_shadow(i, &plan_of(i)?))
            .collect()
    }

    /// Apply [`Self::battle_limb_dim_plan`] to a per-frame posed battle mesh's
    /// packet colours - the native window's seat. `colors` must be the
    /// stream `legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot` built from the
    /// same `tmd` / `raw` (it walks the textured prims in the order
    /// `tmd_to_vram_mesh_with_object_ids` does, which supplies the object of
    /// each vertex). Returns whether anything was dimmed.
    pub fn dim_posed_battle_mesh(
        &self,
        actor_idx: usize,
        tmd: &legaia_tmd::Tmd,
        raw: &[u8],
        colors: &mut [[u8; 3]],
    ) -> bool {
        let Some(plan) = self.battle_limb_dim_plan(actor_idx, tmd.objects.len()) else {
            return false;
        };
        let mut ids = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(tmd, raw).1;
        if ids.len() != colors.len() {
            return false;
        }
        // The rule tests the draw pass's channel counter, so an object
        // drawn in another object's channel (an equipment variant) dims
        // with that channel.
        if let Some(ch) = self.battle_live_object_channels(actor_idx) {
            for id in &mut ids {
                *id = ch.get(*id as usize).copied().unwrap_or(*id);
            }
        }
        plan.apply_rgb(colors, &ids);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_battle_vm::battle_actor_draw::RotLimbTable;
    use legaia_engine_battle_vm::status_effects::StatusKind;
    use legaia_engine_mode::mode::SceneMode;

    fn world_with_table() -> World {
        let mut world = World::default();
        world.enter_battle(3, 2);
        let mut rows = [[0u8; 5]; 4];
        rows[0] = [2, 4, 5, 7, 10];
        rows[1] = [1, 1, 2, 2, 3];
        world.tables.rot_limb_table = Some(RotLimbTable { rows });
        world
    }

    #[test]
    fn a_rotted_party_limb_dims_its_own_object_range() {
        let mut world = world_with_table();
        assert!(world.battle_limb_dim_plan(0, 12).is_none());
        world.battle.status_effects.apply(0, StatusKind::Rot);
        world.battle.status_effects.set_rot_limb(0, 1);
        let plan = world.battle_limb_dim_plan(0, 12).expect("rotted limb");
        let dimmed: Vec<usize> = (0..12).filter(|&o| plan.is_dimmed(o as u32)).collect();
        assert_eq!(dimmed, vec![5, 6, 7]);
        // Neutral node colour 0x808080 -> far (0x20, 0x20, 0x40).
        assert_eq!(plan.far, [0x20, 0x20, 0x40]);
    }

    #[test]
    fn the_row_follows_the_roster_id_not_the_seat() {
        let mut world = world_with_table();
        world.set_active_party(vec![1, 0, 2]);
        world.battle.status_effects.apply(0, StatusKind::Rot);
        world.battle.status_effects.set_rot_limb(0, 0);
        let plan = world.battle_limb_dim_plan(0, 12).expect("rotted limb");
        // Seat 0 holds roster slot 1 (id 2): row [1, 1, ...].
        assert!(plan.is_dimmed(1) && !plan.is_dimmed(2));
    }

    fn battle_world() -> World {
        let mut world = World::default();
        world.enter_battle(3, 1);
        world.actors[3].battle_monster_id = Some(4);
        // Past the entry sweep's stage load: the ramp tests below start
        // from the loader's floor.
        world.battle.stage_registration_pending = false;
        world
    }

    fn pose(tr_z: f32) -> BattleCamPose {
        BattleCamPose {
            pitch: 0.0,
            yaw: 0.0,
            tr: [0.0, 0.0, tr_z],
            focus: [0.0; 3],
        }
    }

    /// A battle body is single-sided - the ghosted near body too - unless
    /// its colour word carries `FUN_80043390`'s double-sided bit.
    #[test]
    fn a_battle_body_culls_back_faces_unless_its_word_is_double_sided() {
        let mut world = battle_world();
        world.actors[0].move_state.world_z = 0;
        let plan = world
            .battle_actor_draw_plan(0, Some(&pose(4000.0)), 4.0, false)
            .expect("party seat");
        assert_eq!(plan.nclip_mode(), BATTLE_BODY_NCLIP);
        let ghost = BattleActorDrawPlan {
            draw_colour: 0x87FF_2020,
            ..plan
        };
        assert_eq!(ghost.semi_mode(), Some(3));
        assert_eq!(ghost.nclip_mode(), BATTLE_BODY_NCLIP);
        let both = BattleActorDrawPlan {
            draw_colour: plan.draw_colour | DOUBLE_SIDED_BIT,
            ..plan
        };
        assert_eq!(both.nclip_mode(), 0);
        // A shattering body draws both sides whatever its word.
        world.actors[0].battle.shatter = 0x100;
        let shattering = world
            .battle_actor_draw_plan(0, Some(&pose(4000.0)), 4.0, false)
            .expect("party seat");
        let s = shattering
            .shatter
            .expect("the +0x1BA word reaches the plan");
        assert_eq!(s.word, 0x100, "battle rows read zero: default spin + push");
        assert_eq!(shattering.nclip_mode(), 0);
    }

    /// The hide is the zero colour word under the raised `0xFF`, not the
    /// byte alone: Freed's re-forming creature and frozen monsters keep a
    /// lit word and draw.
    #[test]
    fn a_raised_hide_byte_with_a_lit_word_still_draws() {
        let mut world = battle_world();
        world.actors[0].battle.render_flag = 0xFF;
        world.actors[0].battle.render_color = 0;
        assert!(world.battle_body_hidden(0));
        world.actors[0].battle.render_color = 0x3FF0_0000;
        assert!(!world.battle_body_hidden(0));
        world.actors[0].battle.render_flag = 0;
        world.actors[0].battle.render_color = 0;
        assert!(!world.battle_body_hidden(0), "a zero word reads neutral");
    }

    /// A drawn party body casts the 24-column subtractive disc of radius
    /// `640 * 4 / 10`, under it on the floor; a pitched body casts none.
    #[test]
    fn a_drawn_party_body_casts_the_ground_shadow_disc() {
        let mut world = battle_world();
        world.actors[0].move_state.world_z = 0;
        world.actors[0].move_state.world_x = 300;
        let plan = world
            .battle_actor_draw_plan(0, Some(&pose(4000.0)), 4.0, false)
            .expect("party seat");
        assert_eq!(plan.radius, PARTY_BODY_RADIUS);
        let d = world.battle_ground_shadow(0, &plan).expect("a shadow");
        assert_eq!(d.world_pos, [300.0, -BATTLE_SHADOW_DEPTH_LIFT, 0.0]);
        assert_eq!(d.mesh.indices.len(), 24 * 6);
        let r = d
            .mesh
            .positions
            .iter()
            .map(|p| p[0].hypot(p[2]))
            .fold(0.0f32, f32::max);
        assert!((r - 256.0).abs() < 1.5, "radius {r}");
        let all = world.battle_ground_shadows(|i| {
            world.battle_actor_draw_plan(i, Some(&pose(4000.0)), 4.0, false)
        });
        assert!(!all.is_empty());
        world.actors[0].move_state.render_24 = 0x100;
        assert!(world.battle_ground_shadow(0, &plan).is_none());
    }

    /// The disc sits under the drawn pose's centroid, turned by the facing,
    /// not under the node: a clip whose parts stand 400 units forward of the
    /// node moves the shadow with them (`FUN_80048A08`'s `MVMVA` of the pose
    /// pool's `+0x120 / +0x124`).
    #[test]
    fn the_ground_shadow_follows_the_pose_centroid() {
        use legaia_actor_formats::monster_archive::{MonsterAnimation, PartPose};
        let mut world = battle_world();
        world.actors[0].move_state.world_x = 100;
        world.actors[0].move_state.world_z = -700;
        let plan = world
            .battle_actor_draw_plan(0, Some(&pose(4000.0)), 4.0, false)
            .expect("party seat");
        let anim = MonsterAnimation {
            variant_track: Default::default(),
            action_id: 0,
            rate: 1,
            attach_key: 0,
            solo_flag: 0,
            impact_class: 0,
            effect_script: Vec::new(),
            part_count: 2,
            frame_count: 1,
            frames: vec![vec![
                PartPose {
                    tz: 400,
                    ..PartPose::default()
                };
                2
            ]],
        };
        let a = &mut world.actors[0];
        a.battle_animation = legaia_engine_battle::battle_anim::MonsterAnimPlayer::new(&anim);
        a.pose_frame = Some(a.battle_animation.as_mut().unwrap().seat_cursor(0));
        for (facing, want) in [(0u16, [100.0, -300.0]), (0x800, [100.0, -1100.0])] {
            world.actors[0].battle.facing_angle = facing;
            let d = world.battle_ground_shadow(0, &plan).expect("a shadow");
            assert!(
                (d.world_pos[0] - want[0]).abs() <= 1.0 && (d.world_pos[2] - want[1]).abs() <= 1.0,
                "facing {facing:#x}: {:?}",
                d.world_pos
            );
        }
    }

    /// The seated summon creature is a tinted body on retail's seat 7: PROT
    /// 0903's arm-9 red (`+0x21C = 3`, `+0x04 = 0x3FF`) reaches its draw
    /// colour, where a plan-less creature drew its disc palette (grey) in
    /// `gimard_burning_attack`.
    #[test]
    fn the_summon_creature_takes_its_tint_on_seat_seven() {
        let mut world = battle_world();
        let slot = 9;
        assert!(
            world
                .battle_actor_draw_plan(slot, None, 4.0, false)
                .is_none()
        );
        world.actors[slot].active = true;
        world.seat_summon_actor(slot);
        world.actors[slot].battle.render_flag = 3;
        world.actors[slot].battle.render_color = 0x3FF;
        world.actors[slot].battle.render_blend = 0x1000;
        let plan = world
            .battle_actor_draw_plan(slot, None, 4.0, false)
            .expect("the creature is a battle body");
        assert!(plan.drawn);
        let [r, g, b] = [
            plan.draw_colour as u8,
            (plan.draw_colour >> 8) as u8,
            (plan.draw_colour >> 16) as u8,
        ];
        assert!(r > 0xF0 && g == 0 && b == 0, "{r:#x} {g:#x} {b:#x}");
    }

    /// The grid's ambient fades in from the battle-init floor, dims under a
    /// summon close-up's `ctx[+0x243]`, and climbs back once it clears.
    #[test]
    fn the_battle_ambient_fades_in_and_dims_under_the_close_up_latch() {
        let mut world = battle_world();
        assert_eq!(world.battle_ambient_base(), [0x20; 3]);
        for _ in 0..48 {
            world.tick_battle_ambient();
        }
        assert_eq!(world.battle_ambient_base(), [0x80; 3]);
        world.battle_ctx.gauge_rearm_latch = 1;
        world.tick_battle_ambient();
        assert_eq!(world.battle_ambient_base(), [0x78; 3]);
        for _ in 0..20 {
            world.tick_battle_ambient();
        }
        assert_eq!(world.battle_ambient_base(), [0x20; 3]);
        world.battle_ctx.gauge_rearm_latch = 0;
        world.tick_battle_ambient();
        assert_eq!(world.battle_ambient_base(), [0x22; 3]);
    }

    /// The floor's fade-in starts on the stage load, four entry-counter
    /// units into the sweep, not at the mode flip: before it the base holds
    /// the floor; on the load pass the loader re-seeds the floor and the
    /// pass ramps it once.
    #[test]
    fn the_ambient_ramp_waits_for_the_stage_load() {
        let mut world = World::default();
        world.enter_battle(3, 1);
        // The flip tick: no camera yet, no stage.
        world.tick_battle_ambient();
        assert_eq!(world.battle_ambient_base(), [0x20; 3]);
        let mut loaded_at = None;
        for _ in 0..64 {
            world.clock.display_frames += 2;
            world.tick_battle_camera();
            let counter = world
                .battle
                .camera
                .as_ref()
                .and_then(|c| c.entry_sweep_counter());
            world.tick_battle_ambient();
            match counter {
                Some(c) if c < 0x84 => assert_eq!(world.battle_ambient_base(), [0x20; 3]),
                _ => {
                    loaded_at.get_or_insert(counter);
                    break;
                }
            }
        }
        assert!(loaded_at.is_some(), "the sweep reaches the stage load");
        // The load pass re-seeds the floor and ramps it once.
        assert_eq!(world.battle_ambient_base(), [0x22; 3]);
        world.tick_battle_ambient();
        assert_eq!(world.battle_ambient_base(), [0x24; 3]);
    }

    /// Through `0x33` / `0x34` the band's `ctx[+0x278] = 1` skips the store
    /// once the base sits on the floor, so the grid holds the last pre-floor
    /// ambient - what the summon captures read (`0x68..0x78`, base at the
    /// floor) - and resumes storing when `0x35` clears the byte.
    #[test]
    fn the_band_latch_freezes_the_ambient_above_the_floor() {
        let mut world = battle_world();
        for _ in 0..48 {
            world.tick_battle_ambient();
        }
        world.battle_ctx.gauge_rearm_latch = 1;
        world.battle_ctx.summon_staging_a = 1;
        for _ in 0..30 {
            world.tick_battle_ambient();
        }
        assert_eq!(
            legaia_engine_battle_vm::battle_ground_grid::ambient_base_rgb(
                world.battle_ctx.ambient_base
            ),
            [0x20; 3]
        );
        assert_eq!(world.battle_ambient_base(), [0x28; 3]);
        world.battle_ctx.summon_staging_a = 0;
        world.tick_battle_ambient();
        assert_eq!(world.battle_ambient_base(), [0x20; 3]);
    }

    #[test]
    fn a_near_resting_party_body_draws_neutral() {
        let mut world = battle_world();
        world.actors[0].move_state.world_z = 0;
        // Party radius 640 -> half-range 320; view 4000 / 16 = 250 is near.
        let p = world
            .battle_actor_draw_plan(0, Some(&pose(4000.0)), 4.0, false)
            .expect("party seat");
        assert!(p.drawn);
        assert_eq!(p.view_z, 4000);
        assert_eq!(p.tint.arm, tint::TintArm::Plain);
        assert_eq!(p.draw_colour & 0xFF_FFFF, 0x80_8080);
        assert!((p.cue_ir0() - 1000.0 / 4096.0).abs() < 1e-6);
    }

    #[test]
    fn a_far_body_fades_darker_indoors_and_brighter_outdoors() {
        let mut world = battle_world();
        world.actors[0].move_state.world_z = 0;
        let indoor = world
            .battle_actor_draw_plan(0, Some(&pose(8117.0)), 4.0, false)
            .unwrap();
        assert_eq!(indoor.tint.arm, tint::TintArm::DepthCue);
        assert_eq!(indoor.draw_colour & 0xFF_FFFF, 0x50_5050);
        let outdoor = world
            .battle_actor_draw_plan(0, Some(&pose(8117.0)), 4.0, true)
            .unwrap();
        assert!(outdoor.draw_colour & 0xFF > 0x80);
    }

    #[test]
    fn a_body_behind_the_near_plane_is_not_drawn() {
        let world = battle_world();
        let p = world
            .battle_actor_draw_plan(0, Some(&pose(0x80 as f32)), 4.0, false)
            .unwrap();
        assert!(!p.drawn);
        // Without a camera the body is judged at the parked depth.
        assert!(
            world
                .battle_actor_draw_plan(0, None, 4.0, false)
                .unwrap()
                .drawn
        );
    }

    #[test]
    fn a_faded_out_monster_drops_unless_the_lone_monster_gate_greys_it() {
        let mut world = battle_world();
        world.actors[3].battle.render_color = 0;
        world.actors[3].battle.render_flag = 2;
        let p = world.battle_actor_draw_plan(3, None, 4.0, false).unwrap();
        assert!(!p.drawn, "zero lanes in an ordinary fight: gone");
        world.battle.no_escape = true;
        let p = world.battle_actor_draw_plan(3, None, 4.0, false).unwrap();
        assert!(p.drawn, "lone monster, scripted fight: stamped grey");
        assert_eq!(p.draw_colour, tick::DEFEATED_GREY);
    }

    #[test]
    fn the_fade_cue_follows_the_blend_word_not_the_pose() {
        let world = battle_world();
        let mut p = world.battle_actor_draw_plan(0, None, 4.0, false).unwrap();
        assert!(p.tint_cue_applies(0));
        assert!(!p.tint_cue_applies(5), "cursor flag keeps its own cue");
        assert!(!p.tint_cue_applies(200));
        p.draw_colour &= 0x7FFF_FFFF;
        assert!(!p.tint_cue_applies(2), "opaque fade: no cue");
        p.draw_colour |= 0x8100_0000;
        assert!(p.tint_cue_applies(2), "additive fade: cued");
        let mut ct = [[0u16, 0u16]; 3];
        p.apply_body_blend(&mut ct);
        assert!(
            ct.iter()
                .all(|c| c[1] & 0x8000 != 0 && (c[1] >> 5) & 3 == 1)
        );
    }

    #[test]
    fn non_combatants_have_no_plan() {
        let world = battle_world();
        assert!(world.battle_actor_draw_plan(5, None, 4.0, false).is_none());
        let mut field = World::default();
        field.enter_battle(3, 1);
        field.mode = SceneMode::Field;
        assert!(field.battle_actor_draw_plan(0, None, 4.0, false).is_none());
    }

    #[test]
    fn monsters_and_a_disc_free_world_never_dim() {
        let mut world = world_with_table();
        world.battle.status_effects.apply(3, StatusKind::Rot);
        assert!(world.battle_limb_dim_plan(3, 12).is_none());
        world.tables.rot_limb_table = None;
        world.battle.status_effects.apply(0, StatusKind::Rot);
        assert!(world.battle_limb_dim_plan(0, 12).is_none());
    }

    /// A body the defeat sink raised `+0x42` on keeps only what stands at
    /// or above the floor: over its posed mesh the clip is the mesh's own
    /// `Y` against the body's height `+0x36`, so a body `183` down (Y-down)
    /// keeps nothing above mesh `y = -183`.
    #[test]
    fn a_sunk_body_is_clipped_at_the_stage_floor() {
        let mut world = World::default();
        world.enter_battle(1, 1);
        assert!(world.battle_actor_floor_clip(1).is_none(), "gate down");
        world.actors[1].battle.floor_clip = true;
        world.actors[1].battle.facing_angle = 0x800;
        world.actors[1].move_state.world_x = 675;
        world.actors[1].move_state.world_y = 183;
        world.actors[1].move_state.world_z = 623;
        let c = world.battle_actor_floor_clip(1).expect("raised");
        assert!(
            c.keeps([30.0, -200.0, -40.0]),
            "a vertex still above the floor"
        );
        assert!(!c.keeps([30.0, -100.0, -40.0]), "a vertex sunk under it");
        assert!(!c.keeps([0.0, 0.0, 0.0]), "the body's origin is under it");
        world.mode = SceneMode::Field;
        assert!(world.battle_actor_floor_clip(1).is_none(), "battle only");
    }
}
