//! Extracted from `window.rs` (mechanical split; behavior-preserving).
//!
//! The `play-window` engine-runner entry points (`cmd_play_window` /
//! `cmd_play_window_with_record`), the render-side `SceneResources`
//! builder, and the cheat-file / party-spec helpers they drive.

use super::*;

/// Parse a GameShark `.gs.txt` or Mednafen `.cht` cheat file and
/// apply every entry to `world` through the
/// [`legaia_engine_core::cheat_applier`] registry. Logs per-entry
/// status to stderr.
fn apply_cheat_file(
    world: &mut legaia_engine_core::world::World,
    path: &Path,
    strict: bool,
) -> Result<()> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut db = if path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("cht"))
        .unwrap_or(false)
    {
        legaia_cheats::parse_mednafen_cht(&text)?
    } else {
        legaia_cheats::parse_gs_text(&text)?
    };
    db.dedupe_identical();
    let opts = legaia_engine_core::cheat_applier::ApplyOptions {
        execute_conditionals: !strict,
        skip_unmapped: false,
    };
    let report = legaia_engine_core::cheat_applier::apply(world, &db, opts);
    eprintln!(
        "Cheat report ({} entries, {} writes; {} applied, {} unmapped, {} unknown):",
        report.per_entry.len(),
        report.total_writes,
        report.applied,
        report.unmapped,
        report.unknown_addresses
    );
    for entry in &report.per_entry {
        let total = entry.applied + entry.skipped;
        let tag = if entry.applied == total {
            "ok  "
        } else if entry.applied == 0 {
            "skip"
        } else {
            "part"
        };
        eprintln!(
            "  {tag}  {:.<60} {}/{} writes",
            entry.description, entry.applied, total
        );
    }
    Ok(())
}

/// Parse a `--party` composition spec: comma-separated character names
/// (case-insensitive `vahn`/`noa`/`gala`/`terra`) or 0-based roster
/// indices, in battle order.
fn parse_party_spec(spec: &str) -> Result<Vec<u8>> {
    spec.split(',')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| match t.to_ascii_lowercase().as_str() {
            "vahn" => Ok(0u8),
            "noa" => Ok(1),
            "gala" => Ok(2),
            "terra" => Ok(3),
            other => other.parse::<u8>().map_err(|_| {
                anyhow::anyhow!(
                    "unknown party member '{t}' (use vahn/noa/gala/terra or a roster index)"
                )
            }),
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_play_window(
    scene: &str,
    extracted_root: &Path,
    disc: Option<&Path>,
    enable_audio: bool,
    world_map: bool,
    str_file: Option<&Path>,
    boot_ui: bool,
    save_dir: &Path,
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
    dynamic_lighting: bool,
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
        dynamic_lighting,
        None,
    )
}

/// Build the play-window's render-side [`SceneResources`] for the host's
/// currently loaded scene: the shared blocks (`init_data` + `player_data`)
/// stay resident, the load kind mirrors the host's `enter_field_scene`
/// selection (WorldMap for `map\d\d`, Field otherwise), the boot-resident
/// system-UI bundle (raw PROT TOC entries 0/1 - the row-510/511 strip
/// CLUTs + the `(960,256)` menu-glyph atlas the town env meshes sample)
/// layers under the build via [`BuildOptions::system_ui`], and the field
/// character atlas is layered on. Used both for the initial scene at
/// window boot and to REBUILD the render state after a door transition
/// (`SceneTickEvent::SceneEntered`) swaps the host's scene.
pub(super) fn build_window_scene_resources(session: &BootSession) -> Result<SceneResources> {
    let s = session
        .host
        .scene
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no scene loaded on the host"))?;
    // Load the shared blocks (`init_data` + `player_data`) so the
    // player TMD + shared UI atlas stay resident across field
    // transitions, then build with the targeted VRAM-upload
    // heuristic. Without this every prim sampled non-uploaded
    // VRAM regions and the filter dropped 100% of the mesh.
    let shared_scenes = crate::shared::load_shared_scenes(&session.host.index, |name, e| {
        log::warn!("play-window: shared block '{name}' not loaded: {e:#}");
    });
    let shared_refs: Vec<&Scene> = shared_scenes.iter().collect();
    // Field-load model (matches retail FUN_8001F7C0 + the engine's
    // `enter_field_scene`): `SceneLoadKind::Field` skips the battle-
    // character `scene_tmd_stream` meshes, and the TMD scan now pulls
    // the town's environment geometry out of the scene_asset_table's
    // LZS-packed mesh pack (previously invisible to the raw scanner,
    // which left the field with a single stray battle mesh). Upload
    // every TIM, as retail's field loader DMAs the whole atlas - the
    // town meshes sample texture pages across all of VRAM, so a
    // render-targeted upload drops most of their prims.
    // World-map scenes (`map\d\d`) draw the kingdom-bundle slot-1
    // landmark pack, not the generic field sweep. Mirror the host's
    // `enter_field_scene` kind selection so the rendered meshes match the
    // gameplay-side resources (otherwise the window draws the Field-mode
    // 2-mesh fallback while the host loaded the full 40-TMD pack).
    let load_kind = if legaia_engine_core::scene::is_world_map_scene(&s.name) {
        SceneLoadKind::WorldMap
    } else {
        SceneLoadKind::Field
    };
    // Boot-resident system-UI bundle (raw PROT TOC entries 0/1): the
    // pre-pass layers it under the scene build - image pages at their
    // declared rects, CLUTs as `FUN_800198E0` flat strips (rows 510/511
    // etc). Soft-fails to None (the affected prims just drop).
    let system_ui = match session.host.index.system_ui_bundle() {
        Ok(b) => Some(b),
        Err(err) => {
            log::warn!("play-window: system-UI bundle parse skipped: {err:#}");
            None
        }
    };
    let (mut res, _stats) = SceneResources::build_targeted_with_options(
        s,
        &shared_refs,
        BuildOptions {
            kind: load_kind,
            upload_all_tims: true,
            system_ui: system_ui.as_deref(),
        },
    )?;
    // Field-character atlas upload (PROT 0874 §2, the `FUN_800198e0`
    // chain): entries 1/2/3 are the Vahn/Noa/Gala atlas pages whose
    // palettes live as **flat strips** on CLUT row 478. The generic TIM
    // scan uploads the pages but places these CLUTs as declared rects
    // (rows 478..481 col 0), so the meshes sample an unpopulated row and
    // the VRAM filter drops them - the invisible-player symptom. Retail
    // field load uploads the pack with strip semantics; replicate that.
    // (NOT the etim effect pool - uploading that battle-resident pool
    // into field VRAM clobbers pages the town meshes sample.)
    match session
        .host
        .index
        .entry_bytes(legaia_asset::field_char_textures::PROT_ENTRY_INDEX)
        .and_then(|b| legaia_asset::field_char_textures::parse(&b))
    {
        Ok(mut pack) => {
            // Entries 1/2/3 only (the character atlas pages). The pack's
            // other entries land on pages the town env meshes sample -
            // uploading them here drops ~26 scene meshes to the filter.
            pack.textures.retain(|t| (1..=3).contains(&t.index));
            pack.upload_to_vram(&mut res.vram, false);
            log::info!(
                "play-window: field char atlas uploaded ({} TIMs, strip CLUTs)",
                pack.textures.len()
            );
        }
        Err(err) => {
            log::warn!("play-window: field char atlas upload skipped: {err:#}");
        }
    }
    Ok(res)
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
    dynamic_lighting: bool,
    record_to: Option<RecordTarget>,
) -> Result<()> {
    // Resolve the cutscene map (explicit `--cutscene-map` override or the
    // heuristic default) and, when neither `--str-file` nor `--disc` was
    // given, the auto-resolved `op*` / `ed*` MV*.STR on disk. `cutscene_map`
    // is reused below for disc-mode STR lookup. Shared with `cmd_play`.
    let (cutscene_map, auto_str) = crate::shared::resolve_cutscene_map_and_str(
        cutscene_map_path,
        scene,
        extracted_root,
        str_file,
        disc,
    )?;
    let resolved_str: Option<&Path> = str_file.or(auto_str.as_deref());
    // Phase 1: if a STR file is provided (or auto-resolved), play the
    // video in a window first. The user closes (or ESC) the STR window,
    // then the scene window opens.
    //
    // When booting from a disc image we can resolve the scene's movie inside
    // the ISO and play it with its interleaved XA audio (read raw 2352-byte
    // sectors). Otherwise we fall back to the filesystem (video only).
    //
    // Screenshot mode skips the *auto-resolved* STR (an explicit `--str-file`
    // is still honoured): winit event loops cannot be recreated in-process, so
    // a phase-1 video window would make the scene window - and therefore the
    // screenshot - impossible. The real boot flow enters the prologue 3D scene
    // with no FMV anyway; the prepended STR is a preview-harness convenience.
    let skip_auto_str = screenshot.is_some() && str_file.is_none();
    if skip_auto_str {
        // No phase-1 window; fall through to the scene window.
    } else if let Some(str_path) = resolved_str {
        cmd_play_str(str_path, None, 640, 480)?;
    } else if let (Some(disc_path), None) = (disc, str_file) {
        // Disc mode, no explicit file: resolve the scene's MV*.STR via the
        // cutscene map / heuristic and play it from the disc with audio.
        if let Some(rel) = cutscene_map.resolve(scene) {
            let iso_path = Path::new(&rel);
            match cmd_play_str(iso_path, Some(disc_path), 640, 480) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("info: scene '{scene}' STR '{rel}' not played from disc ({e:#})")
                }
            }
        }
    }

    let mut session = crate::shared::open_boot_session(scene, enable_audio, extracted_root, disc)?;
    // Drive field dialogue through the inline-script field-VM runner so branch
    // handlers execute (flag-sets / scene-changes / GIVE_ITEM). On by default;
    // `--simple-dialogue` clears it to fall back to the plain typewriter panel.
    session.host.world.use_vm_dialogue = vm_dialogue;
    // Opt-in: snap the player's Y to the per-scene floor height each
    // locomotion step. Off by default → flat-Y behaviour preserved.
    session.host.world.follow_terrain_height = terrain_y;
    // Opt-in: retail's three-probe leading-edge wall footprint (the
    // `DAT_801f2214` standoff). Off by default → candidate-centre test.
    session.host.world.leading_edge_wall_probes = edge_collision;
    session.host.world.solid_field_npcs = solid_npcs;
    // Opt-in: walk field NPCs along their MAN-authored routes through the
    // motion VM. Off by default -> NPCs rest at their placement anchors.
    session.host.world.animate_field_npcs = live_npcs;
    // Opt-in: route live basic-attack damage through the retail damage
    // finisher (9999 cap + no-damage floor). Off by default → flat path.
    session.host.world.use_damage_finish = damage_finish;
    // Opt-in, NON-FAITHFUL QoL: redirect a monster's single-target attack to
    // the lowest-HP living party member (the faithful default is a uniform
    // random target). Enable with `LEGAIA_SMART_MONSTERS=1`. The RNG stream is
    // unchanged, so determinism within a run is preserved.
    session.host.world.smarter_monster_targeting =
        std::env::var_os("LEGAIA_SMART_MONSTERS").is_some();
    // Field-live arming, built once and reused: at startup for the direct path
    // and later by the boot-UI NEW GAME handler when it enters `opdeene`.
    let field_live_opts = legaia_engine_shell::boot::FieldLiveOpts {
        live_loop,
        player_battle,
        battle_bgm,
    };
    if world_map {
        // Load the scene's resources, route its region-keyed encounter table
        // onto the overworld, install the player, and enter world-map mode
        // (camera controller included). World::tick drives locomotion + the
        // per-tile encounter roll from the pad routed via world.set_pad.
        match session.enter_world_map_live(scene, &field_live_opts) {
            Ok(mode) => {
                log::info!("play-window: entered world-map scene '{scene}' (mode={mode:?})")
            }
            Err(e) => log::warn!("play-window: enter_world_map_live('{scene}') failed: {e:#}"),
        }
        // Start in walk mode so the d-pad walks the overworld player (and the
        // per-tile encounter roll fires). The top-view debug camera (orbit /
        // zoom / pan) stays reachable via the toggle combo (debug_enabled).
        if let Some(ctrl) = session.host.world.world_map_ctrl.as_mut() {
            ctrl.debug_enabled = true;
            ctrl.view_mode = 0;
        }
    }
    if !world_map {
        // Drop into the live field scene (run record 0, install the encounter
        // table, arm the live loop). Shared with the v0.1 oracle + headless
        // drivers via `BootSession::enter_field_live`.
        let opts = field_live_opts.clone();
        match session.enter_field_live(scene, &opts) {
            Ok(mode) => log::info!("play-window: entered field scene '{scene}' (mode={mode:?})"),
            Err(e) => log::warn!("play-window: enter_field_live('{scene}') failed: {e:#}"),
        }
    }

    // `--seed-party`: seed the New Game starting party (Vahn from the SCUS
    // template) so the pause menu's Status / party screens render real content
    // rather than an empty roster. Runs after field entry; `begin_new_game`
    // only resets story/money/inventory + sets mode=Field, leaving the loaded
    // scene intact.
    if seed_party {
        session.begin_new_game();
        let seeded = session.host.world.roster.members.len();
        log::info!("play-window: --seed-party seeded {seeded} roster member(s)");
    }

    // Debug start-position override: `LEGAIA_START_TILE=X,Z` seats the player
    // at that tile's centre after boot (tile*128+0x40, the op-0x3F entry-tile
    // mapping). Useful for parking on the overworld continent - a direct
    // `--world-map` boot has no door warp to seat the player, and the ocean
    // is unwalkable, so the (0,0) default strands the marker at sea. E.g.
    // Rim Elm's retail map01 arrival is `LEGAIA_START_TILE=96,25`.
    if let Ok(spec) = std::env::var("LEGAIA_START_TILE") {
        let parts: Vec<i32> = spec
            .split(',')
            .filter_map(|p| p.trim().parse().ok())
            .collect();
        if let [tx, tz] = parts[..] {
            let (cx, cz) = (tx.clamp(0, 255) as u8, tz.clamp(0, 255) as u8);
            session.host.world.seat_player_at_tile(cx, cz);
            log::info!("play-window: LEGAIA_START_TILE seated player at tile ({cx},{cz})");
        } else {
            log::warn!("play-window: LEGAIA_START_TILE ignored (want \"X,Z\"): {spec:?}");
        }
    }

    // Play-window demo seeding (NOT part of the shared field-live core): when
    // a player-driven battle is requested but the boot save carries no items /
    // saved chains, seed a couple so the Item / Arts submenus are exercisable
    // by hand. No-ops when the save already has inventory / chains.
    // Sparring tutorial: prime the in-battle "how to fight" prompt machine
    // (stage overlay 967) for the next battle. Retail arms it off the battle's
    // stage id, which only the Tetsu fight in `town01` carries; the engine has
    // no per-formation stage id yet, so the scene stands in. `LEGAIA_BATTLE_TUTORIAL`
    // forces / suppresses it (`1` / `0`) for hand-testing in any scene.
    if player_battle {
        let forced = std::env::var("LEGAIA_BATTLE_TUTORIAL").ok();
        let want = match forced.as_deref() {
            Some("0") => false,
            Some(_) => true,
            None => scene == "town01",
        };
        if want {
            let script = legaia_engine_core::battle_tutorial::BattleTutorialScript::from_prot(
                &session.host.index,
            );
            let n = script.len();
            session.host.world.prime_battle_tutorial(script);
            log::info!(
                "play-window: sparring tutorial primed for the next battle ({n} prompt(s) off the disc)"
            );
            // `LEGAIA_BATTLE_TUTORIAL=now` drops straight into the sparring
            // fight instead of waiting for a step-driven encounter, so the
            // prompt boxes are reachable in one run. Debug affordance only -
            // retail reaches this fight through the town01 story script.
            if forced.as_deref() == Some("now") {
                let world = &mut session.host.world;
                let pc = world.party_count.clamp(1, 3);
                world.enter_battle(pc, 1);
                // Seed only the combatant slots - the field scene's other
                // actors keep whatever the scene gave them.
                for a in world.actors.iter_mut().take(pc as usize + 1) {
                    if a.battle.max_hp == 0 {
                        a.battle.max_hp = 100;
                        a.battle.hp = 100;
                    }
                }
                log::info!("play-window: LEGAIA_BATTLE_TUTORIAL=now entered the sparring fight");
            }
        }
    }

    if player_battle {
        let world = &mut session.host.world;
        if world.inventory.is_empty() {
            world.inventory.insert(0x01, 5); // Healing Leaf
            world.inventory.insert(0x13, 3); // Bomb (offensive)
        }
        if world.saved_chains.is_empty() {
            use legaia_save::SavedChainRecord;
            for slot in 0u8..3 {
                world.saved_chains.push(SavedChainRecord {
                    char_slot: slot,
                    name: "Quick".into(),
                    sequence: vec![1, 2],
                });
                world.saved_chains.push(SavedChainRecord {
                    char_slot: slot,
                    name: "Combo".into(),
                    sequence: vec![1, 2, 3, 4],
                });
            }
            // Stage a demo art record per character so the "Combo" chain
            // (it ends in Up) resolves through the real art-power path -
            // two damage strikes that burn the target. "Quick" has no
            // matching record and falls back to the synthetic profile.
            use legaia_art::power::PowerByte;
            use legaia_art::queue::{ActionConstant, Command};
            use legaia_art::record::EnemyEffect;
            for character in legaia_art::Character::all() {
                world.set_art_record(
                    character,
                    ActionConstant::Art1B,
                    legaia_art::ArtRecord {
                        action: ActionConstant::Art1B,
                        commands: vec![Command::Up],
                        anim_index: 0,
                        anim_extra: vec![],
                        name: None,
                        power: vec![PowerByte::from_byte(0x18), PowerByte::from_byte(0x1D)],
                        dmg_timing: vec![],
                        effect_cues: Default::default(),
                        hit_cues: vec![],
                        identifier: 0,
                        anim_speed: 0,
                        enemy_effect: EnemyEffect::Toxic,
                        repeat_frames: Default::default(),
                        background: 0,
                        runtime_address: None,
                    },
                );
            }
        }
    }

    // Apply the cheat file (if any) to the live World before building
    // scene resources. The applier mutates `world.roster` /
    // `world.money` / `world.play_time_seconds` etc. through the
    // ram_map registry.
    if let Some(path) = cheat_file {
        apply_cheat_file(&mut session.host.world, path, cheat_strict)?;
    }

    // Install the requested present-party composition (after the roster +
    // cheats have settled, so the actor reseed reads final records). The
    // flag overrides whatever composition the boot save carried.
    if let Some(spec) = party {
        let slots = parse_party_spec(spec)?;
        let world = &mut session.host.world;
        world.set_active_party(slots.clone());
        if world.active_party.len() != slots.len() {
            log::warn!(
                "play-window: --party {spec}: kept the first {} of {slots:?} \
                 (3 on-screen positions)",
                world.active_party.len()
            );
        }
        log::info!(
            "play-window: present party = {:?} (roster slots, battle order)",
            world.active_party
        );
    }

    let scene_res = build_window_scene_resources(&session)?;
    log::info!(
        "play-window: scene '{}', {} TMDs, {} TIMs",
        scene,
        scene_res.tmds.len(),
        scene_res.tim_count
    );

    // Text metrics, best source first. `extracted/font/` (a `font-extract`
    // run) wins when present; otherwise the boot source's own disc bytes
    // (PROT.DAT font TIM + the SCUS width table) - both produce the same
    // atlas + advance table, so this is a source preference, not a fidelity
    // one. The fixed-width placeholder is the last resort: it advances every
    // glyph 9px where retail advances `widths[c] + 1` (4..10), which stretched
    // every string by roughly a third.
    let font = Font::load_from_extracted(extracted_root)
        .map_err(|e| log::debug!("extracted/font not loaded ({e:#}); trying the disc"))
        .ok()
        .or_else(|| session.dialog_font.clone())
        .unwrap_or_else(|| {
            log::warn!("dialog font unavailable; falling back to the placeholder font");
            Font::placeholder()
        });

    let mapping = legaia_engine_core::input::Mapping::load_or_default(&std::path::PathBuf::from(
        "legaia-input.toml",
    ));
    // Try to decode the publisher logos from PROT 0895 (init.pak) up
    // front. Falls back silently when the disc isn't loaded or the
    // entry doesn't parse - retail discs always have it.
    let publisher_logos_atlas_data = match session
        .host
        .index
        .entry_bytes(legaia_asset::init_pak::PROT_INDEX as u32)
    {
        Ok(b) => match legaia_engine_core::publisher_logos::build_atlas_from_init_pak(&b) {
            Ok(a) => {
                log::info!(
                    "play-window: publisher-logos atlas built ({}x{}, {} logos)",
                    a.width,
                    a.height,
                    a.rects.len()
                );
                Some(a)
            }
            Err(e) => {
                log::warn!("play-window: publisher-logos build failed: {e:#}");
                None
            }
        },
        Err(e) => {
            log::warn!("play-window: PROT 0895 read failed: {e:#}");
            None
        }
    };

    // Try to decode the title-screen TIM from PROT 0888 (`sound_data2`
    // per CDNAME, actually carries title art) up front. Falls back
    // silently when the disc isn't loaded or the entry doesn't parse -
    // retail discs always have it.
    let title_screen_atlas_data = match session
        .host
        .index
        .entry_bytes(legaia_asset::title_pak::PROT_INDEX_PRIMARY as u32)
    {
        Ok(b) => match legaia_engine_core::title_screen_atlas::build_atlas_from_prot_888(
            &b,
            legaia_asset::title_pak::TITLE_TIM_OFFSET,
        ) {
            Ok(a) => {
                log::info!(
                    "play-window: title-screen atlas built ({}x{})",
                    a.width,
                    a.height
                );
                Some(a)
            }
            Err(e) => {
                log::warn!("play-window: title-screen build failed: {e:#}");
                None
            }
        },
        Err(e) => {
            log::warn!("play-window: PROT 0888 read failed: {e:#}");
            None
        }
    };

    // Try to decode the menu-glyph atlas from the unindexed pre-init_data
    // gap in `PROT.DAT` (offset `0x11218`). Carries the small-caps font
    // retail samples for "NEW GAME" / "CONTINUE" menu rows. The
    // per-entry extractor never visits this gap, so we read PROT.DAT
    // raw bytes - see `legaia_asset::menu_glyph_atlas`.
    let menu_glyph_atlas_data = match session.host.index.prot_dat_raw_bytes(
        legaia_asset::menu_glyph_atlas::PROT_DAT_OFFSET,
        legaia_asset::menu_glyph_atlas::TIM_SIZE,
    ) {
        Ok(b) => match legaia_engine_core::menu_glyph_atlas::build_atlas_from_prot_dat_slice(&b) {
            Ok(a) => {
                log::info!(
                    "play-window: menu-glyph atlas built ({}x{})",
                    a.width,
                    a.height
                );
                Some(a)
            }
            Err(e) => {
                log::warn!("play-window: menu-glyph build failed: {e:#}");
                None
            }
        },
        Err(e) => {
            log::warn!("play-window: PROT.DAT raw read failed: {e:#}");
            None
        }
    };

    // Try to decode the save-menu UI atlas. Needs TWO disc sources:
    //   1. PROT 0899's extended footprint @ `OVERLAY_SAVE_MENU_TIM_OFFSET`
    //      carries the SLOT 1 / SLOT 2 pill sprites (CLUT 7).
    //   2. Raw PROT.DAT @ `OVERLAY_SYSTEM_UI_TIM_OFFSET = 0x018E0`
    //      carries the 9-slice panel chrome (CLUT row 2).
    // The atlas builder composites both into one 256x256 RGBA atlas;
    // see `crates/engine-core/src/save_menu_atlas.rs`. The 9-slice
    // tile geometry was pinned via `scripts/pcsx-redux/scan_panel_prims.py`
    // against sstate9's RAM dump - every primitive's source u/v + CLUT
    // is byte-pinned to the retail render.
    let save_menu_atlas_data = match (
        session
            .host
            .index
            .entry_bytes_extended(legaia_asset::title_pak::PROT_INDEX_OVERLAY as u32),
        // Pull a slice that covers BOTH the system-UI sheet (panel
        // chrome, cursor) AND the load-screen portrait + frame TIMs
        // (`OVERLAY_LOAD_PORTRAIT_TIM_OFFSET`..end of
        // `OVERLAY_LOAD_EMPTY_FRAME_TIM`). The slice starts at the
        // system-UI TIM header so existing offsets stay
        // slice-relative; `build_atlas` handles both shapes.
        {
            let base = legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_OFFSET;
            let end = legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
                + legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE;
            session
                .host
                .index
                .prot_dat_raw_bytes(base as u64, end - base)
        },
    ) {
        (Ok(pill_bytes), Ok(panel_bytes)) => {
            match legaia_engine_core::save_menu_atlas::build_atlas(&panel_bytes, &pill_bytes) {
                Ok(a) => {
                    log::info!(
                        "play-window: save-menu atlas built ({}x{}) - 9-slice from PROT.DAT[0x018E0] + pills from PROT 0899",
                        a.width,
                        a.height
                    );
                    Some(a)
                }
                Err(e) => {
                    log::warn!("play-window: save-menu build failed: {e:#}");
                    None
                }
            }
        }
        (Err(e), _) => {
            log::warn!("play-window: PROT 0899 read failed: {e:#}");
            None
        }
        (_, Err(e)) => {
            log::warn!("play-window: PROT.DAT raw read failed: {e:#}");
            None
        }
    };

    // Parse the menu overlay's window-descriptor table (PROT 0899
    // @0x15F24): the retail window rects behind every pause-menu screen.
    // Falls back to the pinned mirror consts when unavailable.
    let menu_window_table = session
        .host
        .index
        // The table sits at file 0x15F24, past the entry's TOC size - read
        // the extended footprint (the same read the save-menu pill TIM uses).
        .entry_bytes_extended(legaia_asset::menu_windows::MENU_OVERLAY_PROT_INDEX as u32)
        .ok()
        .and_then(|b| {
            // The same overlay carries the Arrange display-order table
            // (FUN_801D64A8): install it so the Items screen's Arrange
            // command sorts by the retail rank rather than id order.
            session.host.world.install_menu_overlay_tables(&b);
            match legaia_asset::menu_windows::parse(&b) {
                Ok(t) => Some(t),
                Err(e) => {
                    log::warn!("play-window: menu window table parse failed: {e:#}");
                    None
                }
            }
        });

    let initial_boot_ui = if boot_ui {
        if publisher_logos_atlas_data.is_some() {
            BootUiState::PublisherLogos(
                legaia_engine_core::publisher_logos::PublisherLogosSession::new(),
            )
        } else {
            let snapshots = scan_save_dir(save_dir);
            let any_present = snapshots.iter().any(|s| s.present);
            if any_present {
                BootUiState::Title(legaia_engine_core::title::TitleSession::new())
            } else {
                BootUiState::Title(legaia_engine_core::title::TitleSession::without_save_data())
            }
        }
    } else {
        BootUiState::Inactive
    };
    let mut app = PlayWindowApp {
        session,
        font,
        scene_res: Some(scene_res),
        win: EngineWindow::new(),
        font_atlas: None,
        publisher_logos: None,
        pending_publisher_logos_atlas: publisher_logos_atlas_data,
        title_screen: None,
        pending_title_screen_atlas: title_screen_atlas_data,
        menu_glyphs: None,
        pending_menu_glyph_atlas: menu_glyph_atlas_data,
        save_menu: None,
        pending_save_menu_atlas: save_menu_atlas_data,
        menu_window_table,
        caption_atlas: None,
        uploaded_vram: None,
        meshes: Vec::new(),
        scene_tmd_data: Vec::new(),
        field_placement_draws: Vec::new(),
        field_posed_props: Vec::new(),
        field_posed_tmds: Vec::new(),
        field_stager_tmds: Vec::new(),
        color_meshes: Vec::new(),
        field_placement_color_draws: Vec::new(),
        field_terrain_draws: Vec::new(),
        field_terrain_color_draws: Vec::new(),
        world_map_terrain_draws: Vec::new(),
        ground_heightfield: None,
        // Headless capture harnesses can't press `C`; let them start on the
        // wide debug vantage via the env switch.
        field_debug_camera: std::env::var_os("LEGAIA_FIELD_DEBUG_CAM").is_some(),
        world_map_slot4_lines: None,
        ocean_anim: None,
        cpu_vram_base: None,
        battle_vram: None,
        battle_vram_generation: None,
        battle_tex_slots_used: 0,
        battle_faces: Vec::new(),
        face_tables: None,
        art_mouth_tables: None,
        face_tables_attempted: false,
        fishing_prize_venues: None,
        summon_actor_slot: None,
        battle_stage_mesh: None,
        battle_ground_mesh: None,
        prev_scene_mode: None,
        monster_archive: None,
        battle_mesh_base: 0,
        scene_aabb: ([f32::NEG_INFINITY; 3], [f32::INFINITY; 3]),
        pad: 0,
        mapping,
        menu_runtime: MenuRuntime::new(save_dir.to_path_buf()),
        prev_pad: 0,
        tick_no: 0,
        screenshot,
        sweep_next_tick: 0,
        battle_event_log: std::collections::VecDeque::new(),
        battle_hud: legaia_engine_core::battle_hud::BattleHud::new(),
        pending_dynamic_mesh_slots: Vec::new(),
        drained_spawn_slots: std::collections::HashSet::new(),
        tile_slots_queued: std::collections::HashSet::new(),
        player_color_draw: None,
        field_npc_draws: Vec::new(),
        npc_clip_players: std::collections::HashMap::new(),
        npc_anim_srcs: std::collections::HashMap::new(),
        npc_pose_cache: std::collections::HashMap::new(),
        npc_pose_verify: std::collections::HashMap::new(),
        npc_anim_bundles: (None, None),
        npc_bundle_special: std::collections::HashMap::new(),
        boot_ui: initial_boot_ui,
        save_dir: save_dir.to_path_buf(),
        options_state: legaia_engine_core::options::OptionsState::load_or_default(
            &std::path::PathBuf::from(OPTIONS_CONFIG_FILE),
        ),
        record_log: record_to.map(RecordLog::from_target),
        field_live_opts,
        // In-flow cutscene STR resolves from the extracted root (video only)
        // or, when booting from a disc image, straight from the ISO with its
        // interleaved XA audio. Exactly one of these is set.
        extracted_root: disc.map_or_else(|| Some(extracted_root.to_path_buf()), |_| None),
        disc_path: disc.map(|d| d.to_path_buf()),
        cutscene: None,
        cutscene_cam_interp: legaia_engine_render::window::CutsceneCameraInterp::new(),
        cutscene_cam_frames: 0,
        pending_camera_snaps: Vec::new(),
        active_dialog: None,
        seru_names: None,
        battle_camera: None,
        dynamic_lighting,
        orbit_drag_last_x: None,
        cursor_x: 0.0,
    };

    // Push the loaded options into their live consumers (audio downmix)
    // before the loop starts.
    app.apply_options_side_effects();

    // Camera framing + movement toggles from the persisted options file:
    // the distance preset (default `far` - a bit more on screen than
    // retail; `T` cycles) and the precise-movement toggle (`R`). The
    // compass bias tells the engine-core camera what fixed yaw this
    // window's follow camera renders at (compass sense = the negated PSX
    // render yaw), so `BootSession::tick`'s d-pad remap feed tracks the
    // on-screen view exactly - including after a left-mouse drag-orbit.
    app.session.camera.distance = app.options_state.camera_distance;
    app.session.camera.render_yaw_bias = -FIELD_FOLLOW_YAW_UNITS / 4096.0 * std::f32::consts::TAU;
    app.session.host.world.precise_movement = app.options_state.precise_movement;
    log::info!(
        "camera: distance = {} (T cycles); precise movement {} (R toggles); drag to orbit",
        app.options_state.camera_distance.label(),
        if app.options_state.precise_movement {
            "ON"
        } else {
            "off"
        }
    );

    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop.run_app(&mut app).context("event loop")?;
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
