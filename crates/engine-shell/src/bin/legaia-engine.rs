//! Top-level engine driver. The "single command" that turns extracted-disc
//! bytes into a runtime view of any CDNAME scene.
//!
//! Today this is a headless dry-run: it opens the extracted PROT, loads a
//! scene, builds [`SceneResources`] (the runtime VRAM + parsed TMD pool),
//! and prints a one-line summary — proof that the field/town asset chain
//! end-to-end produces the right shape of state without any
//! `tim_scan/<entry>/` or `tmd_scan/<entry>/` filesystem intermediate.
//!
//! For an interactive demo (input + render frame loop) run
//! `asset-viewer field <scene>`. The two binaries share the same scene-host
//! plumbing; this one is the minimal "boot a scene from PROT bytes alone"
//! reference.
//!
//! Usage:
//!
//! ```bash
//! ./target/release/legaia-engine info --scene town01
//! ./target/release/legaia-engine info --scene cave01 --extracted-root path/to/extracted
//! ./target/release/legaia-engine list-scenes
//! ```

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_assets::SceneAssets;
use legaia_engine_core::scene_resources::SceneResources;
use legaia_prot::cdname;

#[derive(Parser, Debug)]
#[command(
    name = "legaia-engine",
    about = "Top-level driver for the Legaia clean-room engine. Boots a CDNAME scene from extracted PROT bytes."
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Build [`SceneResources`] for one scene and print a summary line. Use
    /// this to verify the asset chain produces the right state without
    /// firing up the windowed viewer.
    Info {
        /// CDNAME scene name (e.g. `town01`, `dolk`, `cave01`).
        #[arg(long)]
        scene: String,
        /// Extracted-root directory containing `PROT.DAT` + `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
    },
    /// List every distinct scene name the CDNAME map exposes, with the
    /// PROT entry range each one covers.
    ListScenes {
        /// Extracted-root directory containing `CDNAME.TXT`.
        #[arg(long, default_value = "extracted")]
        extracted_root: PathBuf,
    },
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Info {
            scene,
            extracted_root,
        } => cmd_info(&scene, &extracted_root),
        Cmd::ListScenes { extracted_root } => cmd_list_scenes(&extracted_root),
    }
}

fn cmd_info(scene_name: &str, extracted_root: &std::path::Path) -> Result<()> {
    let prot = extracted_root.join("PROT.DAT");
    let cdname_path = extracted_root.join("CDNAME.TXT");
    if !prot.exists() {
        anyhow::bail!("missing {} (run `legaia-extract` first)", prot.display());
    }
    if !cdname_path.exists() {
        anyhow::bail!(
            "missing {} (run `legaia-extract` first)",
            cdname_path.display()
        );
    }

    let index = ProtIndex::open_extracted(extracted_root)
        .with_context(|| format!("open ProtIndex at {}", extracted_root.display()))?;
    let scene =
        Scene::load(&index, scene_name).with_context(|| format!("load scene '{scene_name}'"))?;
    let assets = SceneAssets::build(&scene);
    let resources = SceneResources::build(&scene)?;

    println!("scene '{}'", scene.name);
    println!(
        "  CDNAME range:           PROT [{}..{})",
        scene.start, scene.end
    );
    println!("  entries swept:          {}", scene.entries.len());
    println!(
        "  TIMs uploaded to VRAM:  {} (parse failures: {})",
        resources.tim_count, resources.tim_parse_failures
    );
    println!("  TMDs parsed:            {}", resources.tmds.len());
    println!(
        "  MES container:          {}",
        if assets.mes.is_some() {
            "present"
        } else {
            "absent"
        }
    );
    println!(
        "  SEQ entries (raw):      {} (in stream wrappers: {})",
        assets.seq_entries.len(),
        assets.seq_in_stream_entries.len()
    );
    println!("  VAB entries:            {}", assets.vab_entries.len());
    println!("  Event-script records:   {}", assets.event_records.len());
    Ok(())
}

fn cmd_list_scenes(extracted_root: &std::path::Path) -> Result<()> {
    let cdname_path = extracted_root.join("CDNAME.TXT");
    if !cdname_path.exists() {
        anyhow::bail!(
            "missing {} (run `legaia-extract` first)",
            cdname_path.display()
        );
    }
    let map =
        cdname::parse(&cdname_path).with_context(|| format!("parse {}", cdname_path.display()))?;

    let mut names: Vec<String> = map.values().cloned().collect();
    names.sort();
    names.dedup();

    println!("{} distinct scene names:", names.len());
    for name in &names {
        if let Some((start, end)) = cdname::block_range_for_name(&map, name) {
            println!(
                "  {:<24} PROT [{}..{}) ({} entries)",
                name,
                start,
                end,
                end - start
            );
        }
    }
    Ok(())
}
