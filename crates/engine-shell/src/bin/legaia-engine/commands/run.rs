//! Real boot/session subcommands (`play`, `save`, `load`, `config`).
//!
//! Mechanical split from `commands.rs` (behavior-preserving).

use super::*;

/// Run retail's post-FMV control transfer for `fmv_id`.
///
/// The master dispatch `FUN_801CEA3C` ends every playback with a second
/// `switch` that writes the mode/scene globals: the next-scene CDNAME label
/// (`0x80084548`), the spawn/door word (`0x80084540`) and the next-game-mode
/// byte (`0x8007B83C`). Mid-game FMVs therefore do **not** resume the trigger
/// scene - `town01` triggers `fmv_id 1` and lands in `town0b`. The per-id map
/// is [`legaia_engine_core::cutscene::fmv_post_play_handoff`]; this is its
/// host, called once playback has finished and the world has left
/// [`SceneMode::Cutscene`](legaia_engine_core::world::SceneMode).
///
/// Only the two field arms are applied. NOT WIRED: the `CardInit` arm hands
/// off to game mode 22 (memory-card init) and `ModeZero` to game mode 0;
/// neither mode exists in the engine's `SceneMode` set, so there is nothing
/// to transfer control *to* and the arm is reported instead of run. Wiring
/// them needs those two modes on the world first.
fn apply_fmv_handoff(session: &mut legaia_engine_shell::BootSession, fmv_id: i16, tick_count: u64) {
    use legaia_engine_core::cutscene::{FmvHandoff, fmv_post_play_handoff};
    match fmv_post_play_handoff(fmv_id) {
        // The door word is retail's spawn/arrival selector. The engine seats a
        // cold field entry from its own resolver and carries no door-word ->
        // arrival-seat table, so it is reported rather than applied; the scene
        // hand-off itself is the part that changes where play continues.
        FmvHandoff::Field { scene, door } => match session.host.enter_field_scene(scene, 0) {
            Ok(()) => println!(
                "frame {tick_count}: fmv {fmv_id} hands off to field scene '{scene}' (door {door:#05x})"
            ),
            Err(e) => println!(
                "frame {tick_count}: fmv {fmv_id} hand-off to field scene '{scene}' failed: {e:#}"
            ),
        },
        FmvHandoff::ResumeField => println!(
            "frame {tick_count}: fmv {fmv_id} resumes the trigger scene (no scene name written)"
        ),
        FmvHandoff::CardInit { card_arg } => println!(
            "frame {tick_count}: fmv {fmv_id} hands off to game mode 22 (card init, arg {card_arg}) - mode not in the engine"
        ),
        FmvHandoff::ModeZero => println!(
            "frame {tick_count}: fmv {fmv_id} hands off to game mode 0 - mode not in the engine"
        ),
        FmvHandoff::None => println!(
            "frame {tick_count}: fmv {fmv_id} encodes no hand-off (dev slot); trigger scene continues"
        ),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_play(
    scene: &str,
    extracted_root: &std::path::Path,
    disc: Option<&std::path::Path>,
    frames: u64,
    enable_audio: bool,
    frame_ms: u64,
    str_file: Option<&Path>,
    cutscene_map_path: Option<&Path>,
) -> Result<()> {
    // Resolve the cutscene map (explicit `--cutscene-map` override or the
    // heuristic default) and, when neither `--str-file` nor `--disc` was
    // given, auto-resolve a `--scene op*` / `--scene edteien` request to its
    // paired FMV on disk. Shared with the windowed `play` path.
    let (_cutscene_map, auto_str) = crate::shared::resolve_cutscene_map_and_str(
        cutscene_map_path,
        scene,
        extracted_root,
        str_file,
        disc,
    )?;
    let resolved_str: Option<&Path> = str_file.or(auto_str.as_deref());

    // If a STR file was supplied (explicitly or auto-resolved), pre-decode
    // it headlessly and log the frame count. This is phase 1 for
    // `op*`/`ed*` in-engine cutscene scenes where an FMV precedes the
    // dialogue-overlay scene proper. The scene ticking (phase 2) runs
    // unconditionally after this block.
    if let Some(str_path) = resolved_str {
        let decoded = decode_str_frame_count(str_path)
            .with_context(|| format!("read STR file {}", str_path.display()))?;
        println!(
            "play: pre-decoded {} STR frames from {}",
            decoded,
            str_path.display()
        );
    }

    let mut session = crate::shared::open_boot_session(scene, enable_audio, extracted_root, disc)?;
    println!(
        "play: scene='{}' frames={} audio={} (entries={}, MES={}, VAB={}, SEQ={})",
        scene,
        if frames == 0 {
            "∞".into()
        } else {
            frames.to_string()
        },
        if session.audio.is_some() { "on" } else { "off" },
        session
            .host
            .scene
            .as_ref()
            .map(|s| s.entries.len())
            .unwrap_or(0),
        if session
            .host
            .assets
            .as_ref()
            .map(|a| a.mes.is_some())
            .unwrap_or(false)
        {
            "yes"
        } else {
            "no"
        },
        session
            .host
            .assets
            .as_ref()
            .map(|a| a.vab_entries.len())
            .unwrap_or(0),
        session
            .host
            .assets
            .as_ref()
            .map(|a| a.seq_entries.len() + a.seq_in_stream_entries.len())
            .unwrap_or(0),
    );

    let mut transitions = 0u64;
    let mut bgm_events = 0u64;
    let mut last_log = 0u64;
    let mut tick_count = 0u64;
    while frames == 0 || tick_count < frames {
        let event = session.tick()?;
        match event {
            SceneTickEvent::SceneEntered { name } => {
                transitions += 1;
                println!("frame {}: entered scene '{}'", tick_count, name);
            }
            SceneTickEvent::UnknownMapId { map_id } => {
                println!(
                    "frame {}: scene_transition({}) had no mapped scene",
                    tick_count, map_id
                );
            }
            SceneTickEvent::Stepped => {}
        }
        // Field -> Cutscene -> Field flow: when the field VM's FMV-trigger op
        // flips the world into the cutscene mode (game mode 26 / StrInit), play
        // the resolved `MV*.STR` here (headless MDEC decode) and tell the world
        // playback finished so the field resumes. The STR overlay owns the
        // frame in retail; the world keeps the field VM suspended until then.
        if let Some(fmv_id) = session.host.world.active_fmv() {
            match session.host.world.active_fmv_str_filename() {
                Some(rel) => {
                    let path = extracted_root.join(rel);
                    match decode_str_frame_count(&path) {
                        Ok(n) => println!(
                            "frame {tick_count}: cutscene fmv_id={fmv_id} {rel} ({n} frames)"
                        ),
                        Err(_) => println!(
                            "frame {tick_count}: cutscene fmv_id={fmv_id} {rel} (not extracted; skipped)"
                        ),
                    }
                }
                None => {
                    println!("frame {tick_count}: cutscene fmv_id={fmv_id} (cut path; skipped)")
                }
            }
            session.host.world.finish_cutscene();
            apply_fmv_handoff(&mut session, fmv_id, tick_count);
        }
        if let Some(bgm) = session.bgm.as_ref()
            && bgm.last_started.is_some()
        {
            bgm_events = bgm_events.max(1);
        }
        if tick_count - last_log >= 60 {
            last_log = tick_count;
            log::info!(
                "frame {}: world.frame={}, transitions={}, bgm_started={}",
                tick_count,
                session.host.world.frame,
                transitions,
                bgm_events
            );
        }
        if frame_ms > 0 {
            std::thread::sleep(Duration::from_millis(frame_ms));
        }
        tick_count += 1;
    }
    println!(
        "exit: ticked {} frames, world.frame={}, transitions={}",
        tick_count, session.host.world.frame, transitions
    );
    Ok(())
}

pub(crate) fn cmd_save(
    extracted_root: &std::path::Path,
    disc: Option<&std::path::Path>,
    save_dir: &std::path::Path,
    slot: u8,
    party_size: usize,
) -> Result<()> {
    use legaia_engine_core::menu_runtime::MenuRuntime;
    use legaia_engine_core::world::World;
    use legaia_save::{CharacterRecord, Party};

    let _ = (extracted_root, disc);
    let mut world = World::default();
    let members = (0..party_size).map(|_| CharacterRecord::zeroed()).collect();
    world.load_party(Party { members });
    world.story_flags = 0;
    world.money = 0;
    let runtime = MenuRuntime::new(save_dir.to_path_buf());
    let path = runtime.save_to_slot(&mut world, slot)?;
    let sf = world.save_full();
    println!(
        "saved slot {} to {} (party={}, story_flags={:#010X}, money={}, inventory={})",
        slot,
        path.display(),
        sf.party.members.len(),
        sf.ext.story_flags,
        sf.ext.money,
        sf.ext.inventory.len()
    );
    Ok(())
}

pub(crate) fn cmd_load(save_dir: &std::path::Path, slot: u8) -> Result<()> {
    use legaia_engine_core::menu_runtime::MenuRuntime;
    use legaia_engine_core::world::World;

    let runtime = MenuRuntime::new(save_dir.to_path_buf());
    let mut world = World::default();
    let path = runtime.load_from_slot(&mut world, slot)?;
    println!(
        "loaded slot {} from {} (party={}, story_flags={:#010X}, money={}, inventory={}, actors={})",
        slot,
        path.display(),
        world.roster.members.len(),
        world.story_flags,
        world.money,
        world.inventory.len(),
        world.actors.iter().filter(|a| a.active).count()
    );
    Ok(())
}

pub(crate) fn cmd_config(cmd: ConfigCmd) -> Result<()> {
    use legaia_engine_core::input::Mapping;
    match cmd {
        ConfigCmd::Show { config_file } => {
            let mapping = Mapping::load_or_default(&config_file);
            let mut pairs: Vec<_> = mapping.bindings.iter().collect();
            pairs.sort_by_key(|(k, _)| k.as_str());
            println!("input mapping ({})", config_file.display());
            for (key, btn) in &pairs {
                println!("  {key:<12} → {btn}");
            }
        }
        ConfigCmd::Set {
            binding,
            config_file,
        } => {
            let Some((key, btn)) = binding.split_once('=') else {
                anyhow::bail!("--binding must be KEY=BUTTON (e.g. Z=Cross)");
            };
            let key = key.trim().to_string();
            let btn = btn.trim().to_string();
            // Validate that the button name is known.
            if legaia_engine_core::input::PadButton::from_name(&btn).is_none() {
                anyhow::bail!(
                    "unknown pad button '{}'; valid names: Select L3 R3 Start Up Right Down Left L2 R2 L1 R1 Triangle Circle Cross Square",
                    btn
                );
            }
            let mut mapping = Mapping::load_or_default(&config_file);
            mapping.bindings.insert(key.clone(), btn.clone());
            mapping.save(&config_file)?;
            println!("binding saved: {key} → {btn} ({})", config_file.display());
        }
        ConfigCmd::DumpCutsceneMap { out } => {
            let map = legaia_engine_core::scene::CutsceneMap::from_heuristic();
            let toml_doc = map.to_toml_string();
            if out.as_os_str() == "-" {
                print!("{toml_doc}");
            } else {
                std::fs::write(&out, &toml_doc)
                    .with_context(|| format!("write {}", out.display()))?;
                println!(
                    "wrote {} cutscene-map entry/entries → {}",
                    map.len(),
                    out.display()
                );
            }
        }
    }
    Ok(())
}
