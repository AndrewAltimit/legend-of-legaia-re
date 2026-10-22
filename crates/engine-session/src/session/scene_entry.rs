//! Entering a scene live: New Game, the field / world-map / generic entries,
//! the save resume and the native half of a card load.

use super::*;
use legaia_engine_save_state::WorldSaveState;

impl<S: AudioSink> BootSession<S> {
    /// Begin a New Game: clear the world to a fresh slate
    /// ([`legaia_engine_core::world::World::begin_new_game`]) and seed the
    /// starting party (Vahn) from the boot source's `SCUS_942.54` template.
    ///
    /// Mirrors the retail NEW GAME → field-launch chain (master mode 2 → 3,
    /// see `docs/subsystems/boot.md`). The opening scene
    /// ([`legaia_game_tables::new_game::OPENING_CUTSCENE_SCENE`] = `opdeene`, the
    /// prologue cutscene, which hands off to `town01`) is entered through the
    /// usual [`BootSession::enter_field_live`] path; this call only resets and
    /// seeds the world state. When the SCUS template isn't available the world
    /// keeps its default scaffold party so the slice stays runnable.
    pub fn begin_new_game(&mut self) {
        self.host.world.begin_new_game_seeded(
            self.starting_party.as_ref(),
            self.starting_inventory.as_ref(),
        );
    }

    /// Start a New Game end to end: the seeded slate
    /// ([`Self::begin_new_game`]), the title theme stopped so the prologue's
    /// own BGM (or its scripted silence) owns the audio, then the opening
    /// scene through the shared order
    /// ([`legaia_engine_scene_host::resume::enter_new_game`]: the prologue cutscene
    /// `opdeene`, else `town01`). Returns the scene entered, or `None` when
    /// neither would enter (the world keeps its seeded slate on whatever
    /// scene was running).
    ///
    /// The browser play page's `play_new_game` is the paired entry: both
    /// hosts' title New Game and post-wipe New Game go through one of the two.
    pub fn start_new_game(&mut self, opts: &FieldLiveOpts) -> Option<&'static str> {
        start_new_game_on(&mut SessionNewGame {
            session: self,
            opts,
        })
    }

    /// Begin the New Game slate and hand the score over: the seeded world
    /// ([`Self::begin_new_game`]) and the title theme stopped so the
    /// prologue's own BGM (or its scripted silence) owns the audio. The
    /// first half of [`start_new_game_on`].
    pub fn begin_new_game_slate(&mut self) {
        self.begin_new_game();
        if let Some(bgm) = self.bgm.as_mut() {
            bgm.stop();
        }
    }

    /// `Ok` when the host's loaded scene is `scene`. [`Self::enter_field_live`]
    /// logs a failed scene entry and returns `Ok` anyway (its oracle and
    /// headless callers keep running on the old scene by design), so the
    /// resume / New Game fallbacks test the landing itself - without this a
    /// save naming a scene that would not load "entered" it and skipped the
    /// fallback to the running scene.
    fn confirm_scene_landed(&self, scene: &str) -> Result<()> {
        match self.host.scene.as_ref() {
            Some(s) if s.name == scene => Ok(()),
            other => anyhow::bail!(
                "scene '{scene}' did not load (still on {:?})",
                other.map(|s| s.name.as_str())
            ),
        }
    }

    /// Drop the world into a live field scene: run the scene's event-script
    /// record 0 (the init prologue) so the field VM actually ticks, install
    /// the per-scene encounter table, and arm the live gameplay loop per
    /// `opts`.
    ///
    /// [`BootSession::open`] only calls `load_scene`, which leaves the world
    /// in [`SceneMode::Title`] with no field events firing. This is the
    /// reusable core of the windowed host's `--live-loop` setup, shared so the
    /// v0.1 oracle and headless drivers reach Field/Battle the same way the
    /// window does.
    ///
    /// Soft-fails the same way the window does: a scene with no event script
    /// logs and continues (the world stays in whatever mode it was in).
    /// Returns the active [`SceneMode`] after the attempt.
    pub fn enter_field_live(&mut self, scene: &str, opts: &FieldLiveOpts) -> Result<SceneMode> {
        // Retail reaches the field through the mode table, not through a call:
        // whoever wants the field stores `MAIN INIT` (2) - the title
        // dispatcher does it at `0x801DFC00` - and mode 2's handler
        // `FUN_80025B64` stages the field overlay and calls the per-scene
        // initializer `FUN_801D6704` before handing the word to `MAIN MODE`
        // at `0x80025E50`. The port replaces the overlay load with native
        // scene entry, so the INIT column here is the plan and the body
        // below is the staging it names.
        {
            use legaia_engine_mode::mode::{GameMode, ModeInitPlan};
            let plan = self
                .mode_seat
                .enter(GameMode::MainInit, &mut self.host.world);
            debug_assert!(
                matches!(plan, Some(ModeInitPlan::Stage(st))
                    if st.overlay_entry == legaia_engine_title::title_overlay::FIELD_SCENE_INIT_PC),
                "MAIN INIT's plan should name the per-scene initializer"
            );
        }
        match self.host.enter_field_scene(scene, 0) {
            Ok(()) => log::info!("entered field scene '{scene}' record 0 (field VM live)"),
            Err(e) => log::warn!(
                "enter_field_scene('{scene}', 0) failed ({e:#}); staying on the load_scene-only \
                 path (field VM will not tick)"
            ),
        }
        // A direct entry (dev warp, load from a save) is not a `SceneEntered`
        // tick event, so the camera-side reset that event drives must run
        // here: an interrupted cutscene's shot otherwise frames the new scene
        // (`Camera::reset_for_scene_entry`). The browser play page's
        // `enter_field` is the paired site.
        self.camera.reset_for_scene_entry();

        let world = &mut self.host.world;

        // Install the equipment / spell / item catalogs unconditionally so
        // every consumer - not just the battle loop - sees real data. The
        // field pause-menu (Equip / Magic / Items screens) reads these off
        // the world; before they were flag-gated and the menu fell back to
        // throwaway vanilla()/new() placeholders that ignored disc data.
        // Each prefers the disc-accurate real-id table and falls back to the
        // fabricated-id vanilla catalog on disc-free builds.
        world.set_equipment_table(self.equip_modifier_table.clone().unwrap_or_else(|| {
            legaia_engine_menus::equipment::vanilla_equipment_catalog().to_modifier_table()
        }));
        if let Some(stats) = self.equip_stats.clone() {
            world.set_equip_stats(stats);
        }
        world.set_spell_catalog(
            self.spell_catalog
                .clone()
                .unwrap_or_else(legaia_engine_battle::retail_magic::retail_seru_magic_catalog),
        );
        world.set_item_catalog(legaia_engine_menus::items::ItemCatalog::vanilla());
        if let Some(steal) = self.steal_table.clone() {
            world.set_steal_table(steal);
        }
        // The SCUS starting-party template is a disc table like the ones
        // above, not New Game state: a member who joins after a card load
        // takes their level-1 record from it exactly as one who joins in
        // the run that started the game (`World::set_active_party`). Left
        // to the New Game path alone, a resumed save reached Noa's `cave01`
        // join with no template, and she arrived as a zeroed record - no
        // name for the naming prompt to confirm, 0 / 0 HP.
        if world.tables.starting_party.is_none() {
            world.tables.starting_party = self.starting_party.clone();
        }

        // Scene label + encounter fallback + the loop / player-battle / BGM
        // arming are the browser host's business too, so they live in one
        // kernel both hosts call (`World::arm_live_loop`). Only the catalogs
        // above stay here - they are disc-derived on native.
        world.arm_live_loop(scene, &opts.to_live_loop_opts());

        self.restage_audio_for_direct_entry();
        Ok(self.host.world.mode)
    }

    /// The audio half of a **direct** scene entry (dev warp, prologue skip,
    /// save load) - the browser play page's `enter_field` makes the same
    /// three moves. A deliberate scene boot restages BGM from scratch: SFX
    /// cues queued against the old scene are dropped, the dedupe latch is
    /// cleared so the scene's own op-`0x35` start is honoured even when it
    /// names the track already playing (which re-stages that track's bank).
    /// A door does not come through here; [`Self::after_scene_swap`] keeps
    /// the latch so a carried track keeps its playhead. No-op until audio is
    /// up.
    fn restage_audio_for_direct_entry(&mut self) {
        if let Some(bgm) = self.bgm.as_mut() {
            bgm.clear_sfx();
            bgm.last_started = None;
        }
    }

    /// Enter a world-map scene live: load the scene's resources, route its
    /// region-keyed encounter table onto the overworld, install the player
    /// actor, and switch into [`SceneMode::WorldMap`].
    ///
    /// The window's `--world-map` flag used to call [`World::enter_world_map`]
    /// directly, which only installs the camera controller (a camera-only
    /// debug viewer). This is the playable counterpart to [`Self::enter_field_live`]:
    /// it loads the scene through [`SceneHost::enter_field_scene`] (which seeds
    /// the formation table + monster catalog from the MAN, so overworld
    /// encounters resolve to real monsters), builds the
    /// [`RegionEncounterTable`](legaia_engine_battle::region_encounter::RegionEncounterTable)
    /// from the same MAN, routes it via
    /// [`World::set_world_map_regions`], installs the field player so
    /// `tick_world_map`'s locomotion + per-tile encounter roll run, and enters
    /// world-map mode with the live loop armed.
    ///
    /// Soft-fails like [`Self::enter_field_live`]: a scene that fails to load
    /// logs and continues into world-map mode without a region table (camera
    /// only). Returns the active [`SceneMode`].
    pub fn enter_world_map_live(&mut self, scene: &str, opts: &FieldLiveOpts) -> Result<SceneMode> {
        // The scene load + region routing + world-map mode now live in
        // `SceneHost::enter_world_map_scene`, so the natural boot/transition
        // path (`SceneHost::tick` auto-routing an overworld scene) and this
        // explicit `--world-map` entry seed the overworld identically. This
        // wrapper only layers the live-loop / battle options on top.
        // Same camera-side reset as the field entry above: an overworld
        // entered from the picker after an interrupted scene keeps no shot.
        self.camera.reset_for_scene_entry();
        match self.host.enter_world_map_scene(scene) {
            Ok(()) => log::info!("entered world-map scene '{scene}' (overworld seeded)"),
            Err(e) => {
                log::warn!(
                    "enter_world_map_scene('{scene}') failed ({e:#}); world map camera-only"
                );
                // Still switch into world-map mode so the window has a camera.
                self.host.world.set_active_scene_label(scene);
                self.host.world.enter_world_map();
            }
        }

        let equip_table = self.equip_modifier_table.clone().unwrap_or_else(|| {
            legaia_engine_menus::equipment::vanilla_equipment_catalog().to_modifier_table()
        });
        let world = &mut self.host.world;
        world.set_equipment_table(equip_table);
        if opts.player_battle {
            world.set_item_catalog(legaia_engine_menus::items::ItemCatalog::vanilla());
            world.set_spell_catalog(
                self.spell_catalog
                    .clone()
                    .unwrap_or_else(legaia_engine_battle::retail_magic::retail_seru_magic_catalog),
            );
        }
        // The overworld always arms the loop (it is the encounter surface),
        // through the same shared kernel the field path uses.
        let mut live_opts = opts.to_live_loop_opts();
        live_opts.live_loop = true;
        world.arm_live_loop(scene, &live_opts);
        // A direct overworld entry (picker, save load) is a deliberate scene
        // boot like the field entry above, and gets the same audio restage;
        // the browser page's `enter_field` makes it for both kinds.
        self.restage_audio_for_direct_entry();
        Ok(self.host.world.mode)
    }

    /// Enter `scene` live through whichever entry the scene's own label
    /// names: an overworld label ([`legaia_engine_scene::scene::is_world_map_scene`])
    /// through [`Self::enter_world_map_live`], anything else through
    /// [`Self::enter_field_live`]. The same one-predicate branch the browser
    /// play page's `enter_field` and the in-world door transition take.
    pub fn enter_scene_live(&mut self, scene: &str, opts: &FieldLiveOpts) -> Result<SceneMode> {
        if legaia_engine_scene::scene::is_world_map_scene(scene) {
            self.enter_world_map_live(scene, opts)
        } else {
            self.enter_field_live(scene, opts)
        }
    }

    /// Enter a scene live, then seed the world from a saved game.
    ///
    /// The entry goes through [`Self::enter_scene_live`], so a save written
    /// on a kingdom overworld (`mapNN` - where most saves are written)
    /// resumes in world-map mode. It used to call [`Self::enter_field_live`]
    /// unconditionally, which loaded the overworld as a plain field scene
    /// with no region table and no overworld controller, while the browser
    /// page's card Load routed the same label through the world-map entry.
    ///
    /// [`Self::enter_field_live`] cold-boots the scene at record 0 (a fresh
    /// party, no story progress). This variant runs that path and then
    /// hydrates the world from `save` via [`WorldSaveState::load_full`](legaia_engine_save_state::WorldSaveState::load_full)
    /// (party records, story flags, money, inventory) so the field VM sees the
    /// saved story state on its first tick. It is the building block for
    /// "continue a saved game" and for the story-gated paths that a cold boot
    /// into record 0 can't reach, such as a scripted-encounter trigger armed
    /// by story state.
    ///
    /// The save is applied *after* the scene is entered, so the scene record
    /// is still 0; selecting the story-appropriate record from the seeded
    /// flags is a separate concern (the field VM's record picker).
    ///
    /// To seed from a retail memory-card SC block, parse it first with
    /// [`legaia_save::SaveFile::from_retail_sc_block`].
    pub fn enter_field_live_from_save(
        &mut self,
        scene: &str,
        opts: &FieldLiveOpts,
        save: legaia_save::SaveFile,
    ) -> Result<SceneMode> {
        self.enter_scene_live(scene, opts)?;
        self.host.world.load_full(save);
        self.host.refresh_party_battle_inputs();
        log::info!("seeded world from save ({} party records)", {
            self.host.world.party.party_count
        });
        Ok(self.host.world.mode)
    }

    /// Resume a loaded save the way both hosts resume one,
    /// [`legaia_engine_scene_host::resume::resume_card_load`]: seed the saved
    /// story flags, land it through
    /// [`legaia_engine_scene_host::resume::land_save`] (the save's own scene, else
    /// the scene already running, else the opening town - never a New Game),
    /// entering scenes through [`Self::enter_scene_live`], then hydrate the
    /// whole save over the landing.
    ///
    /// `save_scene` is the save's resume label ([`legaia_save::SaveResume::scene`],
    /// empty for a file that carries none). The caller rebuilds its
    /// render-side scene state when [`ResumeLanding::entered_scene`] is set.
    /// The browser play page's `play_resume_save` is the paired entry.
    ///
    /// [`ResumeLanding::entered_scene`]: legaia_engine_scene_host::resume::ResumeLanding::entered_scene
    pub fn resume_save(
        &mut self,
        save: legaia_save::SaveFile,
        save_scene: &str,
        opts: &FieldLiveOpts,
    ) -> legaia_engine_scene_host::resume::ResumeLanding {
        // The order (story flags, landing, whole save) is the shared
        // kernel's - `resume_card_load`, which the browser page runs too.
        let landing = legaia_engine_scene_host::resume::resume_card_load(
            &mut SessionCardLoad {
                session: self,
                opts,
            },
            save,
            save_scene,
        );
        log::info!(
            "resume: landed {} ({:?}); seeded world from save ({} party records)",
            landing.kind(),
            landing.scene(),
            self.host.world.party.party_count
        );
        landing
    }

    /// Where a save written now would resume
    /// ([`legaia_engine_scene_host::scene_host::SceneHost::current_resume`], the one
    /// derivation the page's card and LGSF writers call too).
    pub fn current_resume(&self) -> legaia_save::SaveResume {
        self.host.current_resume()
    }
}

/// A host's half of a New Game ([`start_new_game_on`]): the session it runs
/// and the way it enters a scene. The native window and every headless
/// driver enter through [`BootSession::enter_field_live`]
/// ([`BootSession::start_new_game`]); the browser play page enters through
/// its own field entry, which also rebuilds what it draws.
pub trait NewGameHost {
    /// The audio output the host's session runs over.
    type Sink: AudioSink;
    /// The host's session, once one exists.
    fn new_game_session(&mut self) -> Option<&mut BootSession<Self::Sink>>;
    /// Enter `scene` as an opening leg (not a picker visit, not a resume).
    fn enter_opening_scene(&mut self, scene: &str) -> std::result::Result<(), String>;
}

/// Start a New Game end to end, on any host: the seeded slate and the score
/// handed over ([`BootSession::begin_new_game_slate`]), then the opening
/// through the shared order
/// ([`legaia_engine_scene_host::resume::enter_new_game`]: the prologue
/// cutscene `opdeene`, else `town01`), each leg entered through the host and
/// confirmed by the scene the session then holds. `None` when the host has
/// no session or neither scene would enter.
pub fn start_new_game_on<H: NewGameHost>(host: &mut H) -> Option<&'static str> {
    host.new_game_session()?.begin_new_game_slate();
    legaia_engine_scene_host::resume::enter_new_game(|scene| {
        host.enter_opening_scene(scene)
            .map_err(anyhow::Error::msg)?;
        let session = host
            .new_game_session()
            .ok_or_else(|| anyhow::anyhow!("the session went away entering '{scene}'"))?;
        session.confirm_scene_landed(scene)
    })
}

/// The session's own half of a New Game: the live field entry.
struct SessionNewGame<'a, S: AudioSink> {
    session: &'a mut BootSession<S>,
    opts: &'a FieldLiveOpts,
}

impl<S: AudioSink> NewGameHost for SessionNewGame<'_, S> {
    type Sink = S;

    fn new_game_session(&mut self) -> Option<&mut BootSession<S>> {
        Some(self.session)
    }

    fn enter_opening_scene(&mut self, scene: &str) -> std::result::Result<(), String> {
        self.session
            .enter_field_live(scene, self.opts)
            .map(|_| ())
            .map_err(|e| format!("{e:#}"))
    }
}

/// The session's half of a card load
/// ([`legaia_engine_scene_host::resume::resume_card_load`]).
struct SessionCardLoad<'a, S: AudioSink> {
    session: &'a mut BootSession<S>,
    opts: &'a FieldLiveOpts,
}

impl<S: AudioSink> legaia_engine_scene_host::resume::CardLoadHost for SessionCardLoad<'_, S> {
    fn card_load_world(&mut self) -> &mut legaia_engine_core::world::World {
        &mut self.session.host.world
    }

    fn card_load_running_scene(&self) -> Option<String> {
        self.session.host.scene.as_ref().map(|s| s.name.clone())
    }

    fn card_load_enter(
        &mut self,
        scene: &str,
        save: &legaia_save::SaveFile,
        save_scene: &str,
    ) -> Result<(), String> {
        // The saved scene is entered at the save's own position, as retail's
        // card load seats it (`SceneHost::arm_resume_seat`).
        let armed = self.session.host.arm_resume_seat(save, save_scene, scene);
        let entered = self
            .session
            .enter_scene_live(scene, self.opts)
            .and_then(|_| self.session.confirm_scene_landed(scene));
        if entered.is_err() && armed {
            self.session.host.disarm_entry_seat();
        }
        entered.map(|_| ()).map_err(|e| format!("{e:#}"))
    }

    fn card_load_hydrated(&mut self) {
        // The save's equipment prices the arts input, as retail's card load
        // (hydrate, then scene load) selects it.
        self.session.host.refresh_party_battle_inputs();
    }
}
