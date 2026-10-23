//! The `play-window` engine-runner entry points (`cmd_play_window` /
//! `cmd_play_window_with_record`) and the launch stages they run in order:
//! the intro movie, the session open and its scene entry, the debug seeds,
//! the UI asset decode, the window assembly and the event loop.

use super::*;

#[path = "run/app_assembly.rs"]
mod app_assembly;
#[path = "run/scene_resources.rs"]
mod scene_resources;
#[path = "run/session_seeding.rs"]
mod session_seeding;
#[path = "run/ui_assets.rs"]
mod ui_assets;

use app_assembly::WindowParts;
pub(super) use scene_resources::*;
pub(super) use session_seeding::*;
use ui_assets::*;

#[allow(clippy::too_many_arguments)]
pub fn cmd_play_window(
    scene: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    enable_audio: bool,
    world_map: bool,
    str_file: Option<&Path>,
    boot_ui: bool,
    save_dir: &Path,
    card: Option<&Path>,
    cutscene_map_path: Option<&Path>,
    cheat_file: Option<&Path>,
    cheat_strict: bool,
    live_loop: bool,
    player_battle: bool,
    party: Option<&str>,
    vm_dialogue: bool,
    terrain_y: bool,
    edge_collision: bool,
    solid_npcs: bool,
    live_npcs: bool,
    damage_finish: bool,
    battle_bgm: Option<u16>,
    screenshot: Option<super::ScreenshotConfig>,
    seed_party: bool,
    battle: Option<&str>,
    fidelity: super::LaunchFidelity,
    debug_seeds: super::DebugSeeds,
) -> Result<()> {
    cmd_play_window_with_record(
        scene,
        extracted_root,
        disc,
        enable_audio,
        world_map,
        str_file,
        boot_ui,
        save_dir,
        card,
        cutscene_map_path,
        cheat_file,
        cheat_strict,
        live_loop,
        player_battle,
        party,
        vm_dialogue,
        terrain_y,
        edge_collision,
        solid_npcs,
        live_npcs,
        damage_finish,
        battle_bgm,
        screenshot,
        seed_party,
        battle,
        fidelity,
        debug_seeds,
        None,
    )
}

/// Phase 1: if a STR file is provided (or auto-resolved), play the
/// video in a window first. The user closes (or ESC) the STR window,
/// then the scene window opens.
///
/// When booting from a disc image we can resolve the scene's movie inside
/// the ISO and play it with its interleaved XA audio (read raw 2352-byte
/// sectors). Otherwise we fall back to the filesystem (video only).
///
/// winit allows one event loop per process, so the movie and the scene
/// share this one: the movie runs on it on demand and hands it back, then
/// the scene window runs on it to the end.
///
/// Screenshot mode skips the *auto-resolved* STR (an explicit `--str-file`
/// is still honoured) so a capture is not held behind a movie. The real
/// boot flow enters the prologue 3D scene with no FMV anyway; the
/// prepended STR is a preview-harness convenience.
fn play_intro_movie(
    event_loop: &mut EventLoop<()>,
    scene: &str,
    disc: Option<&Path>,
    str_file: Option<&Path>,
    auto_str: Option<&Path>,
    cutscene_map: &legaia_engine_scene::scene::CutsceneMap,
    screenshot: bool,
) -> Result<()> {
    let resolved_str: Option<&Path> = str_file.or(auto_str);
    let skip_auto_str = screenshot && str_file.is_none();
    if skip_auto_str {
        // No phase-1 window; fall through to the scene window.
    } else if let Some(str_path) = resolved_str {
        play_str_in(event_loop, str_path, None, 640, 480, !screenshot)?;
    } else if let (Some(disc_path), None) = (disc, str_file) {
        // Disc mode, no explicit file: resolve the scene's MV*.STR via the
        // cutscene map / heuristic and play it from the disc with audio.
        if let Some(rel) = cutscene_map.resolve(scene) {
            let iso_path = Path::new(&rel);
            match play_str_in(event_loop, iso_path, Some(disc_path), 640, 480, !screenshot) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("info: scene '{scene}' STR '{rel}' not played from disc ({e:#})")
                }
            }
        }
    }
    Ok(())
}

/// The per-launch world switches a play window sets before its scene is
/// entered.
struct LaunchToggles {
    entry_pulse: bool,
    smart_monsters: bool,
    tweaks: legaia_engine_session::session_switches::GameplayTweaks,
    retail: bool,
    vm_dialogue: bool,
    terrain_y: bool,
    edge_collision: bool,
    solid_npcs: bool,
    live_npcs: bool,
    damage_finish: bool,
}

/// Set the host policy and the world switches of a freshly opened session.
fn apply_launch_toggles(session: &mut NativeSession, toggles: LaunchToggles) {
    let LaunchToggles {
        entry_pulse,
        smart_monsters,
        tweaks,
        retail,
        vm_dialogue,
        terrain_y,
        edge_collision,
        solid_npcs,
        live_npcs,
        damage_finish,
    } = toggles;
    // The window drains every per-tick world queue itself, right after each
    // session tick (`drain_and_log_battle_events` / `drain_and_route_field_events`).
    session.set_host_drains_queues(true);
    // The scene-entry VDF pulse (enhancement) gate is written by the shared
    // switch set below - it must land before the first `enter_field_scene`,
    // which is where the installer runs.
    // Drive field dialogue through the inline-script field-VM runner so branch
    // handlers execute (flag-sets / scene-changes / GIVE_ITEM). On by default;
    // `--simple-dialogue` clears it to fall back to the plain typewriter panel.
    session.host.world.toggles.use_vm_dialogue = vm_dialogue;
    // The window lands its drawn frame for a framebuffer-reading `43 12`
    // copy (the ending vignettes' photo grab) - see the redraw's grab.
    session.host.world.enable_frame_grab(true);
    // Opt-in: snap the player's Y to the per-scene floor height each
    // locomotion step. Off by default → flat-Y behaviour preserved.
    session.host.world.locomotion.follow_terrain_height = terrain_y;
    // Retail's three-probe leading-edge wall footprint (the `DAT_801f2214`
    // standoff), solid NPCs (the `DAT_801f21b4` actor probes) and the
    // villagers' ambient tail-section-1 wander are all retail
    // behaviour and default ON - the same three the browser play page sets
    // unconditionally. `--no-edge-collision` / `--no-solid-npcs` /
    // `--no-live-npcs` clear them (candidate-centre test, walk-through NPCs,
    // NPCs parked at their placement anchors).
    session.host.world.locomotion.leading_edge_wall_probes = edge_collision;
    session.host.world.npcs.solid = solid_npcs;
    session.host.world.npcs.animate = live_npcs;
    // Retail always runs the damage finisher (`FUN_801ddb30`) after the
    // melee roll, so it is the default; `--no-damage-finish` keeps the flat
    // pre-finisher path for comparison.
    session.host.world.toggles.use_damage_finish = damage_finish;
    // `LEGAIA_SMART_MONSTERS` is raised with the other environment switches
    // at session open (`legaia_engine_session::native::apply_env_world_toggles`);
    // `--smart-monsters` (the launcher's Smarter monsters row) raises it too.
    // `--retail` then holds every port-only switch off, whichever source
    // raised it - the play page's retail mode does the same through
    // `legaia_engine_session::session_switches`.
    let switches = legaia_engine_session::session_switches::SessionSwitches {
        smart_monsters: smart_monsters || session.host.world.toggles.smarter_monster_targeting,
        world_map_markers: session.host.world.toggles.overworld_marker_overlay,
        entry_pulse,
        battle_frame_cadence: session.host.world.toggles.battle_sm_frame_cadence,
        arts_saved_list: session.host.world.toggles.arts_saved_list,
        tweaks,
        retail,
    };
    switches.apply(&mut session.host.world.toggles);
}

/// Enter the launch scene: a `--resume-save` landing through the card-load
/// path, an overworld through the world-map entry, anything else through the
/// live field entry.
fn enter_launch_scene(
    session: &mut NativeSession,
    scene: &str,
    world_map: bool,
    debug_seeds: &super::DebugSeeds,
    field_live_opts: &legaia_engine_session::session::FieldLiveOpts,
) -> Result<()> {
    // The direct `--scene` entry is this window's scene picker: a cold entry
    // (no save, no New Game) seeds the full Vahn / Noa / Gala party through
    // the engine's one picker seed (`World::seed_picker_party`), the same one
    // the browser play page's picker reaches. The boot-UI NEW GAME still
    // reseeds retail's Vahn-alone roster; `--seed-party` does too.
    if let Some(defaults) = session.host.new_game_defaults.as_mut() {
        defaults.picker_party = true;
    }
    // Which entry a scene takes is the scene's own property, decided by the
    // one engine predicate every host asks (`is_world_map_scene`) - the
    // browser play page's `enter_field` and the in-world door transition both
    // route an overworld label through the world-map entry by name. Without
    // it `--scene map01` entered the overworld as a plain field scene here
    // unless `--world-map` was also passed. The flag still forces the
    // world-map entry for any other label.
    let world_map = world_map || legaia_engine_scene::scene::is_world_map_scene(scene);
    // `--resume-save`: land an LGSF file through the card-load path instead
    // of the door entry below - the order the retail comparison corpus's
    // headless channels seed with, so its image channel frames the same
    // entry. The save's resume scene wins; `--scene` covers a file without.
    let resumed = match debug_seeds.resume_save.as_deref() {
        None => None,
        Some(path) => {
            let bytes = std::fs::read(path)
                .with_context(|| format!("--resume-save: read {}", path.display()))?;
            let (sf, resume) = legaia_save::SaveFile::parse_with_resume(&bytes)
                .with_context(|| format!("--resume-save: parse {}", path.display()))?;
            let target = if resume.scene.is_empty() {
                scene.to_string()
            } else {
                resume.scene.clone()
            };
            let landing = session.resume_save(sf, &target, field_live_opts);
            log::info!(
                "play-window: --resume-save landed {} ({:?})",
                landing.kind(),
                landing.scene()
            );
            Some(session.host.world.mode)
        }
    };
    let world_map = match resumed {
        Some(mode) => mode == legaia_engine_mode::mode::SceneMode::WorldMap,
        None => world_map,
    };
    // A resumed overworld leaves the developer band off: a Load is not a
    // picker entry (`World::arm_picker_world_map_debug`).
    if world_map && resumed.is_none() {
        // Load the scene's resources, route its region-keyed encounter table
        // onto the overworld, install the player, and enter world-map mode
        // (camera controller included). World::tick drives locomotion + the
        // per-tile encounter roll from the pad routed via world.set_pad.
        // A picked overworld seats the party where a town exit lands them
        // (`SceneHost::arm_picker_entry_seat`, the page picker's rule too).
        session.host.arm_picker_entry_seat(scene);
        match session.enter_world_map_live(scene, field_live_opts) {
            Ok(mode) => {
                log::info!("play-window: entered world-map scene '{scene}' (mode={mode:?})")
            }
            Err(e) => log::warn!("play-window: enter_world_map_live('{scene}') failed: {e:#}"),
        }
        // Start in walk mode so the d-pad walks the overworld player (and the
        // per-tile encounter roll fires). The top-view debug camera (orbit /
        // zoom / pan) stays reachable via the toggle combo - the engine's
        // picker rule, which the page's scene picker asks too.
        session.host.world.arm_picker_world_map_debug();
    }
    if !world_map && resumed.is_none() {
        // Free-roam story staging: the `--scene` direct entry is the native
        // picker - stage the scene at its canonical free-roam visit (entry
        // BGM pause dropped, story-twin event flags seeded). The boot-UI
        // NEW GAME handler clears the staging again via `begin_new_game`.
        // The engine's one picker rule (`World::stage_picker_entry`), which
        // the browser page's `enter_field` asks too.
        session.host.world.stage_picker_entry(scene, false);
        // ...and seats the party where the disc's doors land them in it
        // (`SceneHost::arm_picker_entry_seat`), as the page picker does.
        session.host.arm_picker_entry_seat(scene);
        // Drop into the live field scene (run record 0, install the encounter
        // table, arm the live loop). Shared with the v0.1 oracle + headless
        // drivers via `NativeSession::enter_field_live`.
        let opts = field_live_opts.clone();
        match session.enter_field_live(scene, &opts) {
            Ok(mode) => log::info!("play-window: entered field scene '{scene}' (mode={mode:?})"),
            Err(e) => log::warn!("play-window: enter_field_live('{scene}') failed: {e:#}"),
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn cmd_play_window_with_record(
    scene: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    enable_audio: bool,
    world_map: bool,
    str_file: Option<&Path>,
    boot_ui: bool,
    save_dir: &Path,
    card: Option<&Path>,
    cutscene_map_path: Option<&Path>,
    cheat_file: Option<&Path>,
    cheat_strict: bool,
    live_loop: bool,
    player_battle: bool,
    party: Option<&str>,
    vm_dialogue: bool,
    terrain_y: bool,
    edge_collision: bool,
    solid_npcs: bool,
    live_npcs: bool,
    damage_finish: bool,
    battle_bgm: Option<u16>,
    screenshot: Option<super::ScreenshotConfig>,
    seed_party: bool,
    battle: Option<&str>,
    fidelity: super::LaunchFidelity,
    debug_seeds: super::DebugSeeds,
    record_to: Option<RecordTarget>,
) -> Result<()> {
    // Resolve the cutscene map (explicit `--cutscene-map` override or the
    // heuristic default) and, when neither `--str-file` nor `--disc` was
    // given, the auto-resolved `op*` / `ed*` MV*.STR on disk. `cutscene_map`
    // is reused below for disc-mode STR lookup. Shared with `cmd_play`.
    let (cutscene_map, auto_str) = crate::host_setup::resolve_cutscene_map_and_str(
        cutscene_map_path,
        scene,
        extracted_root,
        str_file,
        disc,
    )?;
    // winit allows one event loop per process, so the movie and the scene
    // share this one: the movie runs on it on demand and hands it back, then
    // the scene window runs on it to the end.
    let mut event_loop = EventLoop::new().context("create event loop")?;
    // A capture driver reads this line as "past the X11 startup race": a
    // child that dies of SIGSEGV without it died inside `EventLoop::new`
    // (libXi's first-use race) and is retried once
    // (`legaia_parity::window_child`).
    log::info!("{}", legaia_parity::window_child::EVENT_LOOP_OPEN_MARKER);
    play_intro_movie(
        &mut event_loop,
        scene,
        disc,
        str_file,
        auto_str.as_deref(),
        &cutscene_map,
        screenshot.is_some(),
    )?;

    let mut app = assemble_play_window(PlayWindowLaunch {
        scene,
        extracted_root,
        disc,
        enable_audio,
        audio_detached: false,
        world_map,
        boot_ui,
        save_dir,
        card,
        cheat_file,
        cheat_strict,
        live_loop,
        player_battle,
        party,
        vm_dialogue,
        terrain_y,
        edge_collision,
        solid_npcs,
        live_npcs,
        damage_finish,
        battle_bgm,
        screenshot,
        seed_party,
        battle,
        fidelity,
        debug_seeds,
        record_to,
    })?;

    // A player's window reads controllers; a scripted capture's pad word
    // belongs to its script.
    // Its window chrome too: the start-up key hint is for a player, and a
    // capture must not draw it.
    // `--screenshot-chrome` keeps the hint in a capture on purpose.
    if app.screenshot.is_none() {
        app.gamepad = super::gamepad::GamepadHost::open();
    }
    if app.screenshot.as_ref().is_none_or(|s| s.chrome) {
        app.startup_hint.arm();
    }

    // On demand, not `run_app`: only `run_app_on_demand` clears the exit
    // request a phase-1 movie left on this loop. Through `run_app` the game
    // would see that stale exit and quit on its first iteration.
    {
        use winit::platform::run_on_demand::EventLoopExtRunOnDemand;
        event_loop
            .run_app_on_demand(&mut app)
            .context("event loop")?;
    }
    // After the event loop returns, flush any pending record log. The
    // Escape / CloseRequested handlers also flush proactively so a
    // mid-run crash still produces a partial replay file - the trailing
    // flush is the safety net.
    if let Some(log) = app.record_log.as_mut()
        && let Err(e) = log.flush()
    {
        log::error!("record: flush on exit failed: {e:#}");
    }
    Ok(())
}

/// Everything a launch decides before the window exists: the `play-window`
/// command-line surface minus the intro movie, which needs the event loop.
/// [`assemble_play_window`] turns it into the app the loop runs; the
/// headless driver (`window::headless`) builds one with the CLI defaults.
pub(super) struct PlayWindowLaunch<'a> {
    pub(super) scene: &'a str,
    pub(super) extracted_root: &'a Path,
    pub(super) disc: Option<&'a Path>,
    pub(super) enable_audio: bool,
    /// Open the audio director over a device-less output
    /// (`AudioOut::detached`) rather than the machine's device: the headless
    /// driver's audio, which routes every command and sounds nothing.
    pub(super) audio_detached: bool,
    pub(super) world_map: bool,
    pub(super) boot_ui: bool,
    pub(super) save_dir: &'a Path,
    pub(super) card: Option<&'a Path>,
    pub(super) cheat_file: Option<&'a Path>,
    pub(super) cheat_strict: bool,
    pub(super) live_loop: bool,
    pub(super) player_battle: bool,
    pub(super) party: Option<&'a str>,
    pub(super) vm_dialogue: bool,
    pub(super) terrain_y: bool,
    pub(super) edge_collision: bool,
    pub(super) solid_npcs: bool,
    pub(super) live_npcs: bool,
    pub(super) damage_finish: bool,
    pub(super) battle_bgm: Option<u16>,
    pub(super) screenshot: Option<super::ScreenshotConfig>,
    pub(super) seed_party: bool,
    pub(super) battle: Option<&'a str>,
    pub(super) fidelity: super::LaunchFidelity,
    pub(super) debug_seeds: super::DebugSeeds,
    pub(super) record_to: Option<RecordTarget>,
}

/// The launch stages after the intro movie: the session, its launch switches
/// and its scene, the debug seeds, the UI asset decode and the window state.
/// Creates no window and no renderer.
pub(super) fn assemble_play_window(launch: PlayWindowLaunch<'_>) -> Result<PlayWindowApp> {
    let PlayWindowLaunch {
        scene,
        extracted_root,
        disc,
        enable_audio,
        audio_detached,
        world_map,
        boot_ui,
        save_dir,
        card,
        cheat_file,
        cheat_strict,
        live_loop,
        player_battle,
        party,
        vm_dialogue,
        terrain_y,
        edge_collision,
        solid_npcs,
        live_npcs,
        damage_finish,
        battle_bgm,
        screenshot,
        seed_party,
        battle,
        fidelity,
        debug_seeds,
        record_to,
    } = launch;
    // The session, its launch switches and its scene.
    let mut session = match disc {
        Some(disc) if audio_detached => NativeSession::open_disc_detached_audio(
            disc,
            &legaia_engine_session::BootConfig {
                scene: scene.to_string(),
                enable_audio,
            },
        )?,
        _ => crate::host_setup::open_boot_session(scene, enable_audio, extracted_root, disc)?,
    };
    apply_launch_toggles(
        &mut session,
        LaunchToggles {
            entry_pulse: fidelity.entry_pulse,
            smart_monsters: fidelity.smart_monsters,
            tweaks: fidelity.tweaks,
            retail: fidelity.retail,
            vm_dialogue,
            terrain_y,
            edge_collision,
            solid_npcs,
            live_npcs,
            damage_finish,
        },
    );
    // Field-live arming, built once and reused: at startup for the direct path
    // and later by the boot-UI NEW GAME handler when it enters `opdeene`.
    let field_live_opts = legaia_engine_session::session::FieldLiveOpts {
        live_loop,
        player_battle,
        battle_bgm,
    };
    // `--set-flag`: raise the named system-flag bits BEFORE the scene is
    // entered, so a MAN entry script that branches on one takes the raised
    // arm. They are raised a second time after `--seed-party` below, because
    // `begin_new_game` resets the bank - dropping them there would make the
    // two flags silently exclusive.
    seed_debug_story_flags(&mut session, &debug_seeds);
    enter_launch_scene(
        &mut session,
        scene,
        world_map,
        &debug_seeds,
        &field_live_opts,
    )?;

    // Debug seeds, in dependency order: the roster, the flags it resets, the
    // seat, then what reads the settled party.
    if seed_party {
        seed_new_game_party(&mut session);
    }

    // `--seed-party` calls `begin_new_game`, which clears the flag bank, so
    // the `--set-flag` bits are raised again here. The dome reads them at
    // arena entry, long after this, so this is the pass that matters for a
    // seeded ban.
    seed_debug_story_flags(&mut session, &debug_seeds);
    apply_debug_seat_env(&mut session);
    apply_capture_palette_seeds(&mut session);
    teach_debug_spells(&mut session, &debug_seeds);
    apply_rng_seed_env(&mut session);
    apply_start_tile_env(&mut session);
    log_battle_ui_labels(&session);
    if player_battle {
        apply_battle_tutorial_env(&mut session);
    }
    seed_demo_battle(&mut session, player_battle);

    // Apply the cheat file (if any) to the live World before building
    // scene resources. The applier mutates `world.party.roster` /
    // `world.party.money` / `world.clock.play_time_seconds` etc. through the
    // ram_map registry.
    if let Some(path) = cheat_file {
        apply_cheat_file(&mut session.host.world, path, cheat_strict)?;
    }

    if let Some(spec) = party {
        install_party_spec(&mut session, spec)?;
    }
    apply_debug_cheats(&mut session, &debug_seeds);
    if let Some(spec) = battle {
        arm_launch_battle(&mut session, spec, boot_ui, party);
    }

    // The scene's render resources and the UI assets, decoded before the
    // renderer exists.
    let scene_res = build_window_scene_resources(&session)?;
    log::info!(
        "play-window: scene '{}', {} TMDs, {} TIMs",
        scene,
        scene_res.tmds.len(),
        scene_res.tim_count
    );

    let font = load_dialog_font(&session, extracted_root);

    let mapping = legaia_engine_system::input::Mapping::load_or_default(&std::path::PathBuf::from(
        "legaia-input.toml",
    ));
    let publisher_logos_atlas_data = build_publisher_logos_atlas(&session);
    let title_screen_atlas_data = build_title_screen_atlas(&session);
    let menu_glyph_atlas_data = build_menu_glyph_atlas(&session);
    let menu_glyph_tim_bytes = read_menu_glyph_tim(&session);
    let font = attach_menu_font_cells(&session, font, &menu_glyph_tim_bytes);
    let save_menu_atlas_data = build_save_menu_atlas(&session, &menu_glyph_tim_bytes);
    let menu_window_table = read_menu_window_table(&mut session);
    let mounted_card = mount_card(card);
    let port1_card = Some(open_port_one_card(save_dir, Some(&session.host.index)));
    let initial_boot_ui = initial_boot_ui(
        boot_ui,
        publisher_logos_atlas_data.is_some(),
        [port1_card.as_ref(), mounted_card.as_ref()],
        &session.mode_seat,
    );

    // The window, then the loop.
    let mut app = PlayWindowApp::assemble(WindowParts {
        session,
        font,
        scene_res,
        publisher_logos_atlas_data,
        title_screen_atlas_data,
        menu_glyph_atlas_data,
        save_menu_atlas_data,
        menu_window_table,
        mapping,
        initial_boot_ui,
        mounted_card,
        port1_card,
        screenshot,
        record_to,
        field_live_opts,
        save_dir,
        extracted_root,
        disc,
    });
    app.apply_launch_options(fidelity);
    // Which fidelity toggles this session runs off retail, once the launch
    // switches and the (possibly pinned) options have both landed: empty
    // under `--retail`.
    log::info!(
        "world: toggles off their retail value: {:?}",
        app.session.host.world.toggles.retail_deviations()
    );
    // The spell / seru display names off the boot SCUS, for the shop's Trade
    // row: read once here, as the browser play page reads them at disc load.
    app.ensure_seru_names();
    Ok(app)
}
