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
    // The extension decides; a file with neither says which by its text -
    // the same sniff the play page's pasted code list goes through.
    use legaia_engine_core::cheat_applier::{CheatTextFormat, apply_text};
    let format = match path.extension().and_then(|s| s.to_str()) {
        Some(e) if e.eq_ignore_ascii_case("cht") => CheatTextFormat::MednafenCht,
        _ => CheatTextFormat::sniff(&text),
    };
    let report = apply_text(world, &text, format, strict)?;
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
    dynamic_lighting: Option<bool>,
    dyn_shadows: bool,
    entry_pulse: bool,
    occlusion_fade: bool,
    volumetric_fog: bool,
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
        dynamic_lighting,
        dyn_shadows,
        entry_pulse,
        occlusion_fade,
        volumetric_fog,
        debug_seeds,
        None,
    )
}

/// Raise every `--set-flag` index in the world's shared system-flag bank.
///
/// The bank is retail's `DAT_80085758`, the same one the field VM's op `0x07`
/// writes and `FUN_8003CE64` reads, so a raised bit is indistinguishable from
/// one a script raised. Called twice - once before scene entry and once after
/// the `--seed-party` reset - and idempotent, because setting a bit twice is
/// setting a bit.
fn seed_debug_story_flags(session: &mut BootSession, seeds: &super::DebugSeeds) {
    if seeds.story_flags.is_empty() {
        return;
    }
    for &flag in &seeds.story_flags {
        session.host.world.system_flag_set(flag);
    }
    log::info!(
        "play-window: --set-flag raised {} story flag(s): {:?}",
        seeds.story_flags.len(),
        seeds.story_flags
    );
}

/// A `--battle` operand: a scene MAN formation-row index, or "the first row
/// the scene registered that carries monsters".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BattleEntry {
    Row(u16),
    First,
}

impl BattleEntry {
    fn parse(spec: &str) -> Result<Self> {
        let t = spec.trim();
        if t.eq_ignore_ascii_case("first") {
            return Ok(Self::First);
        }
        t.parse::<u16>().map(Self::Row).map_err(|_| {
            anyhow::anyhow!("--battle wants a formation row index or `first`, got {spec:?}")
        })
    }

    /// Resolve against the world's registered formations.
    fn resolve(self, world: &legaia_engine_core::world::World) -> Option<u16> {
        match self {
            Self::Row(row) => Some(row),
            Self::First => world.first_rollable_formation_id(),
        }
    }
}

/// Arm the `--battle` fight on a world that has just entered its scene.
///
/// The battle is driven through `World::force_encounter`, which hands the
/// named row to the encounter session's transition state machine exactly as a
/// region roll does - so the intro, the BGM swap and the battle-load path are
/// the ordinary ones, and what the window shows is what an organic encounter
/// shows. Nothing here runs when `--battle` is absent.
fn arm_requested_battle(session: &mut BootSession, spec: &str) {
    let entry = match BattleEntry::parse(spec) {
        Ok(e) => e,
        Err(err) => {
            log::error!("play-window: {err:#}");
            return;
        }
    };
    let world = &mut session.host.world;
    let Some(row) = entry.resolve(world) else {
        log::error!(
            "play-window: --battle {spec} found no registered formation in '{}' (rows: {:?})",
            world.active_scene_label,
            world.registered_formation_ids()
        );
        return;
    };
    // The transition is drained by the live field tick, so the loop has to be
    // on for the armed fight to open at all. `--battle` is an explicit request
    // for a fight; honour it over `--no-live-loop`.
    if !world.toggles.live_gameplay_loop {
        log::info!("play-window: --battle turns the live loop on (it drains the transition)");
        world.toggles.live_gameplay_loop = true;
    }
    // A scripted carrier's fight is entered by its own record, and that
    // record can raise a one-shot system-flag arm on the way in - the Rim
    // Elm sparring record's `50 19` two ops before its `3E FF` battle-entry
    // op, which is retail's whole condition for the sparring tutorial.
    // `--battle` on that carrier's row replays the entry without running the
    // record, so the arm is replayed with it; every other row, and a scene
    // whose script never raises the flag, is untouched.
    if world.replay_scripted_battle_arm(row) {
        log::info!(
            "play-window: --battle {row} is a scripted carrier's fight - its record's \
             system-flag arm is replayed with the entry"
        );
    }
    // The same record's op-0x35 words pick the fight's music - a scripted
    // boss's event starts its theme and selects the battle sound set just
    // before the entry op - so `--battle` replays them as well.
    if world.replay_scripted_battle_score(row) {
        log::info!(
            "play-window: --battle {row} replays its record's BGM words (track start + sound set)"
        );
    }
    // `LEGAIA_BATTLE_STAGE=N[,K]` stamps the battle-stage variant the fight
    // is staged in (`_DAT_8007BD60 & 0x1F`) and, with `K`, battle init's
    // keep-object-1 byte (`_DAT_8007B64B`), for a fight entered without
    // standing on the region tile that names them - the retail comparison
    // corpus reads both off the capture.
    if let Ok(s) = std::env::var("LEGAIA_BATTLE_STAGE") {
        let mut it = s.trim().split(',');
        if let Some(v) = it.next().and_then(|v| v.trim().parse::<u8>().ok()) {
            world.seed_battle_stage_variant(v);
        }
        if let Some(k) = it.next().and_then(|k| k.trim().parse::<u8>().ok()) {
            world.seed_battle_backdrop_keep_object_1(k != 0);
        }
    }
    // `LEGAIA_BATTLE_FRAME_STEP=step,state` (the retail-compare image
    // channel): run the battle on frames of `step` vsyncs from the first tick
    // the action SM reaches `state` (`World::seed_battle_frame_step`).
    if let Some((step, state)) = std::env::var("LEGAIA_BATTLE_FRAME_STEP")
        .ok()
        .and_then(|s| {
            let (a, b) = s.split_once(',')?;
            Some((a.trim().parse::<u8>().ok()?, b.trim().parse::<u8>().ok()?))
        })
    {
        world.seed_battle_frame_step(step, state);
    }

    // `LEGAIA_BATTLE_INFLIGHT=caster,spell,target[;x:z,...]`: dispatch that
    // cast the moment the first command prompt opens - the retail comparison
    // corpus's replay of a capture taken mid-cast (`InflightCastSeed`), with
    // each battle slot's ground (`-` keeps the seat). A debug seam.
    if let Some(seed) = std::env::var("LEGAIA_BATTLE_INFLIGHT").ok().and_then(|s| {
        let (head, ground_s) = s.split_once(';').unwrap_or((s.as_str(), ""));
        let v: Vec<u8> = head
            .split(',')
            .filter_map(|p| p.trim().parse().ok())
            .collect();
        let mut ground = [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS];
        for (g, tok) in ground.iter_mut().zip(ground_s.split(',')) {
            *g = tok.split_once(':').and_then(|(x, z)| {
                Some([x.trim().parse::<i16>().ok()?, z.trim().parse::<i16>().ok()?])
            });
        }
        (v.len() == 3).then(|| legaia_engine_core::world::InflightCastSeed {
            caster: v[0],
            spell_id: v[1],
            target: v[2],
            ground,
        })
    }) {
        log::info!("play-window: LEGAIA_BATTLE_INFLIGHT seeds {seed:?} at the first prompt");
        world.battle.inflight_seed = Some(seed);
    }
    // `LEGAIA_BATTLE_MONSTER_CAST=seat,spell` (decimal or `0x..`): the
    // monster in engine battle `seat` casts `spell` on its next turn instead
    // of the AI's pick (`BattleState::forced_monster_cast`, the seam the
    // retail comparison corpus replays a mid-cast capture through). Pairs
    // with `--battle <ROW>` to watch one enemy special from a cold boot. A
    // debug seam.
    if let Some((seat, spell)) = std::env::var("LEGAIA_BATTLE_MONSTER_CAST")
        .ok()
        .and_then(|s| {
            let parse = |t: &str| {
                let t = t.trim();
                t.strip_prefix("0x")
                    .map_or_else(|| t.parse::<u8>().ok(), |h| u8::from_str_radix(h, 16).ok())
            };
            let (a, b) = s.split_once(',')?;
            Some((parse(a)?, parse(b)?))
        })
    {
        log::info!(
            "play-window: LEGAIA_BATTLE_MONSTER_CAST seeds seat {seat} -> spell {spell:#04x}"
        );
        world.battle.forced_monster_cast = Some((seat, spell));
    }
    // `LEGAIA_BATTLE_RNG_SEED=<u32>`: the world stream's state at the entry,
    // so the fight does not inherit however many field draws the boot took.
    // The retail comparison corpus pins it on both its sides
    // (`retail_compare_battle::BATTLE_RNG_SEEDS`).
    if let Some(seed) = std::env::var("LEGAIA_BATTLE_RNG_SEED")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
    {
        world.rng_state = seed;
        world.encounters.rng_hold = Some(seed);
    }
    if world.force_encounter(row) {
        log::info!(
            "play-window: --battle armed formation row {row} in '{}' - the fight opens through \
             the normal encounter transition",
            world.active_scene_label
        );
    } else {
        world.encounters.rng_hold = None;
    }
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
    let shared_scenes = crate::host_setup::load_shared_scenes(&session.host.index, |name, e| {
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
    // The rest of the section is layered *under* the build below, never
    // written over it.
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
    // The whole effect-texture pool of that section (`etim`: the
    // `(448, 0)` page, the `fb_y = 256` pages, CLUT strips on rows 473 /
    // 475 / 478) is resident in retail field and world-map VRAM, and it
    // is where the field fog sheets sample: texture page `0x27` is
    // `(448, 0)`, the wisps sit at rows `0x40..0x6F` of it, CLUT `0x7640`
    // is `(0, 473)`. Every PCSX-Redux field / world-map state in the
    // library holds those cells byte-exact. Retail loads the pool before
    // the scene, so a scene TIM on an overlapping rect wins (`dolk`'s
    // `(448, 0)` page keeps its own texels in retail) - the boot-resident
    // underlay order `SceneResources` uses for the system-UI bundle.
    // Without it the fog quads sampled all-zero texels and the shader
    // discarded every fragment. See `docs/subsystems/field-ambient-fx.md`.
    let mut effect_pool = legaia_tim::Vram::new();
    match legaia_engine_core::scene::upload_effect_textures_into_vram(
        &session.host.index,
        &mut effect_pool,
        true,
    ) {
        Ok(n) => {
            res.vram.underlay(&effect_pool);
            log::info!("play-window: effect-texture pool underlaid ({n} TIMs)");
        }
        Err(err) => {
            log::warn!("play-window: effect-texture pool underlay skipped: {err:#}");
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
    dynamic_lighting: Option<bool>,
    dyn_shadows: bool,
    entry_pulse: bool,
    occlusion_fade: bool,
    volumetric_fog: bool,
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
    let resolved_str: Option<&Path> = str_file.or(auto_str.as_deref());
    // Phase 1: if a STR file is provided (or auto-resolved), play the
    // video in a window first. The user closes (or ESC) the STR window,
    // then the scene window opens.
    //
    // When booting from a disc image we can resolve the scene's movie inside
    // the ISO and play it with its interleaved XA audio (read raw 2352-byte
    // sectors). Otherwise we fall back to the filesystem (video only).
    //
    // winit allows one event loop per process, so the movie and the scene
    // share this one: the movie runs on it on demand and hands it back, then
    // the scene window runs on it to the end.
    //
    // Screenshot mode skips the *auto-resolved* STR (an explicit `--str-file`
    // is still honoured) so a capture is not held behind a movie. The real
    // boot flow enters the prologue 3D scene with no FMV anyway; the
    // prepended STR is a preview-harness convenience.
    let mut event_loop = EventLoop::new().context("create event loop")?;
    let skip_auto_str = screenshot.is_some() && str_file.is_none();
    if skip_auto_str {
        // No phase-1 window; fall through to the scene window.
    } else if let Some(str_path) = resolved_str {
        play_str_in(&mut event_loop, str_path, None, 640, 480)?;
    } else if let (Some(disc_path), None) = (disc, str_file) {
        // Disc mode, no explicit file: resolve the scene's MV*.STR via the
        // cutscene map / heuristic and play it from the disc with audio.
        if let Some(rel) = cutscene_map.resolve(scene) {
            let iso_path = Path::new(&rel);
            match play_str_in(&mut event_loop, iso_path, Some(disc_path), 640, 480) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("info: scene '{scene}' STR '{rel}' not played from disc ({e:#})")
                }
            }
        }
    }

    let mut session =
        crate::host_setup::open_boot_session(scene, enable_audio, extracted_root, disc)?;
    // The window drains every per-tick world queue itself, right after each
    // session tick (`drain_and_log_battle_events` / `drain_and_route_field_events`).
    session.set_host_drains_queues(true);
    // Scene-entry VDF pulse (enhancement) gate - must land before the first
    // `enter_field_scene`, which is where the installer runs.
    session.host.world.toggles.entry_pulse_enabled = entry_pulse;
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
    // Opt-in, NON-FAITHFUL QoL: redirect a monster's single-target attack to
    // the lowest-HP living party member (the faithful default is a uniform
    // random target). Enable with `LEGAIA_SMART_MONSTERS=1`. The RNG stream is
    // unchanged, so determinism within a run is preserved.
    session.host.world.toggles.smarter_monster_targeting =
        std::env::var_os("LEGAIA_SMART_MONSTERS").is_some();
    // Field-live arming, built once and reused: at startup for the direct path
    // and later by the boot-UI NEW GAME handler when it enters `opdeene`.
    let field_live_opts = crate::boot::FieldLiveOpts {
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
    let world_map = world_map || legaia_engine_core::scene::is_world_map_scene(scene);
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
            let landing = session.resume_save(sf, &target, &field_live_opts);
            log::info!(
                "play-window: --resume-save landed {} ({:?})",
                landing.kind(),
                landing.scene()
            );
            Some(session.host.world.mode)
        }
    };
    // The overworld's per-placement entity markers are a port debug overlay
    // (`WorldToggles::overworld_marker_overlay`, off by default): retail
    // draws nothing over a town entrance.
    session.host.world.toggles.overworld_marker_overlay =
        std::env::var_os("LEGAIA_WORLD_MAP_MARKERS").is_some();
    let world_map = match resumed {
        Some(mode) => mode == legaia_engine_core::world::SceneMode::WorldMap,
        None => world_map,
    };
    // A resumed overworld leaves the developer band off: a Load is not a
    // picker entry (`World::arm_picker_world_map_debug`).
    if world_map && resumed.is_none() {
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
    //
    // A `--world-map` entry keeps its mode: `begin_new_game`'s `mode = Field`
    // is the New Game boot's field launch, and letting it stand turned the
    // overworld into a field scene - the field fog pool (the kingdom MAN's
    // fog bit) then drew its sheets across the terrain through the field
    // camera, and the terrain took the field draw path, neither of which the
    // world map runs.
    if seed_party {
        let entered = session.host.world.mode;
        session.begin_new_game();
        if entered == legaia_engine_core::world::SceneMode::WorldMap {
            session.host.world.mode = entered;
        }
        let seeded = session.host.world.party.roster.members.len();
        log::info!("play-window: --seed-party seeded {seeded} roster member(s)");
    }

    // `--seed-party` calls `begin_new_game`, which clears the flag bank, so
    // the `--set-flag` bits are raised again here. The dome reads them at
    // arena entry, long after this, so this is the pass that matters for a
    // seeded ban.
    seed_debug_story_flags(&mut session, &debug_seeds);

    // Debug seat: `LEGAIA_SEAT=X,Z` puts the player on raw world `(X, Z)`
    // (floor-sampled Y) and re-arms the zone camera's arrival snap - the
    // frame-pairing aid for comparing this host against a retail save state
    // at that state's own player position. The play page's twin is
    // `play_debug_seat`.
    if let Ok(seat) = std::env::var("LEGAIA_SEAT") {
        let xz: Vec<i16> = seat
            .split(',')
            .filter_map(|v| v.trim().parse::<i16>().ok())
            .collect();
        if let [x, z] = xz[..]
            && session.host.debug_seat_standing(x, z)
        {
            // `LEGAIA_SEAT_CAMERA_BLOCK` (the retail-compare image channel):
            // the snap composes from the retail state's own camera
            // parameter block instead of a tile re-query.
            match std::env::var("LEGAIA_SEAT_CAMERA_BLOCK")
                .ok()
                .and_then(|b| legaia_parity::retail_compare::camera_block_from_env(&b))
            {
                Some(block) => session.camera.zone.arm_arrival_over(block),
                None => session.camera.zone.arm_arrival(),
            }
            // `LEGAIA_SEAT_FOCUS=FX,FZ` (stored form, X / Z negated): a
            // retail state's focus pair when it is not on the player - the
            // snap lands it instead of pinning the player, and the follow
            // ease leaves it there until the player moves, as retail's does.
            if let Some([fx, fz]) = std::env::var("LEGAIA_SEAT_FOCUS").ok().and_then(|f| {
                let v: Vec<i32> = f
                    .split(',')
                    .filter_map(|v| v.trim().parse::<i32>().ok())
                    .collect();
                <[i32; 2]>::try_from(v).ok()
            }) {
                session.camera.zone.seat_focus_after_snap([fx, fz]);
            }
            // `LEGAIA_SEAT_HEADING=H` (retail space, `0` = -Z): stand the
            // seated player in the state's own heading, which is walk
            // history like the position - as the arrival facing too, so an
            // entry script's `4C 3A` hands over the same heading.
            if let Some(h) = std::env::var("LEGAIA_SEAT_HEADING")
                .ok()
                .and_then(|h| h.trim().parse::<i16>().ok())
            {
                session.host.world.locomotion.arrival_facing = h;
                session.host.world.apply_arrival_facing();
            }
            log::info!("play-window: LEGAIA_SEAT seated the player at ({x}, {z})");
        } else {
            log::warn!("play-window: LEGAIA_SEAT='{seat}' not applied (want X,Z and a player)");
        }
    }

    // `LEGAIA_SEAT_CLUT_FX` (the retail-compare image channel): a retail
    // state's live CLUT-cell cycler snapshots, written over the matching
    // cycled cells so the frame shows the captured palette phase
    // (`AmbientFxState::cell_fx_seed`).
    if let Ok(fx) = std::env::var("LEGAIA_SEAT_CLUT_FX") {
        session.host.world.ambient.cell_fx_seed =
            legaia_parity::retail_compare::cell_fx_from_env(&fx);
    }

    // `LEGAIA_SEAT_VRAM_RECTS=<file>` (the retail-compare image channel): a
    // retail state's scroller rects with their captured texels, written over
    // the engine's after every field VRAM pass
    // (`AmbientFxState::vram_rect_seed`).
    if let Some(bytes) =
        std::env::var_os("LEGAIA_SEAT_VRAM_RECTS").and_then(|p| std::fs::read(p).ok())
    {
        session.host.world.ambient.vram_rect_seed =
            legaia_parity::retail_compare::vram_rects_from_file(&bytes);
    }

    // Debug learn path: `--learn-spell 0x81` (repeatable) or the older
    // `LEGAIA_LEARN_SPELLS=0x81,0x9e` prepends those spell ids (level 1) onto
    // the lead character's record, so a seeded New Game party can reach the
    // Magic arm and cast - the cast-presentation screenshot harness. A
    // development aid for the parity sweeps, not a player surface.
    {
        let parse = |s: &str| {
            let s = s.trim();
            s.strip_prefix("0x")
                .or_else(|| s.strip_prefix("0X"))
                .map_or_else(|| s.parse::<u8>().ok(), |h| u8::from_str_radix(h, 16).ok())
        };
        let from_env: Vec<u8> = std::env::var("LEGAIA_LEARN_SPELLS")
            .ok()
            .map(|list| list.split(',').filter_map(parse).collect())
            .unwrap_or_default();
        let mut learned = 0usize;
        for id in debug_seeds.learn_spells.iter().copied().chain(from_env) {
            if let Some(lead) = session.host.world.party.roster.members.first_mut() {
                legaia_engine_core::magic_xp::learn_spell_prepend(lead, id);
                learned += 1;
            }
        }
        if learned > 0 {
            log::info!("play-window: taught the lead {learned} debug spell(s)");
        } else if !debug_seeds.learn_spells.is_empty() {
            log::warn!(
                "play-window: --learn-spell had no lead party member to teach \
                 (pass --seed-party, or --boot-ui and start a game, first)"
            );
        }
    }

    // Debug RNG seed override: `LEGAIA_RNG_SEED=<u32>` (decimal or `0x` hex)
    // replaces the world's boot seed, so a sweep can be re-rolled onto a
    // different deterministic stream - the way to reach an AI branch (a
    // monster cast) the default seed never picks. A development aid for the
    // parity sweeps, not a player surface.
    if let Ok(spec) = std::env::var("LEGAIA_RNG_SEED") {
        let spec = spec.trim();
        let seed = spec
            .strip_prefix("0x")
            .or_else(|| spec.strip_prefix("0X"))
            .map_or_else(
                || spec.parse::<u32>().ok(),
                |h| u32::from_str_radix(h, 16).ok(),
            );
        if let Some(seed) = seed {
            session.host.world.rng_state = seed;
            log::info!("play-window: LEGAIA_RNG_SEED reseeded the world RNG to {seed:#010x}");
        }
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
            session.host.world.seat_player_at_tile_rescued(cx, cz);
            log::info!("play-window: LEGAIA_START_TILE seated player at tile ({cx},{cz})");
        } else {
            log::warn!("play-window: LEGAIA_START_TILE ignored (want \"X,Z\"): {spec:?}");
        }
    }

    // Battle chip / banner labels off the user's own disc: the `Ambushed!` and
    // `surprised the enemy` lines, `Spirit`, `Escape` and the per-character
    // Ra-Seru magic-command name, both halves installed at boot (`boot.rs`,
    // the builder the browser page shares). Empty on a partial extraction, in
    // which case the port's own wording draws instead of nothing.
    //
    // The party cast trigger's per-spell anim-pair lists are installed by
    // `BootSession::open` for every session (`boot.rs`), auto-battles
    // included.
    {
        let n = session.host.world.battle.ui_strings.len();
        log::info!("play-window: battle UI labels read off the disc ({n} string(s))");
    }
    // Sparring tutorial: nothing to gate here.
    //
    // The prompt corpus (stage overlay 967) is installed by scene entry for
    // every host, and the arming is the disc's own one-shot system-flag test
    // consumed in `World::enter_battle` - the port of the entity SM's
    // battle-entry tail (`FUN_801DA51C`, `0x801DA698..0x801DA6B0`). The flag
    // is raised by town01's Tetsu record executing `50 19` two ops before its
    // battle-entry op, so the tutorial fires in the fight retail fires it in
    // and in no other, with no host cooperation at all.
    //
    // What used to be here - a `--player-battle` flag AND `scene == "town01"`
    // AND an env var - was a development shim standing in for that condition,
    // and it was one-host: nothing equivalent existed on the browser page, so
    // the page drew tutorial boxes it could never be given.
    //
    // `LEGAIA_BATTLE_TUTORIAL` survives as a pure debug affordance, and each
    // arm is a one-shot poke at the same flag rather than a mode:
    //   `=0`     drop an arm the boot scene's script already raised,
    //   `=1`     raise it, so the next battle in any scene is the tutorial,
    //   `=now`   raise it AND drop straight into a fight.
    // `=0` cannot stop a *later* script SET - suppressing that would mean
    // adding a knob retail has no equivalent of.
    if player_battle {
        match std::env::var("LEGAIA_BATTLE_TUTORIAL").ok().as_deref() {
            Some("0") => {
                // Drop the arm the field script may already have raised.
                session
                    .host
                    .world
                    .system_flag_clear(legaia_engine_core::battle_tutorial::TUTORIAL_ARM_FLAG);
                log::info!("play-window: LEGAIA_BATTLE_TUTORIAL=0 suppressed the sparring arm");
            }
            Some(forced) => {
                session
                    .host
                    .world
                    .system_flag_set(legaia_engine_core::battle_tutorial::TUTORIAL_ARM_FLAG);
                let n = session.host.world.battle.tutorial_script.len();
                log::info!(
                    "play-window: LEGAIA_BATTLE_TUTORIAL forced the sparring arm for the next \
                     battle ({n} prompt(s) off the disc)"
                );
                // `=now` drops straight into the sparring fight instead of
                // waiting for a step-driven encounter, so the prompt boxes are
                // reachable in one run. Retail reaches this fight through the
                // town01 story script.
                if forced == "now" {
                    let world = &mut session.host.world;
                    let pc = world.party.party_count.clamp(1, 3);
                    world.enter_battle(pc, 1);
                    // Seed only the combatant slots - the field scene's other
                    // actors keep whatever the scene gave them.
                    for a in world.actors.iter_mut().take(pc as usize + 1) {
                        if a.battle.max_hp == 0 {
                            a.battle.max_hp = 100;
                            a.battle.hp = 100;
                        }
                    }
                    // Tag the monster seat with an id. `enter_battle` only seats
                    // actors; the id is what a step-driven encounter would have
                    // written from its resolved formation, and the whole battle
                    // RENDER path keys on it - `battle_monster_slots()` is empty
                    // without it, so `enter_battle_render` returns before it
                    // builds the stage backdrop or the ground grid. Without this
                    // the forced fight showed the field scene behind its HUD.
                    // `LEGAIA_BATTLE_TUTORIAL_MONSTER` picks the id.
                    let mid: u16 = std::env::var("LEGAIA_BATTLE_TUTORIAL_MONSTER")
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(1);
                    if let Some(a) = world.actors.get_mut(pc as usize) {
                        a.battle_monster_id = Some(mid);
                    }
                    log::info!(
                        "play-window: LEGAIA_BATTLE_TUTORIAL=now entered the sparring fight \
                         (monster id {mid})"
                    );
                }
            }
            None => {}
        }
    }

    // Play-window demo seeding (NOT part of the shared field-live core): two
    // demo items, two saved chains per character and a one-command demo art
    // written over every character's `Art1B` record, so the Item / Arts
    // submenus are exercisable by hand on an empty boot save. Opt-in
    // (`LEGAIA_DEMO_BATTLE_SEED=1`): it used to ride the default-on
    // player-battle flag, so every native session that booted without a save
    // played with free items and a fabricated art the browser page never
    // had - the same engine, two different parties.
    let demo_seed = std::env::var("LEGAIA_DEMO_BATTLE_SEED").is_ok_and(|v| v == "1");
    if player_battle && demo_seed {
        log::info!("play-window: LEGAIA_DEMO_BATTLE_SEED=1 - seeding demo items / chains / art");
        let world = &mut session.host.world;
        if world.party.inventory.is_empty() {
            world.party.inventory.insert(0x01, 5); // Healing Leaf
            world.party.inventory.insert(0x13, 3); // Bomb (offensive)
        }
        if world.party.saved_chains.is_empty() {
            use legaia_save::SavedChainRecord;
            for slot in 0u8..3 {
                world.party.saved_chains.push(SavedChainRecord {
                    char_slot: slot,
                    name: "Quick".into(),
                    sequence: vec![1, 2],
                });
                world.party.saved_chains.push(SavedChainRecord {
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
    // scene resources. The applier mutates `world.party.roster` /
    // `world.party.money` / `world.clock.play_time_seconds` etc. through the
    // ram_map registry.
    if let Some(path) = cheat_file {
        apply_cheat_file(&mut session.host.world, path, cheat_strict)?;
    }

    // Install the requested present-party composition (after the roster +
    // cheats have settled, so the actor reseed reads final records). The
    // flag overrides whatever composition the boot save carried.
    if let Some(spec) = party {
        let slots = parse_party_spec(spec)?;
        // Seed the roster slots the spec names before installing the
        // composition. `--seed-party` seeds retail's New Game roster, which is
        // Vahn alone (correct for the early game), so a `--party vahn,noa,gala`
        // asked three battle ordinals to resolve to roster slots 1 and 2 that
        // did not exist: both HUD panels read `P2 0/0` / `P3 0/0` and neither
        // actor got a live HP / SPD mirror. Fill them from the same SCUS
        // template rows retail seeds those characters from when they join.
        // Records already carrying stats (a loaded save) are left alone.
        if let Some(starting) = session.starting_party.clone() {
            let seeded = session
                .host
                .world
                .seed_party_members(&starting, &slots.clone());
            if seeded > 0 {
                log::info!(
                    "play-window: --party seeded {seeded} roster slot(s) from the SCUS \
                     new-game template"
                );
            }
        } else {
            log::warn!(
                "play-window: --party cannot seed absent roster slots - the SCUS \
                 starting-party template was not readable from this boot source"
            );
        }
        let world = &mut session.host.world;
        world.set_active_party(slots.clone());
        if world.party.active_party.len() != slots.len() {
            log::warn!(
                "play-window: --party {spec}: kept the first {} of {slots:?} \
                 (3 on-screen positions)",
                world.party.active_party.len()
            );
        }
        // Fold the freshly-seeded records into the battle stat mirrors
        // (attack / defence split / SPD / AP base). `set_active_party` copies
        // HP / MP only; without this the new members fight at zero attack.
        world.seed_party_battle_stats();
        // The MP ceiling the battle HUD draws is the only one the world
        // carries and nothing else seeds it from a record.
        for member in 0..world.party.active_party.len() {
            let rslot = world.party_roster_slot(member);
            let mp_max = world
                .party
                .roster
                .members
                .get(rslot)
                .map(|r| r.hp_mp_sp().mp_max)
                .unwrap_or(0);
            world.set_character_max_mp(member as u8, mp_max);
        }
        log::info!(
            "play-window: present party = {:?} (roster slots, battle order)",
            world.party.active_party
        );
    }

    // `--cheat-*`: after the roster and the present party have settled, so
    // the level cheat grows the members that will actually fight.
    let templates = session
        .host
        .new_game_defaults
        .as_ref()
        .map(|d| d.party.clone());
    debug_seeds
        .cheats
        .apply(&mut session.host.world, templates.as_ref());

    // `--battle <ROW|first>`: arm a deterministic fight. Last of the world
    // setup, so the party / cheats / New Game reset have all settled and the
    // combatants entering the battle are the ones the run configured.
    if let Some(spec) = battle {
        if boot_ui {
            // The title / save-select flow enters its own scene afterwards and
            // would reset the session under the armed fight.
            log::warn!("play-window: --battle ignored under --boot-ui (the boot flow re-enters)");
        } else if session.host.world.mode == legaia_engine_core::world::SceneMode::Battle {
            log::warn!("play-window: --battle skipped - a battle is already open");
        } else {
            // `LEGAIA_BATTLE_SETTLE=N`: tick the landed field N frames with
            // no input before the fight is armed - the retail comparison
            // corpus's headless seed enters every battle from a field that
            // settled `retail_compare::SETTLE_TICKS` first, and seeds the
            // world stream after it. Armed at boot instead, the encounter
            // transition owned the very first tick, so the scene's entry
            // scripts (the overworld's ambient-particle gate among them)
            // never ran and the two sides entered the same fight on
            // different `rand()` streams. The ticks run here, before the
            // window loop, with the session draining its own queues and
            // running the fog pool's render step as the headless seed does.
            if let Some(n) = std::env::var("LEGAIA_BATTLE_SETTLE")
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
            {
                session.set_host_drains_queues(false);
                for _ in 0..n {
                    if let Err(e) = session.tick() {
                        log::error!("play-window: LEGAIA_BATTLE_SETTLE tick: {e:#}");
                        break;
                    }
                    session.fog_render_tick();
                }
                session.set_host_drains_queues(true);
                // The settle can run a script that re-seats the present
                // party (a scripted duel's entry); the headless seed installs
                // the fight's roster after its settle, so the child does too.
                if let Some(slots) = party.and_then(|p| parse_party_spec(p).ok())
                    && slots != session.host.world.party.active_party
                {
                    session.host.world.set_active_party(slots);
                }
            }
            arm_requested_battle(&mut session, spec);
        }
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
        // The `0xCE` escape sprites ride the atlas whichever source won.
        .map(|f| match session.escape_icons.as_ref() {
            Some(icons) => f.with_escape_icons(icons),
            None => f,
        })
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

    // The battle HUD's 8x12 numeral cells come off the same menu-glyph TIM
    // as the small-caps rows above, through its sub-palette 13. Baked into
    // the save-menu atlas (below) rather than sampled from the menu-glyph
    // atlas so the HUD's sprite list stays one texture.
    let menu_glyph_tim_bytes = session
        .host
        .index
        .prot_dat_raw_bytes(
            legaia_asset::menu_glyph_atlas::PROT_DAT_OFFSET,
            legaia_asset::menu_glyph_atlas::TIM_SIZE,
        )
        .ok();

    // The menus' bold fixed-width numerals and the list pager pieces ride
    // the font atlas as sprite cells, so every menu number draws off the
    // same texture as its labels (`save_menu_atlas::menu_font_cells`; the
    // browser page attaches the same cells).
    let font = match session.host.index.prot_dat_raw_bytes(
        legaia_engine_core::save_menu_atlas::SYSTEM_UI_CLUT_EXT_TIM_OFFSET as u64,
        legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
            + legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE
            - legaia_engine_core::save_menu_atlas::SYSTEM_UI_CLUT_EXT_TIM_OFFSET,
    ) {
        Ok(system_ui) => {
            font.with_sprite_cells(&legaia_engine_core::save_menu_atlas::menu_font_cells(
                &system_ui,
                menu_glyph_tim_bytes.as_deref(),
            ))
        }
        Err(_) => font,
    };

    // Try to decode the save-menu UI atlas. Needs TWO disc sources:
    //   1. PROT 0899's extended footprint @ `OVERLAY_SAVE_MENU_TIM_OFFSET`
    //      carries the SLOT 1 / SLOT 2 pill sprites (CLUT 7).
    //   2. Raw PROT.DAT @ `OVERLAY_SYSTEM_UI_TIM_OFFSET = 0x018E0`
    //      carries the 9-slice panel chrome (CLUT row 2).
    // Plus the optional menu-glyph TIM above for the HUD numerals.
    // The atlas builder composites both into one 256x256 RGBA atlas;
    // see `crates/engine-menus/src/save_menu_atlas.rs`. The 9-slice
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
            // Rooted one TIM earlier than the sheet, at the row-511 CLUT
            // extension: it carries sub-palettes 16..18, which three of the
            // nine status-element badges decode with. `build_atlas` splits
            // that leading TIM off and treats the remainder exactly as a
            // sheet-rooted slice.
            let base = legaia_engine_core::save_menu_atlas::SYSTEM_UI_CLUT_EXT_TIM_OFFSET;
            let end = legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_OFFSET
                + legaia_asset::title_pak::OVERLAY_LOAD_EMPTY_FRAME_TIM_SIZE;
            session
                .host
                .index
                .prot_dat_raw_bytes(base as u64, end - base)
        },
    ) {
        (Ok(pill_bytes), Ok(panel_bytes)) => {
            match legaia_engine_core::save_menu_atlas::build_atlas(
                &panel_bytes,
                &pill_bytes,
                menu_glyph_tim_bytes.as_deref(),
            ) {
                Ok(mut a) => {
                    // The red cross-out X lives on the battle effect page, not
                    // the system-UI sheet; bake it into the same atlas so it
                    // draws in the chip list (the browser page bakes it too).
                    if let Ok(flame) = session.host.index.entry_bytes_extended(
                        legaia_engine_core::save_menu_atlas::FLAME_ATLAS_PROT_ENTRY,
                    ) {
                        legaia_engine_core::save_menu_atlas::add_cross_out_mark(&mut a, &flame);
                    }
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
    // @0x15F20): the retail window rects behind every pause-menu screen.
    // Falls back to the pinned mirror consts when unavailable.
    let menu_window_table = session
        .host
        .index
        // The table sits at file 0x15F20, past the entry's TOC size - read
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

    // Cold boot with no publisher logos on the disc goes straight to the
    // title, so the Continue-enable scan happens here instead of in the
    // mode-table hand-off - and it has to scan BOTH ports, exactly as that
    // hand-off does. Scanning the save directory alone greys the row out for
    // a player whose only save is on the memory-card image they mounted,
    // which is the one thing `--card` exists for.
    // Port 2 of the save screen's rack, mounted once. A container the
    // detector does not recognise is a mistake worth naming, not an empty
    // port, so the mount failure logs instead of silently leaving `None`.
    let mounted_card = card.and_then(|p| match super::MountedCard::open(p) {
        Ok(c) => Some(c),
        Err(e) => {
            log::warn!("play-window: --card not mounted: {e:#}");
            None
        }
    });
    let initial_boot_ui = if boot_ui {
        if publisher_logos_atlas_data.is_some() {
            BootUiState::PublisherLogos(
                legaia_engine_core::publisher_logos::PublisherLogosSession::new(),
            )
        } else {
            BootUiState::Title(super::boot_cutscene::title_session(
                super::boot_cutscene::rack_has_save(save_dir, mounted_card.as_ref()),
                0,
            ))
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
        field_pack_mesh_idx: Vec::new(),
        field_morph_live: std::collections::HashMap::new(),
        color_meshes: Vec::new(),
        field_placement_color_draws: Vec::new(),
        field_placement_window_keys: Vec::new(),
        field_placement_records: Vec::new(),
        field_placement_color_records: Vec::new(),
        field_pack_meshes: Vec::new(),
        field_placement_stream_bound: Vec::new(),
        field_placement_color_stream_bound: Vec::new(),
        field_pack_color_meshes: Vec::new(),
        field_placement_color_window_keys: Vec::new(),
        field_placement_cell_keys: Vec::new(),
        field_placement_color_cell_keys: Vec::new(),
        field_terrain_draws: Vec::new(),
        field_lit: Default::default(),
        draw_census: std::env::var_os("LEGAIA_DIAG_DRAWS").map(|_| Default::default()),
        field_floor_wave: Default::default(),
        coplanar_env_offsets: std::collections::HashMap::new(),
        field_terrain_color_draws: Vec::new(),
        world_map_terrain_draws: Vec::new(),
        world_map_terrain_color_draws: Vec::new(),
        world_map_terrain_records: Vec::new(),
        world_map_terrain_color_records: Vec::new(),
        world_map_deco_start: (0, 0),
        ground_heightfield: None,
        ground_src: None,
        ground_crop: None,
        field_terrain_cell_keys: Vec::new(),
        field_terrain_color_cell_keys: Vec::new(),
        field_terrain_facing: Vec::new(),
        field_terrain_color_facing: Vec::new(),
        // Headless capture harnesses can't press `F3`; let them start on the
        // wide debug vantage via the env switch.
        field_debug_camera: std::env::var_os("LEGAIA_FIELD_DEBUG_CAM").is_some(),
        menu_from_title: false,
        world_map_slot4_lines: None,
        ocean_anim: None,
        cpu_vram_base: None,
        dance_vram_restore: None,
        battle_vram: None,
        battle_intro: None,
        battle_intro_vram: None,
        battle_vram_generation: None,
        battle_tex_slots_used: 0,
        battle_faces: Vec::new(),
        face_tables: None,
        art_mouth_tables: None,
        face_tables_attempted: false,
        dev_menu: None,
        dev_menu_draws: Vec::new(),
        dev_menu_records: false,
        fishing_banners: Default::default(),
        fishing_banner_draws: Vec::new(),
        baka_hud_widgets: None,
        baka_chrome_frame: Vec::new(),
        baka_surface: Default::default(),
        baka_gpu: None,
        muscle_surface: Default::default(),
        muscle_gpu: None,
        fishing_surface: Default::default(),
        fishing_gpu: None,
        slot_cabinet_assets: None,
        slot_gpu: None,
        slot_marquee_clock: Default::default(),
        slot_dots: Vec::new(),
        dance_venue_gpu: None,
        dance_venue_failed: None,
        dance_cast_surface: Default::default(),
        dance_cast_gpu: None,
        muscle_hub: None,
        summon_actor_slot: None,
        battle_stage_mesh: None,
        battle_stage_color_mesh: None,
        battle_stage_shell: None,
        battle_ground_mesh: None,
        battle_ground_cue_far: None,
        battle_ground_rgbc: legaia_engine_vm::battle_ground_grid::GRID_RGBC_SETTLED,
        battle_stage_outdoor: false,
        prev_scene_mode: None,
        monster_archive: None,
        battle_mesh_base: 0,
        battle_rest_vmesh: std::collections::HashMap::new(),
        battle_color_mesh_base: 0,
        scene_aabb: ([f32::NEG_INFINITY; 3], [f32::INFINITY; 3]),
        pad: 0,
        mapping,
        keys_down: std::collections::HashSet::new(),
        pending_key_name: None,
        menu_runtime: MenuRuntime::new(save_dir.to_path_buf()),
        prev_pad: 0,
        pad_taps: Default::default(),
        tick_no: 0,
        screenshot,
        sweep_next_tick: 0,
        battle_event_log: std::collections::VecDeque::new(),
        encounter_banner: None,
        battle_hud: legaia_engine_core::battle_hud::BattleHud::new(),
        pending_dynamic_mesh_slots: Vec::new(),
        drained_spawn_slots: std::collections::HashSet::new(),
        tile_slots_queued: std::collections::HashSet::new(),
        player_color_draw: None,
        field_npc_draws: Vec::new(),
        npc_clip_players: std::collections::HashMap::new(),
        npc_anim_srcs: std::collections::HashMap::new(),
        npc_rest_srcs: std::collections::HashMap::new(),
        npc_morph_static: std::collections::HashMap::new(),
        npc_pose_cache: std::collections::HashMap::new(),
        npc_pose_verify: std::collections::HashMap::new(),
        npc_anim_bundles: (None, None),
        npc_bundle_special: std::collections::HashMap::new(),
        boot_ui: initial_boot_ui,
        save_dir: save_dir.to_path_buf(),
        card: mounted_card,
        save_flow: legaia_engine_core::save_screen::SaveScreenFlow::new(),
        options_state: {
            let mut o = legaia_engine_core::options::OptionsState::load_or_default(
                &std::path::PathBuf::from(OPTIONS_CONFIG_FILE),
            );
            // `LEGAIA_BATTLE_CAMERA_OPTION=<0|1|2>`: the retail-compare image
            // child plays the capture's own Battle Camera word (`0x800846C0`).
            if let Some(v) = std::env::var("LEGAIA_BATTLE_CAMERA_OPTION")
                .ok()
                .and_then(|v| v.trim().parse::<u8>().ok())
            {
                o.battle_camera = legaia_engine_core::options::BattleCameraOpt::from_word(v);
            }
            o
        },
        record_log: record_to.map(RecordLog::from_target),
        field_live_opts,
        // In-flow cutscene STR resolves from the extracted root (video only)
        // or, when booting from a disc image, straight from the ISO with its
        // interleaved XA audio. Exactly one of these is set.
        extracted_root: disc.map_or_else(|| Some(extracted_root.to_path_buf()), |_| None),
        disc_path: disc.map(|d| d.to_path_buf()),
        cutscene: None,
        movie_score: legaia_engine_core::movie_audio::MovieScore::new(),
        cutscene_glide: legaia_engine_core::frame_step::CutsceneGlide::new(),
        sim_stepper: legaia_engine_core::frame_step::SimStepper::new(),
        active_dialog: None,
        seru_names: None,
        // Resolved against the persisted option right below, once the
        // options file has loaded.
        dynamic_lighting: false,
        dyn_shadows,
        occlusion_fade,
        field_occluders: Default::default(),
        occl_fade_strength: std::cell::Cell::new(Default::default()),
        scene_point_lights: Vec::new(),
        scene_prop_lights: Vec::new(),
        orbit_drag_last_x: None,
        orbit_drag_last_y: None,
        last_left_press: None,
        // `atan(0.85)`: the angle the window's long-standing eye-height
        // ratio encoded, so an untouched debug vantage is unchanged.
        debug_orbit_pitch: 0.85f32.atan(),
        debug_orbit_zoom: 1.0,
        cursor_x: 0.0,
        cursor_y: 0.0,
        field_party_hud: Default::default(),
        field_party_hud_scene: None,
        diag_rows: legaia_engine_render::diag_hud_enabled(),
    };

    // Retail-shaped equipment buy: this window draws the recipient picker
    // (window 36) and the two stat-compare windows (25 / 41) over the parked
    // buy list (`legaia_engine_screens`), so opt into the flow and install
    // the disc restrictions the buy-list kind dispatch reads. Same arming
    // the browser play page performs at `load_disc`; without the table the
    // route falls back to the quantity picker, so the opt-in is gated on it.
    if let Some(info) = app.session.equip_restrictions.clone() {
        app.menu_runtime.install_equip_info(info);
        app.menu_runtime.retail_equipment_buy = true;
    }

    // `--no-volumetric-fog` (and a recording, which stays on the faithful
    // render) lowers the volumetric ground-fog option for this session only:
    // the window never writes the override back unless `F9` is pressed.
    if !volumetric_fog {
        app.options_state.volumetric_fog = false;
    }
    // Push the loaded options into their live consumers (audio downmix)
    // before the loop starts.
    app.apply_options_side_effects();

    // Enhanced lighting: the persisted option (default on) unless the
    // command line forced it (`--dynamic-lighting` / `--no-dynamic-lighting`;
    // replays force it off).
    app.dynamic_lighting = dynamic_lighting.unwrap_or(app.options_state.enhanced_lighting);

    // Camera framing + movement toggles from the persisted options file:
    // the distance preset (default `far` - a bit more on screen than
    // retail; `T` cycles) and the precise-movement toggle (`R`). The
    // compass bias tells the engine-core camera what fixed yaw this
    // window's follow camera renders at (compass sense = the negated PSX
    // render yaw), so `BootSession::tick`'s d-pad remap feed tracks the
    // on-screen view exactly - including after a left-mouse drag-orbit.
    app.session.camera.distance = app.options_state.camera_distance;
    app.session.camera.render_yaw_bias =
        legaia_engine_core::camera_view::retail_field_render_yaw_bias();
    app.session.host.world.locomotion.precise_movement = app.options_state.precise_movement;
    // Field Move (pause menu Walk / Run, retail config word 0x800846CC). The
    // run BUTTON inverts this per frame - see `World::field_run_active` - and
    // is fed from the pad each tick in the event handler.
    app.session.host.world.locomotion.run_default =
        app.options_state.field_move == legaia_engine_core::options::FieldMoveOpt::Run;
    log::info!(
        "camera: distance = {} (T cycles); precise movement {} (R toggles); drag to orbit",
        app.options_state.camera_distance.label(),
        if app.options_state.precise_movement {
            "ON"
        } else {
            "off"
        }
    );
    log::info!(
        "field move: {} by default (hold the run button to invert)",
        if app.session.host.world.locomotion.run_default {
            "RUN"
        } else {
            "walk"
        }
    );

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
