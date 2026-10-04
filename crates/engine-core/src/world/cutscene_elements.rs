//! The **element channel**: the pool of spawned field-overlay plain-template
//! actors whose handlers live in [`crate::cutscene_script_elements`]. Its one
//! occupant is the ambient particle emitter (`FUN_801D6058`).
//!
//! It follows the shape `World` already uses for the field overlay's other
//! plain-template actors ([`crate::world::FieldVmState::eased_moves`],
//! `floor_tier_bobs`): one `Vec` per family on the world, advanced together on
//! the frame tick, retired when the kernel says so.
//!
//! The channel used to carry two more element kinds - a "position tween"
//! (`FUN_801D5C08`) and a "teardown" (`FUN_801D5D60`) - that no producer ever
//! spawned. Those two handlers are the ledge-hop / op-`0x43` arc pair, ported
//! and seated in `legaia_engine_vm::field_ledge_hop_arc` and
//! `World::script_actors`; see [`crate::cutscene_script_elements`] for the
//! template evidence.
//!
//! # Where the ambient emitter actually comes from
//!
//! `crate::cutscene_script_elements`'s module note said there was "no
//! element-actor dispatch to hang these off". For the **ambient emitter** that
//! is falsified by the bytes: `0x801D6058` is the `+0x08` handler word of a
//! `0x18`-byte spawn descriptor at `0x801F271C` in the field overlay's own
//! plain-template table - the same table, and the same
//! `[u32 0][u16 0][u16 0xFFFF][u32 handler][u32 0][u32 0][u32 0]` shape, as the
//! floor-ladder oscillator (`0x801F27EC`), the eased move (`0x801F2840`) and
//! the shutter bars (`0x801F2858`) the engine already hosts.
//!
//! Its one spawn site is `0x801D6FD8` in the field overlay:
//! `FUN_80024C88(&pos, 0x801F271C, pool)` with `pos = [s7 + 0xA40, 0,
//! fp + 0xA40]` on the caller's stack, followed by `sh $s0, 0x1a($v0)` - i.e.
//! `+0x1A = 1` on the actor the spawn returned, which selects the emitter's
//! **scene** arm rather than its point arm. The whole site sits behind a
//! `bnez` on `_DAT_8007B8B8`, so it runs only while that global is clear.
//!
//! REF: FUN_80024C88 (the positioned spawn), FUN_801D6058 (the handler)

use crate::cutscene_script_elements::{AmbientEmitter, AmbientParticle, AmbientScene};
use crate::world::{SceneMode, World};

/// A borrow-free view of [`World::rng_state`], so the channel can feed the
/// ambient emitter the world's own deterministic LCG while the tick holds
/// `&mut self`.
///
/// The emitter consumes exactly as many draws as retail does on the arm it
/// takes, so threading the world's stream through it (rather than a private
/// one) is what keeps a replay reproducible.
pub struct WorldRng(u32);

impl WorldRng {
    /// Seed from [`World::rng_state`].
    pub fn new(state: u32) -> Self {
        WorldRng(state)
    }

    /// The same LCG step [`World::next_rng`] runs. Named `step` rather than
    /// `next` so it cannot be mistaken for an iterator.
    pub fn step(&mut self) -> u32 {
        self.0 = legaia_engine_vm::battle_formulas::world_lcg_step(self.0);
        self.0
    }

    /// The state to write back to the world.
    pub fn state(&self) -> u32 {
        self.0
    }
}

/// The BIOS `rand()` shaping, applied to the raw [`WorldRng`] state before
/// the element channel's consumers see a draw. One definition for the whole
/// engine: [`legaia_engine_vm::battle_formulas::bios_rand_shape`].
pub use legaia_engine_vm::battle_formulas::bios_rand_shape;

/// Runtime VA of the ambient emitter's spawn descriptor in the field overlay's
/// plain-template table (see the module note).
pub const AMBIENT_EMITTER_TEMPLATE_VA: u32 = 0x801F_271C;

/// The value the spawn site stores into the new actor's `+0x1A`
/// ([`AmbientEmitter::state`]) - the arm selector, `1` = the scene arm.
pub const AMBIENT_EMITTER_SCENE_ARM: i16 = 1;

/// Which handler an element runs.
#[derive(Debug, Clone)]
pub enum ElementKind {
    /// `FUN_801D6058` - the ambient particle emitter.
    AmbientEmitter {
        emitter: AmbientEmitter,
        scene: AmbientScene,
    },
}

/// One live element on the channel.
#[derive(Debug, Clone)]
pub struct CutsceneElement {
    /// The handler.
    pub kind: ElementKind,
    /// `+0x10` bit `8` - the element's own done bit. A done element is retired
    /// at the end of the frame that set it.
    pub done: bool,
}

/// What one frame of the channel produced, for a host to act on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ElementFrame {
    /// Particles the ambient emitters spawned this frame.
    pub particles: Vec<AmbientParticle>,
    /// How many elements retired this frame.
    pub retired: usize,
}

impl ElementFrame {
    /// `true` when the channel did nothing at all.
    pub fn is_empty(&self) -> bool {
        self.particles.is_empty() && self.retired == 0
    }
}

impl World {
    /// Spawn the ambient particle emitter on its **scene** arm - the one arm
    /// the retail spawn site at `0x801D6FD8` selects (`+0x1A = 1`).
    ///
    /// REF: FUN_801D6058, FUN_80024C88
    pub fn spawn_ambient_emitter(&mut self, scene: AmbientScene) {
        self.cutscene.elements.push(CutsceneElement {
            kind: ElementKind::AmbientEmitter {
                emitter: AmbientEmitter {
                    state: AMBIENT_EMITTER_SCENE_ARM,
                    actor_x: 0,
                    actor_y: 0,
                },
                scene,
            },
            done: false,
        });
    }

    /// Spawn the ambient emitter on its **point** arm at `(x, y)` - the actor
    /// position halves the handler reads out of `+0x14` / `+0x18`.
    ///
    /// REF: FUN_801D6058
    pub fn spawn_ambient_emitter_at(&mut self, scene: AmbientScene, x: i16, y: i16) {
        self.cutscene.elements.push(CutsceneElement {
            kind: ElementKind::AmbientEmitter {
                emitter: AmbientEmitter {
                    state: 0,
                    actor_x: x as u16,
                    actor_y: y as u16,
                },
                scene,
            },
            done: false,
        });
    }

    /// The **producer**: retail's one element spawn on a field entry.
    ///
    /// `FUN_801D6704` (field MAIN INIT) reaches `0x801D6FB8..0x801D6FE0` only
    /// when the field-entry mode word `_DAT_8007B8B8` is zero
    /// ([`crate::mode_entry_init::FieldEntryMode::Cold`], which is every
    /// ordinary scene change), and there it runs
    /// `FUN_80024C88(&pos, 0x801F271C, pool)` and stores `1` into the returned
    /// actor's `+0x1A` - the emitter's **scene** arm. `pos` is
    /// `(s7 + 0xA40, 0, fp + 0xA40)`, which the scene arm never reads: it
    /// samples points across the `DAT_1F8003E8..EB` span instead. So the
    /// spawn is one per field entry, positionless, and there is exactly one
    /// of it - which is what [`Self::install_field_scene_elements`] enforces
    /// by clearing any previous emitter first.
    ///
    /// The emitter is spawned with its master gate **clear**. That gate is
    /// `_DAT_8007B854`, and it has exactly six references disc-wide: two SCUS
    /// clears (`0x800259AC`, `0x8003B690`), one SCUS reader in the field
    /// render pass (`0x80026EBC`, gated on game mode `3`), the emitter's own
    /// first instruction (`0x801D605C`) and **two field-VM writers** -
    /// `0x801E0F38` sets it, `0x801E0F44` clears it, the sub-`0` and sub-`1`
    /// arms of the op-`0x4C` outer-nibble-`3` jump table at `0x801CEEB8`. So
    /// ambient particles are script-driven per scene, and an emitter spawned
    /// with the gate clear emits nothing until a script raises it - which is
    /// what [`Self::set_ambient_particles_enabled`] is for.
    ///
    /// PORT: FUN_801D6704
    ///
    /// WIRED: the field arm of `Scene::enter_field_scene`
    /// (`scene/host/scene_entry.rs`) calls this beside the
    /// `crate::mode_entry_init::field_spawn` that seats the player, so every
    /// cold field entry on both hosts installs exactly one emitter. What no
    /// host yet DRAWS is the particles themselves: the channel fills
    /// `ElementFrame::particles` every frame and no renderer consumes them,
    /// because retail's per-particle actor (`FUN_801D629C`) is a separate
    /// template family the port has no producer for.
    ///
    /// REF: FUN_80024C88 (the positioned spawn), FUN_801D6058 (the handler)
    pub fn install_field_scene_elements(
        &mut self,
        entry_mode: crate::mode_entry_init::FieldEntryMode,
        span: crate::cutscene_script_elements::SceneSpan,
    ) {
        if entry_mode != crate::mode_entry_init::FieldEntryMode::Cold {
            return;
        }
        self.cutscene
            .elements
            .retain(|el| !matches!(el.kind, ElementKind::AmbientEmitter { .. }));
        self.spawn_ambient_emitter(AmbientScene {
            // `_DAT_8007B854`: cleared by SCUS, raised only by the field VM.
            enabled: false,
            // `_DAT_1F800394 & 1` - the world-map render-policy bit, clear in
            // an ordinary field scene.
            dense: false,
            span,
            camera_x: 0,
            camera_y: 0,
        });
    }

    /// Raise or clear the ambient-particle master gate `_DAT_8007B854` on
    /// every live emitter.
    ///
    /// This is the sink for op `0x4C` outer nibble `3`, sub-ops `0` and `1`
    /// (`0x801E0F2C` / `0x801E0F3C` off the jump table at `0x801CEEB8`).
    /// Retail keeps the gate in one word and the emitter re-reads it every
    /// frame; the port keeps a copy per element, so the write fans out here.
    ///
    /// The VM side of this is `FieldHost::set_ambient_particle_gate`, and
    /// `FieldHostImpl` (`crate::world::vm_hosts`) forwards it straight here,
    /// so a script that raises the gate reaches every live emitter on both
    /// hosts. The global's only readers are that emitter and the field render
    /// pass's particle-table stage at `0x80026EBC`; neither touches pad state,
    /// which is why the hook is named for ambience rather than for input.
    pub fn set_ambient_particles_enabled(&mut self, on: bool) {
        for el in self.cutscene.elements.iter_mut() {
            let ElementKind::AmbientEmitter { scene, .. } = &mut el.kind;
            scene.enabled = on;
        }
        // The same word gates the render pass's pool walk (`0x80026EBC`), so
        // the pool keeps its own copy for `World::fog_render_step`.
        self.fog.gate = on;
    }

    /// Run every live element for one frame and return what they asked for.
    ///
    /// `_frame_step` is retail's `DAT_1F800393`. The emitter's scene arm does
    /// not scale by it (it runs its draws once per handler call), so it is
    /// unused today; it stays in the signature beside the other
    /// plain-template passes so a frame-scaled occupant needs no host edit.
    ///
    /// Retired elements are dropped at the end of the pass; an element spawned
    /// *during* it is spliced in rather than overwritten, the same way the
    /// eased-move pass handles a spawn from its own frame.
    pub fn tick_cutscene_elements(&mut self, _frame_step: u8, mut raw_rand: impl FnMut() -> u32) {
        // The emitter and the fog spawner call the BIOS `rand()` (`A(2Fh)`,
        // reached through `FUN_80056798`), which returns the **high** half of
        // its LCG state, `(seed >> 16) & 0x7FFF`. The world stream is a raw
        // 32-bit LCG whose low four bits cycle with period 16, and both
        // routines test exactly those bits (`rand & 0xF`, `rand & 7`,
        // `rand & 0x7F`): fed the raw state, the emitter's burst gate failed
        // on almost every frame and then passed on all 24 draws at once,
        // spawning fifty-odd fog records in one frame - the "pool density
        // above retail" the vell poll measured. Shaping the draw the way the
        // BIOS does restores retail's per-frame spawn statistics.
        let mut rand = move || bios_rand_shape(raw_rand());
        if self.cutscene.elements.is_empty() {
            if !self.cutscene.element_frame.is_empty() {
                self.cutscene.element_frame = ElementFrame::default();
            }
            return;
        }
        let mut frame = ElementFrame::default();
        let mut live = std::mem::take(&mut self.cutscene.elements);
        // The fog pool the ambient emitter spawns into (`FUN_801D629C`):
        // taken out for the pass so the spawn closure can hold it beside the
        // element list. Its inputs are the walk-region box the spawner
        // clips against (`0x1F800384..87`) and the player position the
        // emitter's scene arm centres its bursts on (`_DAT_80089118/20`
        // hold the negated player X/Z; the follow camera's focus).
        let mut fog = std::mem::take(&mut self.fog);
        // `_DAT_1F800394 & 1`, the overworld bit: the emitter's dense profile
        // and the spawner's overworld arm both key on it.
        let overworld = self.mode == SceneMode::WorldMap;
        fog.overworld = overworld;
        let window = self.terrain.region_attributes.box_bytes;
        let [player_x, _, player_z] = self.fog_player_world_pos();
        let trig = crate::action_effect_script::retail_rotation_lut();
        for el in live.iter_mut() {
            match &mut el.kind {
                // The emitter's handler (`FUN_801D6058`) is field-overlay code
                // (PROT 0897, slot A at `0x801CE818`), and battle loads PROT
                // 0898 over the same window: nothing runs it during a fight.
                // It is parked, not retired - the field it belongs to comes
                // back after the battle, and a return from battle is not the
                // cold entry that would spawn a fresh one. Stepping it here
                // drew the world `rand()` stream once a frame for the whole
                // fight whenever the scene's script had raised the gate.
                ElementKind::AmbientEmitter { .. } if self.mode == SceneMode::Battle => {}
                ElementKind::AmbientEmitter { emitter, scene } => {
                    scene.dense = overworld;
                    // The burst span is the live visible-tile window, read
                    // afresh every frame (`lb` of `0x1F8003E8..EB`): on the
                    // overworld a region record widens it to reach well
                    // ahead of the player, which is where retail's fog sits.
                    if let Some([x_min, y_min, x_max, y_max]) = fog.view_window {
                        scene.span = crate::cutscene_script_elements::SceneSpan {
                            x_min,
                            y_min,
                            x_max,
                            y_max,
                        };
                    }
                    scene.camera_x = -player_x;
                    scene.camera_y = -player_z;
                    emitter.step_with(scene, &mut rand, |p, rand| {
                        fog.spawn(p.x, p.y, window, trig, rand);
                        frame.particles.push(p);
                    });
                }
            }
        }
        self.fog = fog;
        let before = live.len();
        live.retain(|el| !el.done);
        frame.retired = before - live.len();
        live.append(&mut self.cutscene.elements);
        self.cutscene.elements = live;
        self.cutscene.element_frame = frame;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::SceneMode;

    fn world() -> World {
        let mut w = World::new();
        w.mode = SceneMode::Field;
        w.clock.display_frame_step = 1;
        w
    }

    #[test]
    fn a_cold_field_entry_installs_exactly_one_emitter_with_the_gate_clear() {
        use crate::cutscene_script_elements::SceneSpan;
        use crate::mode_entry_init::{FIELD_DEFAULT_VIEW_WINDOW, FieldEntryMode};
        let (x_min, y_min, x_max, y_max) = FIELD_DEFAULT_VIEW_WINDOW;
        let span = SceneSpan {
            x_min,
            y_min,
            x_max,
            y_max,
        };
        let mut w = world();
        // A warp entry (`_DAT_8007B8B8 == 2`) reaches no spawn at all.
        w.install_field_scene_elements(FieldEntryMode::Warp, span);
        assert!(w.cutscene.elements.is_empty());

        // A cold entry installs one, gate clear, so it emits nothing.
        w.install_field_scene_elements(FieldEntryMode::Cold, span);
        assert_eq!(w.cutscene.elements.len(), 1);
        let mut seed = 0x1234_5678u32;
        let mut rand = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            seed >> 8
        };
        let mut total = 0;
        for _ in 0..64 {
            w.tick_cutscene_elements(1, &mut rand);
            total += w.cutscene.element_frame.particles.len();
        }
        assert_eq!(total, 0, "the gate is clear until a script raises it");

        // The script raises it and the same element starts emitting.
        w.set_ambient_particles_enabled(true);
        for _ in 0..64 {
            w.tick_cutscene_elements(1, &mut rand);
            total += w.cutscene.element_frame.particles.len();
        }
        assert!(total > 0, "the gate must let the scene arm emit");

        // A second cold entry replaces rather than stacks.
        w.install_field_scene_elements(FieldEntryMode::Cold, span);
        assert_eq!(w.cutscene.elements.len(), 1);
    }

    #[test]
    fn a_battle_parks_the_ambient_emitter_without_drawing_rand() {
        let mut w = world();
        w.spawn_ambient_emitter(AmbientScene {
            enabled: true,
            dense: false,
            span: crate::cutscene_script_elements::SceneSpan {
                x_min: -100,
                x_max: 100,
                y_min: -60,
                y_max: 60,
            },
            camera_x: 0,
            camera_y: 0,
        });
        w.mode = SceneMode::Battle;
        let mut draws = 0u32;
        for _ in 0..64 {
            w.tick_cutscene_elements(1, || {
                draws += 1;
                0
            });
        }
        assert_eq!(draws, 0, "field-overlay code does not run in a fight");
        assert_eq!(w.cutscene.elements.len(), 1, "parked, not retired");
        w.mode = SceneMode::Field;
        w.tick_cutscene_elements(1, || {
            draws += 1;
            0
        });
        assert!(draws > 0, "the field it belongs to resumes it");
    }

    #[test]
    fn the_ambient_emitter_runs_on_the_channel_and_stays_in_its_span() {
        let mut w = world();
        let scene = AmbientScene {
            enabled: true,
            dense: false,
            span: crate::cutscene_script_elements::SceneSpan {
                x_min: -100,
                x_max: 100,
                y_min: -60,
                y_max: 60,
            },
            camera_x: 0,
            camera_y: 0,
        };
        w.spawn_ambient_emitter(scene);
        let mut seed = 0x1234_5678u32;
        let mut rand = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            seed >> 8
        };
        let mut total = 0;
        for _ in 0..64 {
            w.tick_cutscene_elements(1, &mut rand);
            total += w.cutscene.element_frame.particles.len();
        }
        assert!(total > 0, "the scene arm must emit something in 64 frames");
        // The emitter never retires itself - it is a scene ambience, not a
        // one-shot - so it must still be on the channel.
        assert_eq!(w.cutscene.elements.len(), 1);
    }

    #[test]
    fn an_empty_channel_publishes_an_empty_frame() {
        let mut w = world();
        w.tick_cutscene_elements(1, || 0);
        assert!(w.cutscene.element_frame.is_empty());
    }
}
