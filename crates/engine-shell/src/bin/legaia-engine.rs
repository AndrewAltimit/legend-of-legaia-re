//! Top-level engine driver. The "single command" that turns extracted-disc
//! bytes into a runtime view of any CDNAME scene.
//!
//! Subcommands:
//!
//! - `info` - headless one-line summary of a scene's resolved asset chain.
//! - `list-scenes` - every CDNAME scene name with its PROT range.
//! - `play` - headless engine tick: world + camera + audio, no window.
//! - `play-window` - windowed engine: opens a wgpu surface, renders scene
//!   TMDs against the software PSX VRAM each frame. Input: arrows = D-pad,
//!   Z = Cross, Esc = quit.

#[path = "legaia-engine/cli.rs"]
mod cli;
#[path = "legaia-engine/commands.rs"]
mod commands;
#[path = "legaia-engine/shared.rs"]
mod shared;
#[path = "legaia-engine/window.rs"]
mod window;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Cmd};
use commands::{
    cmd_audio_trace, cmd_battle, cmd_chain_editor, cmd_clut_trace, cmd_config, cmd_encounter,
    cmd_equip, cmd_gte_replay, cmd_info, cmd_inventory, cmd_list_scenes, cmd_load, cmd_man_scripts,
    cmd_mode_trace, cmd_pcm_trace, cmd_play, cmd_replay, cmd_save, cmd_save_select, cmd_scenarios,
    cmd_seru_capture, cmd_target_pick, cmd_title, cmd_vram_oracle,
};
use std::path::Path;
use window::{cmd_play_str, cmd_play_window, cmd_record};

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Info {
            scene,
            extracted_root,
            disc,
            vram_png,
            vram_bin,
            runtime_vram,
            vram_diff_png,
            tmd_stats,
            no_targeted,
        } => cmd_info(
            &scene,
            &extracted_root,
            disc.as_deref(),
            vram_png.as_deref(),
            vram_bin.as_deref(),
            runtime_vram.as_deref(),
            vram_diff_png.as_deref(),
            tmd_stats,
            !no_targeted,
        ),
        Cmd::ListScenes {
            extracted_root,
            disc,
        } => cmd_list_scenes(&extracted_root, disc.as_deref()),
        Cmd::ClutTrace {
            scene,
            extracted_root,
            disc,
            runtime_vram,
            max_sources,
        } => cmd_clut_trace(
            &scene,
            &extracted_root,
            disc.as_deref(),
            runtime_vram.as_deref(),
            max_sources,
        ),
        Cmd::ManScripts {
            scene,
            extracted_root,
            disc,
            all,
            disasm_record,
            disasm_partition,
            dump_man,
            gflag_partition,
            narration,
            system_flag_census,
        } => cmd_man_scripts(
            &scene,
            &extracted_root,
            disc.as_deref(),
            all,
            disasm_record,
            disasm_partition,
            dump_man.as_deref(),
            gflag_partition,
            narration,
            system_flag_census,
        ),
        Cmd::VramOracle {
            scene,
            extracted_root,
            disc,
            runtime_vram,
            scenario,
            manifest,
            frames,
            strict,
            diff_png,
            tiles,
            rows_csv,
            clut_regions,
        } => cmd_vram_oracle(VramOracleArgs {
            scene: scene.as_deref(),
            extracted_root: &extracted_root,
            disc: disc.as_deref(),
            runtime_vram: runtime_vram.as_deref(),
            scenario: scenario.as_deref(),
            manifest: &manifest,
            frames,
            strict,
            diff_png: diff_png.as_deref(),
            tiles,
            rows_csv: rows_csv.as_deref(),
            clut_regions,
        }),
        Cmd::ModeTrace {
            scene,
            extracted_root,
            disc,
            scenario,
            manifest,
            frames,
            out,
            strict,
        } => cmd_mode_trace(ModeTraceArgs {
            scene: scene.as_deref(),
            extracted_root: &extracted_root,
            disc: disc.as_deref(),
            scenario: scenario.as_deref(),
            manifest: &manifest,
            frames,
            out: &out,
            strict,
        }),
        Cmd::AudioTrace {
            scene,
            extracted_root,
            disc,
            scenario,
            manifest,
            frames,
            bgm_id,
            out,
            retail_jsonl,
            strict,
        } => cmd_audio_trace(AudioTraceArgs {
            scene: scene.as_deref(),
            extracted_root: &extracted_root,
            disc: disc.as_deref(),
            scenario: scenario.as_deref(),
            manifest: &manifest,
            frames,
            bgm_id,
            out: &out,
            retail_jsonl: retail_jsonl.as_deref(),
            strict,
        }),
        Cmd::PcmTrace {
            scene,
            extracted_root,
            disc,
            scenario,
            manifest,
            frames,
            bgm_id,
            retail_save,
            engine_wav,
            retail_wav,
            strict,
        } => cmd_pcm_trace(PcmTraceArgs {
            scene: scene.as_deref(),
            extracted_root: &extracted_root,
            disc: disc.as_deref(),
            scenario: scenario.as_deref(),
            manifest: &manifest,
            frames,
            bgm_id,
            retail_save: retail_save.as_deref(),
            engine_wav: engine_wav.as_deref(),
            retail_wav: retail_wav.as_deref(),
            strict,
        }),
        Cmd::Replay { input, out, strict } => cmd_replay(&input, &out, strict),
        Cmd::Record {
            out,
            scene,
            extracted_root,
            disc,
            no_audio,
            world_map,
            save_dir,
            scenario,
            rng_seed,
        } => cmd_record(
            &out,
            &scene,
            &extracted_root,
            disc.as_deref(),
            !no_audio,
            world_map,
            &save_dir,
            scenario.as_deref(),
            rng_seed,
        ),
        Cmd::Play {
            scene,
            extracted_root,
            disc,
            frames,
            no_audio,
            frame_ms,
            str_file,
            cutscene_map,
        } => cmd_play(
            &scene,
            &extracted_root,
            disc.as_deref(),
            frames,
            !no_audio,
            frame_ms,
            str_file.as_deref(),
            cutscene_map.as_deref(),
        ),
        Cmd::PlayWindow {
            scene,
            extracted_root,
            disc,
            no_audio,
            world_map,
            str_file,
            boot_ui,
            save_dir,
            cutscene_map,
            cheat_file,
            cheat_strict,
            live_loop,
            player_battle,
            party,
            simple_dialogue,
            flat_y,
            edge_collision,
            solid_npcs,
            live_npcs,
            damage_finish,
            battle_bgm,
            screenshot,
            screenshot_tick,
            screenshot_every,
            screenshot_dir,
            screenshot_last_tick,
            pad_script,
            seed_party,
        } => cmd_play_window(
            &scene,
            &extracted_root,
            disc.as_deref(),
            !no_audio,
            world_map,
            str_file.as_deref(),
            boot_ui,
            &save_dir,
            cutscene_map.as_deref(),
            cheat_file.as_deref(),
            cheat_strict,
            live_loop,
            player_battle,
            party.as_deref(),
            !simple_dialogue,
            !flat_y,
            edge_collision,
            solid_npcs,
            live_npcs,
            damage_finish,
            battle_bgm,
            window::ScreenshotConfig::from_args(
                screenshot,
                screenshot_tick,
                screenshot_every,
                screenshot_dir,
                screenshot_last_tick,
                pad_script.as_deref(),
            )?,
            seed_party,
        ),
        Cmd::Save {
            extracted_root,
            disc,
            save_dir,
            slot,
            party_size,
        } => cmd_save(
            &extracted_root,
            disc.as_deref(),
            &save_dir,
            slot,
            party_size,
        ),
        Cmd::Load { save_dir, slot } => cmd_load(&save_dir, slot),
        Cmd::PlayStr {
            str_file,
            disc,
            width,
            height,
        } => cmd_play_str(&str_file, disc.as_deref(), width, height),
        Cmd::Config { cmd } => cmd_config(cmd),
        Cmd::Battle {
            monsters,
            monster_hp,
            max_ticks,
            script,
        } => cmd_battle(monsters, monster_hp, max_ticks, &script),
        Cmd::Inventory {
            item,
            party_size,
            script,
        } => cmd_inventory(item, party_size, &script),
        Cmd::Equip { slot, item } => cmd_equip(slot, item),
        Cmd::GteReplay { trace, verbose } => cmd_gte_replay(&trace, verbose),
        Cmd::Title {
            script,
            no_save,
            fade_frames,
        } => cmd_title(&script, no_save, fade_frames),
        Cmd::SaveSelect {
            mode,
            slots,
            script,
        } => cmd_save_select(&mode, &slots, &script),
        Cmd::Encounter { rate, steps, seed } => cmd_encounter(rate, steps, seed),
        Cmd::TargetPick {
            kind,
            actor,
            script,
        } => cmd_target_pick(&kind, actor, &script),
        Cmd::ChainEditor { char_slot, script } => cmd_chain_editor(char_slot, &script),
        Cmd::SeruCapture { seru, count, party } => cmd_seru_capture(seru, count, &party),
        Cmd::Scenarios {
            manifest,
            extracted_root,
            bless,
        } => cmd_scenarios(manifest.as_deref(), &extracted_root, bless),
    }
}

/// Side-by-side compare of engine VRAM (built via the scene's targeted
/// upload) against a runtime VRAM blob from a save state. Reports the
/// per-band overlap and per-tile (64x64) diff if `tiles` is set; writes
/// the colour-coded diff PNG when `diff_png` is provided.
#[allow(clippy::too_many_arguments)]
struct VramOracleArgs<'a> {
    scene: Option<&'a str>,
    extracted_root: &'a Path,
    disc: Option<&'a Path>,
    runtime_vram: Option<&'a Path>,
    scenario: Option<&'a str>,
    manifest: &'a Path,
    frames: u64,
    strict: bool,
    diff_png: Option<&'a Path>,
    tiles: bool,
    rows_csv: Option<&'a Path>,
    clut_regions: bool,
}

/// Phase-E3 mode-trace oracle - args struct for `cmd_mode_trace`.
struct ModeTraceArgs<'a> {
    scene: Option<&'a str>,
    extracted_root: &'a Path,
    disc: Option<&'a Path>,
    scenario: Option<&'a str>,
    manifest: &'a Path,
    frames: u64,
    out: &'a Path,
    strict: bool,
}

/// Audio-trace oracle - args struct for `cmd_audio_trace`.
struct AudioTraceArgs<'a> {
    scene: Option<&'a str>,
    extracted_root: &'a Path,
    disc: Option<&'a Path>,
    scenario: Option<&'a str>,
    manifest: &'a Path,
    frames: u64,
    bgm_id: Option<u16>,
    out: &'a Path,
    retail_jsonl: Option<&'a Path>,
    strict: bool,
}

struct PcmTraceArgs<'a> {
    scene: Option<&'a str>,
    extracted_root: &'a Path,
    disc: Option<&'a Path>,
    scenario: Option<&'a str>,
    manifest: &'a Path,
    frames: u64,
    bgm_id: Option<u16>,
    retail_save: Option<&'a Path>,
    engine_wav: Option<&'a Path>,
    retail_wav: Option<&'a Path>,
    strict: bool,
}

/// Decode a raw PSX STR file (2048-byte sectors) headlessly and return the
/// number of MDEC video frames that decode cleanly. Shared by the `play`
/// pre-decode pass and the in-flow cutscene driver.
fn decode_str_frame_count(str_path: &Path) -> Result<usize> {
    use legaia_mdec::{MdecDecoder, str_sector::StrFrameAssembler};
    let data = std::fs::read(str_path)?;
    let n_sectors = data.len() / 2048;
    let mut asm = StrFrameAssembler::new();
    let mut decoded = 0usize;
    for i in 0..n_sectors {
        let sector = &data[i * 2048..(i + 1) * 2048];
        if let Some((hdr, bs)) = asm.push_sector(sector)? {
            let dec = MdecDecoder::new(hdr.width as u32, hdr.height as u32);
            if dec.decode_frame(&bs).is_ok() {
                decoded += 1;
            }
        }
    }
    Ok(decoded)
}
