//! `monster-block` - dump / re-pack one monster's decoded `battle_data`
//! block (PROT entry 867) so a modder can hex-edit stats, element, or the
//! name string without doing any slot-offset or LZS math by hand.
//!
//! Dump reads the slot off the disc image and LZS-decodes it; write re-packs
//! an edited block with [`legaia_asset::monster_archive::encode_slot`] and
//! patches it back through [`DiscPatcher::patch_monster_slot`] - a same-size
//! in-place sector write with the EDC/ECC re-encoded, exactly like the
//! randomizer's own monster edits.

use std::path::Path;

use anyhow::{Context, Result, bail};

use legaia_asset::monster_archive;
use legaia_rando::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};
use legaia_rando::ppf;

use crate::util::{cue_contents, load_image, note_overwrite};

pub(crate) fn cmd_monster_block(
    input: &Path,
    id: u16,
    dump: Option<&Path>,
    write: Option<&Path>,
    output: Option<&Path>,
    patch: Option<&Path>,
) -> Result<()> {
    if dump.is_none() && write.is_none() {
        bail!("pass --dump <block.bin> to extract and/or --write <block.bin> to re-pack");
    }
    let original = load_image(input)?;
    let mut patcher = DiscPatcher::open(original.clone()).context("parse disc image")?;

    if let Some(dump_path) = dump {
        // The extended footprint, because a retail LZS stream may spill past
        // its own slot; the decoder stops at the block's declared size.
        let entry = patcher.read_entry_footprint(MONSTER_ARCHIVE_ENTRY)?;
        let Some(block) = monster_archive::decode_block(&entry, id)? else {
            bail!("monster id {id}: empty / filler slot (monster-stats lists the populated ids)");
        };
        let name = monster_archive::record(&entry, id)?
            .map(|r| r.name)
            .unwrap_or_else(|| "?".into());
        std::fs::write(dump_path, &block)
            .with_context(|| format!("write block {}", dump_path.display()))?;
        println!(
            "wrote {} ({} bytes) - id {id} {name}; stat record head at +0x00, element byte at +0x1D",
            dump_path.display(),
            block.len(),
        );
    }

    if let Some(block_path) = write {
        if output.is_none() && patch.is_none() {
            bail!("--write needs --output <patched.bin> and/or --patch <out.ppf>");
        }
        let block = std::fs::read(block_path)
            .with_context(|| format!("read block {}", block_path.display()))?;
        let slot = monster_archive::encode_slot(&block)?;
        patcher.patch_monster_slot(id, &slot)?;
        let patched = patcher.into_image();
        if let Some(ppf_path) = patch {
            let runs = ppf::diff_runs(&original, &patched);
            let desc = format!("Legaia monster block edit (id {id})");
            note_overwrite(ppf_path);
            std::fs::write(ppf_path, ppf::write_ppf3(&desc, &runs))
                .with_context(|| format!("write PPF {}", ppf_path.display()))?;
            println!("wrote {} ({} change runs)", ppf_path.display(), runs.len());
        }
        if let Some(out) = output {
            note_overwrite(out);
            std::fs::write(out, &patched)
                .with_context(|| format!("write patched image {}", out.display()))?;
            let cue = out.with_extension("cue");
            let bin_name = out
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "patched.bin".to_string());
            std::fs::write(&cue, cue_contents(&bin_name))
                .with_context(|| format!("write cue {}", cue.display()))?;
            println!("wrote {} (+ {})", out.display(), cue.display());
            // Same sanity check the randomizer runs: the patched image still
            // parses and the edited record still decodes.
            let check = DiscPatcher::open(patched).context("re-parse patched image")?;
            let entry = check.read_entry_footprint(MONSTER_ARCHIVE_ENTRY)?;
            match monster_archive::record(&entry, id)? {
                Some(r) => println!(
                    "verified: id {id} {} decodes (HP {}, element {})",
                    r.name, r.hp, r.element
                ),
                None => bail!("patched image: id {id}'s record no longer decodes"),
            }
        }
        println!(
            "re-packed id {id} ({} bytes decoded) into its slot",
            block.len()
        );
    }
    Ok(())
}
