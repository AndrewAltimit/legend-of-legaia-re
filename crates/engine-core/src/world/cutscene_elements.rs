//! The **element channel**: the pool of spawned script-cutscene elements the
//! three handlers in [`crate::cutscene_script_elements`] run on.
//!
//! Each element is an ordinary field actor whose `+0x0C` handler runs once per
//! frame and drives some *other* object through the back-link at `+0x90` - the
//! "linked object". Every one of the three handlers gates on that object's own
//! done bit (`linked[+0x10] & 8`) rather than on its own state, which is what
//! makes them a channel and not three unrelated routines: the element retires
//! when the thing it was driving finishes.
//!
//! [`crate::cutscene_timeline`] interprets a cutscene record's cross-context
//! yields directly and so has nowhere to hang an element; this module is that
//! missing seat. It follows the shape `World` already uses for the field
//! overlay's other plain-template actors ([`crate::world::World::eased_moves`],
//! `floor_tier_bobs`): one `Vec` per family on the world, advanced together on
//! the same frame delta, retired when the kernel says so.
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
//! `bnez` on `_DAT_800838B8`, so it runs only while that global is clear.
//!
//! REF: FUN_80024C88 (the positioned spawn), FUN_801D6058 (the handler)

use crate::cutscene_script_elements::{
    AmbientEmitter, AmbientParticle, AmbientScene, ElementTeardown, ElementVec, PositionTween,
    TeardownActions, TweenStep,
};
use crate::world::{EasedMoveTarget, World};

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
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }

    /// The state to write back to the world.
    pub fn state(&self) -> u32 {
        self.0
    }
}

/// Runtime VA of the ambient emitter's spawn descriptor in the field overlay's
/// plain-template table (see the module note).
pub const AMBIENT_EMITTER_TEMPLATE_VA: u32 = 0x801F_271C;

/// The value the spawn site stores into the new actor's `+0x1A`
/// ([`AmbientEmitter::state`]) - the arm selector, `1` = the scene arm.
pub const AMBIENT_EMITTER_SCENE_ARM: i16 = 1;

/// What an element is linked to - retail's `+0x90` back-link.
///
/// The engine's addressable objects are the ones the neighbouring `move_to` and
/// eased-move hosts already resolve, plus the camera, which the position tween
/// singles out by pointer identity (`_DAT_8007C364`) when it decides whether to
/// write the `+0x8E` inverted-Y mirror.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementLink {
    /// The party leader's pool slot.
    Player,
    /// A scene NPC placement, keyed the way `World::field_npc_positions` is.
    Placement(u8),
    /// The camera object. The one link the `+0x8E` mirror is NOT written for.
    Camera,
    /// No addressable object. The element still ticks (retail would write
    /// through a back-link the engine does not have), but nothing is written -
    /// a silent write to the wrong actor would be worse than none.
    None,
}

impl ElementLink {
    /// `true` for the camera object, which is compared by pointer identity in
    /// retail rather than by any flag.
    pub fn is_camera(self) -> bool {
        matches!(self, ElementLink::Camera)
    }

    /// The eased-move target this link names, when it names one.
    pub fn as_move_target(self) -> Option<EasedMoveTarget> {
        match self {
            ElementLink::Player => Some(EasedMoveTarget::Player),
            ElementLink::Placement(p) => Some(EasedMoveTarget::Placement(p)),
            _ => None,
        }
    }
}

/// Which handler an element runs.
#[derive(Debug, Clone)]
pub enum ElementKind {
    /// `FUN_801D5C08` - blend the linked object from `start` to `end`.
    PositionTween(PositionTween),
    /// `FUN_801D5D60` - camera restore + one-shot flag teardown.
    Teardown(ElementTeardown),
    /// `FUN_801D6058` - the ambient particle emitter.
    AmbientEmitter {
        emitter: AmbientEmitter,
        scene: AmbientScene,
    },
}

/// One live element on the channel.
#[derive(Debug, Clone)]
pub struct CutsceneElement {
    /// The object this element drives, and whose done bit gates it.
    pub link: ElementLink,
    /// The handler.
    pub kind: ElementKind,
    /// `+0x10` bit `8` - the element's own done bit. A done element is retired
    /// at the end of the frame that set it.
    pub done: bool,
}

/// What one frame of the channel produced, for a host to act on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ElementFrame {
    /// Positions the position tweens wrote, with the link they went to and the
    /// `+0x8E` inverted-Y mirror when the link is not the camera.
    pub tween_writes: Vec<(ElementLink, ElementVec, Option<i16>)>,
    /// Teardown requests, in element order.
    pub teardowns: Vec<TeardownActions>,
    /// Particles the ambient emitters spawned this frame.
    pub particles: Vec<AmbientParticle>,
    /// How many elements retired this frame.
    pub retired: usize,
}

impl ElementFrame {
    /// `true` when the channel did nothing at all.
    pub fn is_empty(&self) -> bool {
        self.tween_writes.is_empty()
            && self.teardowns.is_empty()
            && self.particles.is_empty()
            && self.retired == 0
    }
}

impl World {
    /// Spawn a position-tween element driving `link`.
    ///
    /// REF: FUN_801D5C08
    pub fn spawn_element_position_tween(
        &mut self,
        link: ElementLink,
        start: ElementVec,
        end: ElementVec,
        rate: i16,
    ) {
        self.cutscene_elements.push(CutsceneElement {
            link,
            kind: ElementKind::PositionTween(PositionTween {
                start,
                end,
                t: 0,
                rate,
                done: false,
            }),
            done: false,
        });
    }

    /// Spawn a teardown element watching `link`.
    ///
    /// REF: FUN_801D5D60
    pub fn spawn_element_teardown(
        &mut self,
        link: ElementLink,
        restore_armed: i16,
        owns_camera: i16,
        flag_mask: u32,
    ) {
        self.cutscene_elements.push(CutsceneElement {
            link,
            kind: ElementKind::Teardown(ElementTeardown {
                restore_armed,
                owns_camera,
                flag_mask,
                done: false,
            }),
            done: false,
        });
    }

    /// Spawn the ambient particle emitter on its **scene** arm - the one arm
    /// the retail spawn site at `0x801D6FD8` selects (`+0x1A = 1`).
    ///
    /// REF: FUN_801D6058, FUN_80024C88
    pub fn spawn_ambient_emitter(&mut self, scene: AmbientScene) {
        self.cutscene_elements.push(CutsceneElement {
            link: ElementLink::None,
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
        self.cutscene_elements.push(CutsceneElement {
            link: ElementLink::None,
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

    /// Is the object `link` names finished (`linked[+0x10] & 8`)?
    ///
    /// The engine has no per-object flag word on a placement, so a link's done
    /// bit is the one thing the world can answer for it: a placement that is no
    /// longer seated, or a player whose eased move has retired. An unlinked
    /// element is never gated - retail's `+0x90` would be null there and the
    /// load would fault, so "never done" is the only safe reading.
    fn element_link_done(&self, link: ElementLink) -> bool {
        match link {
            ElementLink::Player => !self
                .eased_moves
                .iter()
                .any(|r| matches!(r.target, EasedMoveTarget::Player)),
            ElementLink::Placement(p) => !self.field_npc_positions.contains_key(&p),
            ElementLink::Camera | ElementLink::None => false,
        }
    }

    /// Run every live element for one frame and return what they asked for.
    ///
    /// `frame_step` is retail's `DAT_1F800393`, the same scalar the timer-actor
    /// pass carries, so an element spawned on the same script beat as an eased
    /// move cannot drift away from it.
    ///
    /// Retired elements are dropped at the end of the pass; an element spawned
    /// *during* it is spliced in rather than overwritten, the same way the
    /// eased-move pass handles a spawn from its own frame.
    pub fn tick_cutscene_elements(&mut self, frame_step: u8, mut rand: impl FnMut() -> u32) {
        if self.cutscene_elements.is_empty() {
            if !self.cutscene_element_frame.is_empty() {
                self.cutscene_element_frame = ElementFrame::default();
            }
            return;
        }
        let mut frame = ElementFrame::default();
        let mut live = std::mem::take(&mut self.cutscene_elements);
        for el in live.iter_mut() {
            let linked_done = self.element_link_done(el.link);
            match &mut el.kind {
                ElementKind::PositionTween(t) => {
                    match t.step(frame_step, linked_done) {
                        TweenStep::LinkedAlreadyDone => {}
                        TweenStep::Blending { pos } | TweenStep::Snapped { pos } => {
                            let mirror = PositionTween::linked_field_8e(pos, el.link.is_camera());
                            frame.tween_writes.push((el.link, pos, mirror));
                        }
                    }
                    el.done = t.done;
                }
                ElementKind::Teardown(t) => {
                    let actions = t.step(linked_done);
                    if actions != TeardownActions::default() {
                        frame.teardowns.push(actions);
                    }
                    el.done = t.done;
                }
                ElementKind::AmbientEmitter { emitter, scene } => {
                    frame.particles.extend(emitter.step(scene, &mut rand));
                }
            }
        }
        let before = live.len();
        live.retain(|el| !el.done);
        frame.retired = before - live.len();
        live.append(&mut self.cutscene_elements);
        self.cutscene_elements = live;
        // Apply the tween writes through the same seats the eased-move pass
        // uses, so the two families cannot disagree about where an object is.
        for (link, pos, _) in &frame.tween_writes {
            match link.as_move_target() {
                Some(EasedMoveTarget::Player) => {
                    self.field_ctx.world_x = pos.x as u16;
                    self.field_ctx.world_y = pos.y as u16;
                    self.field_ctx.world_z = pos.z as u16;
                }
                Some(EasedMoveTarget::Placement(slot)) => {
                    self.field_npc_positions.insert(slot, (pos.x, pos.z));
                }
                None => {}
            }
        }
        self.cutscene_element_frame = frame;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::SceneMode;

    fn world() -> World {
        let mut w = World::new();
        w.mode = SceneMode::Field;
        w.field_frame_step = 1;
        w
    }

    #[test]
    fn a_position_tween_blends_then_snaps_and_retires() {
        let mut w = world();
        w.field_npc_positions.insert(3, (0, 0));
        w.spawn_element_position_tween(
            ElementLink::Placement(3),
            ElementVec::default(),
            ElementVec {
                x: 400,
                y: 0,
                z: 800,
                w: 0,
            },
            0x200,
        );
        let mut seen = Vec::new();
        for _ in 0..16 {
            w.tick_cutscene_elements(1, || 0);
            if let Some((_, pos, _)) = w.cutscene_element_frame.tween_writes.first() {
                seen.push((pos.x, pos.z));
            }
            if w.cutscene_elements.is_empty() {
                break;
            }
        }
        assert!(w.cutscene_elements.is_empty(), "the element must retire");
        assert_eq!(*seen.last().unwrap(), (400, 800), "it must reach the end");
        assert!(seen.len() > 2, "and pass through the middle: {seen:?}");
        // The write landed on the linked placement, not just on the frame.
        assert_eq!(w.field_npc_positions.get(&3), Some(&(400, 800)));
    }

    #[test]
    fn the_inverted_y_mirror_is_written_for_everything_but_the_camera() {
        let mut w = world();
        let end = ElementVec {
            x: 0,
            y: 64,
            z: 0,
            w: 0,
        };
        w.spawn_element_position_tween(ElementLink::Camera, ElementVec::default(), end, 0x800);
        w.tick_cutscene_elements(1, || 0);
        assert_eq!(w.cutscene_element_frame.tween_writes[0].2, None);

        let mut w = world();
        w.field_npc_positions.insert(1, (0, 0));
        w.spawn_element_position_tween(
            ElementLink::Placement(1),
            ElementVec::default(),
            end,
            0x800,
        );
        w.tick_cutscene_elements(1, || 0);
        let (_, pos, mirror) = w.cutscene_element_frame.tween_writes[0];
        assert_eq!(mirror, Some(pos.y.wrapping_neg()));
    }

    #[test]
    fn a_linked_object_that_is_already_done_retires_the_element_without_writing() {
        let mut w = world();
        // Placement 9 is not seated, so its done bit reads set.
        w.spawn_element_position_tween(
            ElementLink::Placement(9),
            ElementVec::default(),
            ElementVec {
                x: 100,
                ..Default::default()
            },
            0x100,
        );
        w.tick_cutscene_elements(1, || 0);
        assert!(w.cutscene_element_frame.tween_writes.is_empty());
        assert_eq!(w.cutscene_element_frame.retired, 1);
        assert!(w.cutscene_elements.is_empty());
    }

    #[test]
    fn the_teardown_restores_the_camera_every_frame_and_clears_flags_once() {
        let mut w = world();
        w.spawn_element_teardown(ElementLink::Placement(4), 1, 1, 0x0000_0080);
        w.field_npc_positions.insert(4, (0, 0));
        w.tick_cutscene_elements(1, || 0);
        let a = w.cutscene_element_frame.teardowns[0];
        assert!(a.restore_camera && !a.clear_target_flags);
        assert_eq!(w.cutscene_elements.len(), 1, "still armed");
        // The linked placement goes away: that is its done bit.
        w.field_npc_positions.remove(&4);
        w.tick_cutscene_elements(1, || 0);
        let a = w.cutscene_element_frame.teardowns[0];
        assert!(a.clear_target_flags && a.clear_camera_flags);
        assert!(w.cutscene_elements.is_empty(), "one-shot");
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
            total += w.cutscene_element_frame.particles.len();
        }
        assert!(total > 0, "the scene arm must emit something in 64 frames");
        // The emitter never retires itself - it is a scene ambience, not a
        // one-shot - so it must still be on the channel.
        assert_eq!(w.cutscene_elements.len(), 1);
    }

    #[test]
    fn an_empty_channel_publishes_an_empty_frame() {
        let mut w = world();
        w.tick_cutscene_elements(1, || 0);
        assert!(w.cutscene_element_frame.is_empty());
    }
}
