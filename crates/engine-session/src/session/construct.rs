//! Building a session: the scene source open, `BootSession::from_host` (the
//! one body both play hosts construct through, with its world installs and
//! audio bring-up), the host-policy switches and the SFX region staging.

use super::*;

impl<S: AudioSink> BootSession<S> {
    /// Build a session over an already-open [`SceneHost`] and the disc's
    /// `SCUS_942.54` (`None` on a disc-free boot), then load `cfg.scene`.
    /// The path constructors ([`BootSession::open`] / `open_disc`) land
    /// here, and so does a host that assembled its `SceneHost` from bytes
    /// in memory (the browser). `open_audio` opens the host's output and
    /// runs only when `cfg.enable_audio` is set.
    pub fn from_host(
        mut host: SceneHost,
        scus: Option<&[u8]>,
        cfg: &BootConfig,
        open_audio: impl FnOnce() -> Result<S>,
    ) -> Result<Self> {
        // Parse the new-game starting-party template from the same source
        // (best-effort; never fails the boot).
        let starting_party = scus.and_then(read_starting_party);
        let starting_inventory = scus.and_then(read_starting_inventory);
        let (equip_modifier_table, equip_restrictions, equip_stats) =
            match scus.and_then(read_retail_equip_tables) {
                Some((m, r, s)) => (Some(m), Some(r), Some(s)),
                None => (None, None, None),
            };
        let spell_catalog = scus.and_then(read_retail_spell_catalog);
        let steal_table = scus.and_then(legaia_game_tables::steal_table::StealTable::from_scus);
        // Wire the CDNAME-derived map-id resolver so field-VM scene
        // transitions resolve to the right CDNAME label.
        host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
        // Free-roam liveliness (NPC patrol routes + the ambient walk
        // mirror) on, as both play hosts run it: it is retail behaviour, and
        // it never engages a placement's script (only a touch does), so the headless drivers - ladders,
        // oracles, the retail comparison corpus - see the world a player sees.
        host.world.npcs.animate = true;

        // Retail proportional dialog font off the disc (no save state). See
        // `BootSession::dialog_font`.
        let dialog_font = scus.and_then(|scus| read_dialog_font(&host.index, scus));
        let escape_icons = scus.and_then(|scus| {
            use legaia_font::escape_icons::{EscapeIcons, ICON_PROT_DAT_LEN, ICON_PROT_DAT_OFFSET};
            let head = host
                .index
                .prot_dat_raw_bytes(ICON_PROT_DAT_OFFSET, ICON_PROT_DAT_LEN)
                .ok()?;
            EscapeIcons::from_disc(&head, scus).ok()
        });
        if dialog_font.is_none() {
            log::warn!(
                "dialog font not decodable from the boot source; \
                 text falls back to extracted/ or the placeholder"
            );
        }

        // Hand the host the retail new-game defaults so a cold `--scene X`
        // boot (no New Game confirm, no save loaded) seeds the template party
        // + starting bag at scene entry instead of leaving a zeroed scaffold
        // roster behind the pause menu. Guarded inside `enter_field_scene` -
        // it never fires once a party or save is installed.
        host.new_game_defaults =
            starting_party
                .clone()
                .map(|party| legaia_engine_menus::new_game::NewGameDefaults {
                    party,
                    inventory: starting_inventory.clone(),
                    // Retail's Vahn-alone roster by default: every headless
                    // harness cold-boots `town01` as the opening. The
                    // `play-window` scene picker raises it (`window/run.rs`).
                    picker_party: false,
                    equip_stats: equip_stats.clone(),
                });

        // The static-SCUS progression tables (XP curve + Noa/Gala correction
        // divisors, stat growth, victory pose, XA cue durations, magic-XP
        // thresholds, accessory passives) through the one engine install the
        // browser play page's `load_disc` calls too. Best-effort: absent on a
        // disc-free build, where each consumer keeps its default. Persists
        // across New Game.
        // Both halves of the battle chip / caption labels - the overlay half
        // (banner sentences, `Spirit`, `Escape`, the Ra-Seru names) and the
        // SCUS half (`Begin`, `Run`, `Attack`, ... and the sparring fight's
        // opening caption) - through the one builder the browser runtime's
        // `load_disc` calls too.
        host.world.battle.ui_strings =
            legaia_engine_scene::battle_overlay_readers::battle_ui_strings_for_disc(
                &host.index,
                scus,
            );
        // The party cast trigger's anim-pair lists and the monster casts'
        // opening camera shots, off the same battle-overlay image, for every
        // session - headless ones included, since a monster cast stages its
        // shot from them (the browser runtime's `load_disc` reads the same).
        host.world.battle.spell_anim_pairs =
            legaia_engine_scene::battle_overlay_readers::spell_anim_pairs_from_prot(&host.index);
        if let Some(scus) = scus {
            host.world.install_retail_progression_tables(scus);
            // Pause-menu text: item names + info-window descriptions,
            // spell names / descriptions, accessory passive lines. The
            // Items / Magic pause screens resolve their strings here.
            host.world.install_menu_text(scus);
            // Install the randomizer's seru-trade config (the `--seru-trade`
            // blob in preserved rodata). No-op / disabled on a vanilla disc;
            // when present, vendors offer seru-for-seru trades. Persists across
            // New Game.
            host.world.install_seru_trade_config(scus);
        }

        // Install the gold-shop item data (per-id buy price + name mask) from the
        // SCUS item table, so each field scene's merchant offers its real stock
        // at real prices (populated per scene by `enter_field_scene`). Persists
        // across New Game; absent on disc-free builds (stock stays host-supplied).
        if let Some(shop_data) = scus.and_then(read_shop_item_data) {
            host.world.shops.item_shop_data = Some(shop_data);
        }

        // Install the real item-effect descriptor table so the item catalog's
        // field/battle usability gating matches retail (e.g. cure/revive items
        // are battle-only). Best-effort: absent on disc-free builds, where the
        // catalog keeps its curated usability flags.
        if let Some(effects) = scus.and_then(read_retail_item_effects) {
            host.world.set_item_effects(effects);
        }

        // An empty scene name boots no scene: a host that enters its first
        // scene through its own picker (the browser page) loads none here.
        if !cfg.scene.is_empty() {
            host.load_scene(&cfg.scene)
                .with_context(|| format!("load scene '{}'", cfg.scene))?;
        }

        // Audio + BGM director (optional - disabled for headless tests).
        let (audio, bgm) = if cfg.enable_audio {
            match open_audio() {
                Ok(audio) => {
                    // The native AudioOut owns a cpal::Stream which is Send but not Sync.
                    // BootSession is single-threaded (binary + WASM both
                    // tick on one thread); the Arc just gives the BGM
                    // director a refcounted handle.
                    #[allow(clippy::arc_with_non_send_sync)]
                    let audio = Arc::new(audio);
                    // No scene bank is staged: retail loads a bank only with
                    // its track, into the one BGM slot, and a scene-local id
                    // plays a global fallback track
                    // (`legaia_engine_scene_host::scene_host::SCENE_LOCAL_BGM_FALLBACK_ID`).
                    let mut director = AudioDirector::new(audio.clone());
                    // Decode the static SFX descriptor bank from the same
                    // executable once; it names the program/tone/voice-count
                    // for each cue id, and the VAB slot each cue's category
                    // routes to. Best-effort - an empty bank just no-ops
                    // resolved cues.
                    if let Some((sfx, slots)) = scus.and_then(read_sfx_bank) {
                        director.set_sfx_bank(sfx);
                        director.set_sfx_cue_slots(slots);
                    }
                    // Stage the pinned SFX program banks (slot 0 = PROT 0868,
                    // slot 2 = PROT 0869) into the shared SPU region so a cue
                    // resolves against the bank its own category names, not
                    // whatever BGM VAB is open. Best-effort.
                    if let Err(e) = stage_sfx_vab(&mut director, &host) {
                        log::warn!("resident SFX banks not staged: {e:#}");
                    }
                    (Some(audio), Some(director))
                }
                Err(e) => {
                    log::warn!("audio disabled - open failed: {e:#}");
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        Ok(Self {
            host,
            camera: Camera::default(),
            audio,
            bgm,
            frames: 0,
            starting_party,
            starting_inventory,
            equip_modifier_table,
            equip_restrictions,
            equip_stats,
            spell_catalog,
            steal_table,
            dialog_font,
            escape_icons,
            field_menu: None,
            field_menu_sub: None,
            options_state: OptionsState::default(),
            save_rack: SaveRack::Blocks(Vec::new()),
            save_port_blocks: Vec::new(),
            save_flow: SaveScreenFlow::new(),
            pause_wipe: Default::default(),
            last_save_commit: None,
            spell_level_notice: None,
            art_learned_notice: None,
            field_menu_resume: SceneMode::Field,
            mode_seat: legaia_engine_mode::mode::ModeSeat::new_at_boot(),
            host_drains_queues: false,
            queue_marks: HostQueueMarks::default(),
            camera_azimuth_override: None,
            host_owns_pause_menu: false,
            footstep: Default::default(),
            footstep_prev_pos: None,
        })
    }

    /// Declare that the caller drains the world's per-tick presentation
    /// queues itself after every [`Self::tick`] (the play window does, routing
    /// each into its renderer and audio). Off by default: the session then
    /// drops whatever the previous tick left unconsumed before the next one
    /// runs ([`HostQueueMarks`]), so a driver that only ticks does not grow
    /// the queues without bound.
    pub fn set_host_drains_queues(&mut self, on: bool) {
        self.host_drains_queues = on;
        self.queue_marks = HostQueueMarks::default();
    }

    /// Declare that the caller opens and drives the pause menu itself, so
    /// [`Self::tick`] leaves the Start edge and [`Self::field_menu`] alone.
    pub fn set_host_owns_pause_menu(&mut self, on: bool) {
        self.host_owns_pause_menu = on;
    }
}

/// Stage the reserved SFX region the way retail's SPU map lays it out:
/// slot `0` = PROT 0868 (the system bank the 16 category-`0` shared UI cues
/// key) resident from boot at the region's bottom, and above it the region
/// VAB slots `2` and `6` share (`FUN_800265E8` gives both `0x33010`).
///
/// The shared region is seeded with the class-2 bank (PROT 0869, slot 2) so a
/// cue fired before the first world tick has somewhere to sound; from then on
/// [`BootSession::route_field_sfx`] refills it with whatever the world's
/// residency names for the current mode - PROT 0876 in slot 6 in the field,
/// PROT 0869 in slot 2 in battle ([`AudioDirector::sync_shared_region`]).
///
/// Each entry is a scene-VAB-style stream (`[u32 chunk header][VAB]...`), so
/// the VAB starts at `+4` (with a `+0` fallback for a bare bank).
fn stage_sfx_vab<S: AudioSink>(director: &mut AudioDirector<S>, host: &SceneHost) -> Result<()> {
    use legaia_engine_mode::sfx_residency::SharedRegionBank;
    use legaia_game_tables::sfx_table::SLOT0_SYSTEM_BANK_PROT_INDEX;

    let bytes = host
        .index
        .entry_bytes_extended(SLOT0_SYSTEM_BANK_PROT_INDEX)
        .context("read the slot-0 system bank")?;
    // At the SFX region's floor, through the layout kernel the page shares;
    // the director keeps the bytes to re-stage the bank after a track that
    // overran the region (the ending theme's) lets it go.
    if !director.stage_resident_slot0(bytes) {
        anyhow::bail!("no VAB header at +4 or +0 in PROT 0868");
    }
    if !director.sync_shared_region(Some(SharedRegionBank::CLASS2), |e| {
        host.index.entry_bytes_extended(e).ok()
    }) {
        log::warn!("class-2 SFX bank not staged in the shared region");
    }
    Ok(())
}
