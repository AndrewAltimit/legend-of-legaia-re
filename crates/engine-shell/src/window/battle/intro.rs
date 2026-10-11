//! The field-to-battle intro: arming it for a formation, capturing the
//! frame it wipes from, and handing each frame's screen prims to the redraw.

use super::*;

impl PlayWindowApp {
    /// Arm, advance or drop the field-to-battle intro emitter so it tracks the
    /// encounter session's `Transition` phase, and return this frame's screen
    /// primitives.
    ///
    /// This is the host half of `legaia_engine_render::battle_intro`. The
    /// simulation half is already live: `World::tick_encounter` runs
    /// `tick_transition` every frame the session sits in `Transition`, and
    /// `World::battle.intro` carries the entity whose `+0x1A` clock the styles
    /// ride. What was missing was an owner for the per-style working set and
    /// a route from its output into a draw call - both of which land here.
    ///
    /// The emitter adopts the live entity's clock rather than counting for
    /// itself, so a dropped or repeated simulation tick cannot desynchronise
    /// the visuals from the handoff the same state machine performs.
    ///
    /// Returns an empty list whenever no transition is running, which is also
    /// what makes the caller's target choice a two-arm match rather than a
    /// mode test.
    pub(in crate::window) fn take_battle_intro_frame(
        &mut self,
    ) -> (Option<BattleIntro>, Vec<ScreenPrim>) {
        use legaia_engine_battle::encounter::EncounterPhase;

        let phase = self
            .session
            .host
            .world
            .encounters
            .session
            .as_ref()
            .map(|s| s.phase());
        let Some(EncounterPhase::Transition { roll, .. }) = phase else {
            self.battle_intro = None;
            self.battle_intro_vram = None;
            return (None, Vec::new());
        };
        let Some(entity) = self.session.host.world.battle.intro else {
            return (self.battle_intro.take(), Vec::new());
        };
        let total = self
            .session
            .host
            .world
            .encounters
            .session
            .as_ref()
            .map(|s| i32::from(s.transition_frames))
            .unwrap_or(0);

        if self.battle_intro.is_none() {
            self.battle_intro = Some(self.arm_battle_intro(roll.formation_id, total));
        }
        // Hand the emitter out by value: the render pass needs it while the
        // renderer holds an immutable borrow of `self`, which a `&mut self`
        // method could not have. The caller puts it back.
        let mut intro = self.battle_intro.take().expect("armed above");
        // Stepped to the entity's clock, one step per clock unit: a redraw can
        // drain several world ticks, and stepping once per redraw left the
        // shatter behind the browser page's (`BattleIntro::advance_to`).
        let prims = intro.advance_to(entity.elapsed).prims;
        // Per-frame emitter yield. The styles differ by what they draw on top
        // of the captured field frame, so a style that emits nothing but the
        // fade is indistinguishable from a blank transition on screen - this
        // is the cheapest way to tell the two apart in a screenshot run.
        if log::log_enabled!(log::Level::Debug) {
            let mut lo = [i16::MAX; 2];
            let mut hi = [i16::MIN; 2];
            for p in &prims {
                let xy = match p {
                    ScreenPrim::Textured(q) => q.xy,
                    ScreenPrim::Flat(q) => q.xy,
                };
                for (x, y) in xy {
                    lo[0] = lo[0].min(x);
                    lo[1] = lo[1].min(y);
                    hi[0] = hi[0].max(x);
                    hi[1] = hi[1].max(y);
                }
            }
            log::debug!(
                "battle intro {:?} frame {} -> {} prim(s) screen bbox {lo:?}..{hi:?}",
                intro.style(),
                entity.elapsed,
                prims.len()
            );
        }
        (Some(intro), prims)
    }

    /// Build the emitter for the battle about to open.
    ///
    /// The style is **not** a choice the port makes: `select_intro_style` is a
    /// port of the intro overlay's own init block, which keys on the battle
    /// flags byte, the formation's first monster id and the current scene. The
    /// engine feeds it the resolved formation and the loaded scene so a given
    /// battle gets the style retail gives it.
    pub(super) fn arm_battle_intro(&self, formation_id: u16, total: i32) -> BattleIntro {
        // All three selector inputs come out of one engine-side resolver
        // (`SceneHost::battle_intro_style_inputs`), and the arming itself -
        // the PROT 0979 tables with their disc-free fallbacks, the shade
        // pack, the env seeds - is the one `BattleIntro::arm_for_battle` the
        // browser play page calls too.
        let host = &self.session.host;
        let inputs = host.battle_intro_style_inputs(formation_id);
        let overlay = host
            .index
            .entry_bytes_extended(legaia_render_kernels::battle_intro::INTRO_OVERLAY_PROT)
            .ok();
        let shade = host
            .index
            .entry_bytes(legaia_actor_formats::field_char_textures::PROT_ENTRY_INDEX)
            .ok();
        let intro = BattleIntro::arm_for_battle(
            &inputs,
            total,
            overlay.as_deref(),
            shade.as_deref().map(Vec::as_slice),
            host.world.rng_state,
        );
        log::info!(
            "play-window: battle intro style {:?} (sub {}) for formation slot0 {:#04x}, \
             battle flags {:#04x}",
            intro.style(),
            intro.sub_style(),
            inputs.formation_slot0,
            inputs.battle_flags
        );
        intro
    }

    /// Bring the transition's private VRAM page up to date for this frame and
    /// upload it.
    ///
    /// This is the port's equivalent of the console's framebuffer *being*
    /// VRAM. Two things ride on it. The **capture** is a one-shot: retail's
    /// strips sample the frame the GPU drew moments earlier, so the port
    /// re-renders the field scene offscreen and blits it into the rect the
    /// style's texture pages decode to. The **curtain's intermediate** is
    /// per-frame: its column pass renders into VRAM `(320, 0)` and its row
    /// pass samples that, so the page changes every frame of a curtain
    /// transition (see `legaia_engine_render::battle_intro`).
    ///
    /// An **associated** function, not a method: the render pass calls it
    /// while the renderer is borrowed out of `self`, so it takes the three
    /// pieces it needs rather than `&mut self`. The scene's pristine page is
    /// borrowed and cloned inside, never edited.
    ///
    /// Returns a freshly uploaded page whenever the contents changed, or
    /// `None` when the previously uploaded one is still correct (no emitter,
    /// no base, or a style whose page is a one-shot already landed).
    pub(in crate::window) fn capture_battle_intro_frame(
        intro: Option<&mut BattleIntro>,
        renderer: &legaia_engine_render::Renderer,
        scene: &RenderScene<'_>,
        base: Option<&legaia_tim::Vram>,
    ) -> Option<legaia_engine_render::UploadedVram> {
        let intro = intro?;
        let base = base?;
        let first = intro.needs_capture();
        let page = match legaia_engine_render::battle_intro::update_field_capture(
            intro,
            renderer,
            legaia_engine_render::RenderTarget::Scene(scene),
            base,
        ) {
            Ok(Some(p)) => p,
            Ok(None) => return None,
            Err(e) => {
                log::error!("play-window: battle intro capture: {e:#}");
                return None;
            }
        };
        match renderer.upload_vram(page) {
            Ok(v) => {
                if first {
                    log::info!("play-window: battle intro captured the field frame into VRAM");
                }
                Some(v)
            }
            Err(e) => {
                log::error!("play-window: battle intro VRAM upload: {e:#}");
                None
            }
        }
    }
}
