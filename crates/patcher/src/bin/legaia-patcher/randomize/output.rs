//! The tail of a run: diff the patched image into a PPF and write the patch, image, cue sheet and manifest.

use anyhow::{Context, Result, bail};

use legaia_disc_patch::ppf;

use crate::cli::RandomizeArgs;
use crate::disc_io::{cue_contents, note_overwrite, with_extension};

/// Diff `original` against `patched` and write what the run asked for. A
/// relayout grew the image, so that path writes the image and no PPF.
pub(super) fn write_outputs(
    args: &RandomizeArgs,
    seed: u64,
    original: &[u8],
    patched: &[u8],
    mut manifest: Vec<String>,
    relayout_sectors: u32,
) -> Result<()> {
    if relayout_sectors > 0 {
        // A relayout moved every sector after PROT.DAT: no PPF, image only.
        manifest.push(format!("relayout_sectors = {relayout_sectors}"));
        if args.dry_run {
            println!(
                "dry run: the image would grow by {relayout_sectors} sector(s); no files written"
            );
            return Ok(());
        }
        let out = args
            .output
            .as_ref()
            .context("--allow-relayout grew the image; --output is required")?;
        note_overwrite(out);
        std::fs::write(out, patched).with_context(|| format!("write {}", out.display()))?;
        println!(
            "patched image: {} (grew by {relayout_sectors} sector(s); contains Sony bytes - do not redistribute; no PPF written)",
            out.display()
        );
        let cue_path = out.with_extension("cue");
        let bin_name = out
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "disc.bin".to_string());
        std::fs::write(&cue_path, cue_contents(&bin_name))
            .with_context(|| format!("write {}", cue_path.display()))?;
        println!("cue sheet:     {}", cue_path.display());
        if let Some(mpath) = &args.manifest {
            let mut text = manifest.join("\n");
            text.push('\n');
            std::fs::write(mpath, text)
                .with_context(|| format!("write manifest {}", mpath.display()))?;
            println!("manifest: {}", mpath.display());
        }
        return Ok(());
    }
    if patched.len() != original.len() {
        bail!("patched image changed size - refusing to emit (all edits must be same-size)");
    }
    let runs = ppf::diff_runs(original, patched);
    let changed_bytes: usize = runs.iter().map(|r| r.bytes.len()).sum();
    manifest.push(format!("ppf_records = {}", runs.len()));
    manifest.push(format!("bytes_changed = {changed_bytes}"));

    if runs.is_empty() {
        println!("note: no bytes changed (nothing to randomize for these options)");
    }

    if args.dry_run {
        println!(
            "dry run: would write a {}-record PPF ({} bytes changed); no files written",
            runs.len(),
            changed_bytes
        );
        return Ok(());
    }

    let desc = format!("Legend of Legaia randomizer seed {seed}");
    let ppf_bytes = ppf::write_ppf3(&desc, &runs);
    let patch_path = args
        .patch
        .clone()
        .unwrap_or_else(|| with_extension(&args.input, "ppf"));
    note_overwrite(&patch_path);
    std::fs::write(&patch_path, &ppf_bytes)
        .with_context(|| format!("write patch {}", patch_path.display()))?;
    println!(
        "patch: {} ({} records, {} bytes changed)",
        patch_path.display(),
        runs.len(),
        changed_bytes
    );

    if let Some(out) = &args.output {
        note_overwrite(out);
        std::fs::write(out, patched).with_context(|| format!("write {}", out.display()))?;
        println!(
            "patched image: {} (contains Sony bytes - do not redistribute)",
            out.display()
        );
        // Emit a matching .cue next to the image so emulators that won't load a
        // bare BIN (e.g. mednafen rejects >64 MiB BIN) can open it directly.
        let cue_path = out.with_extension("cue");
        let bin_name = out
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "disc.bin".to_string());
        std::fs::write(&cue_path, cue_contents(&bin_name))
            .with_context(|| format!("write {}", cue_path.display()))?;
        println!("cue sheet:     {}", cue_path.display());
    }

    if let Some(mpath) = &args.manifest {
        let mut text = manifest.join("\n");
        text.push('\n');
        std::fs::write(mpath, text)
            .with_context(|| format!("write manifest {}", mpath.display()))?;
        println!("manifest: {}", mpath.display());
    }

    Ok(())
}
