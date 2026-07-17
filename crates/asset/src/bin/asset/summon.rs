use std::path::Path;

use crate::common::*;
use anyhow::Result;

/// Print the player Seru-magic summon → namesake creature map. The map is a
/// static table recovered from the disc by mesh identity (see
/// [`legaia_asset::summon_creatures`]); `--scus` annotates each row with the
/// summon's spell name.
pub(crate) fn summon_creatures_cmd(scus: Option<&Path>, json: bool) -> Result<()> {
    use legaia_asset::summon_creatures::SUMMON_CREATURES;
    let names = match scus {
        Some(p) => {
            let bytes = crate::common::read_input(p)?;
            legaia_asset::spell_names::SpellNameTable::from_scus(&bytes)
        }
        None => None,
    };
    let spell_name = |id: u8| -> Option<String> {
        names
            .as_ref()
            .and_then(|t| t.name(id))
            .filter(|n| !n.is_empty())
            .map(str::to_string)
    };
    if json {
        let rows: Vec<_> = SUMMON_CREATURES
            .iter()
            .map(|c| {
                serde_json::json!({
                    "spell_id": c.spell_id,
                    "spell_name": spell_name(c.spell_id),
                    "creature_id": c.creature_id,
                    "creature_name": c.name,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    println!(
        "summon → creature map ({} summons; creature mesh byte-identical to the PROT 867 archive record)",
        SUMMON_CREATURES.len()
    );
    println!(
        "{:>9}  {:<14}  {:>11}  creature",
        "spell id", "spell name", "creature id"
    );
    for c in SUMMON_CREATURES {
        println!(
            "  {:#04x}     {:<14}  {:>11}  {}",
            c.spell_id,
            spell_name(c.spell_id).unwrap_or_default(),
            c.creature_id,
            c.name,
        );
    }
    Ok(())
}

/// Parse a per-summon stager overlay and print its move-VM part-record list.
pub(crate) fn summon_overlay_cmd(input: &Path, base: u32, trim: Option<u32>) -> Result<()> {
    let mut bytes = crate::common::read_input(input)?;
    let full_len = bytes.len();
    if let Some(t) = trim {
        bytes.truncate(t as usize);
    }
    let ov = legaia_asset::summon_overlay::parse(&bytes, base);
    println!(
        "summon overlay {}: {} bytes ({} after trim), link base {:#010x}",
        input.display(),
        full_len,
        bytes.len(),
        ov.link_base
    );
    println!(
        "{} FUN_80021B04/FUN_80050ED4 spawn site(s), {} part record(s) recovered",
        ov.spawn_sites,
        ov.parts.len()
    );
    for (i, p) in ov.parts.iter().enumerate() {
        use legaia_asset::summon_overlay::SummonPartKind;
        let kind = match p.kind() {
            SummonPartKind::TransformNode => "transform-node".to_string(),
            SummonPartKind::LibraryMesh => format!("mesh-sel {}", p.model_sel),
            SummonPartKind::Sentinel => format!("sentinel {:#06x}", p.model_sel as u16),
        };
        println!(
            "  part {i:2}: rec @ file {:#06x} (rt {:#010x})  {kind}  flags {:#06x}  bytecode {:#x}..{:#x} ({} bytes)",
            p.record_off,
            base.wrapping_add(p.record_off as u32),
            p.flags,
            p.bytecode.start,
            p.bytecode.end,
            p.bytecode.len(),
        );
    }
    Ok(())
}

pub(crate) fn summon_readef_cmd(
    input: &Path,
    texture_png_dir: Option<&Path>,
    clut_sub: u8,
    action_id: Option<u8>,
) -> Result<()> {
    use legaia_asset::summon_readef::{self, SLOT_BYTES, SlotKind, StreamFile};

    if let Some(id) = action_id {
        let (file, slot) = summon_readef::stream_target(id);
        let (name, prot) = match file {
            StreamFile::Summon => ("summon.dat", summon_readef::SUMMON_PROT_INDEX),
            StreamFile::Readef => ("readef.DAT", summon_readef::READEF_PROT_INDEX),
        };
        println!(
            "action id {id:#04x}: base byte {:#04x} -> {name} (extraction PROT {prot:04}) slot {slot}",
            summon_readef::base_byte_for_action(id),
        );
        return Ok(());
    }

    let bytes = crate::common::read_input(input)?;
    let file = summon_readef::parse(&bytes)?;
    println!(
        "side-band file {}: {} bytes, {} slot(s) of {SLOT_BYTES:#x}",
        input.display(),
        bytes.len(),
        file.slots.len()
    );
    if let Some(dir) = texture_png_dir {
        std::fs::create_dir_all(dir)?;
    }
    if clut_sub > 15 {
        anyhow::bail!("--clut-sub must be 0..=15");
    }
    for slot in &file.slots {
        let raw = &bytes[slot.index * SLOT_BYTES..(slot.index + 1) * SLOT_BYTES];
        match &slot.kind {
            SlotKind::Texture(t) => {
                println!(
                    "  slot {:3}: texture  mode {}  {} CLUT row(s)  page {}x256 (4bpp)",
                    slot.index,
                    t.mode,
                    t.clut_rows,
                    t.texture_width_halfwords * 4,
                );
                if let Some(dir) = texture_png_dir {
                    let path = dir.join(format!("slot_{:03}.png", slot.index));
                    let (w, h) = (t.texture_width_halfwords * 4, 256usize);
                    // 16-color window of the first CLUT row (BGR555 at +4).
                    let clut_base = 4 + clut_sub as usize * 32;
                    let pal: Vec<[u8; 4]> = (0..16)
                        .map(|i| {
                            let off = clut_base + i * 2;
                            legaia_tim::bgr555_to_rgba8(u16::from_le_bytes([
                                raw[off],
                                raw[off + 1],
                            ]))
                        })
                        .collect();
                    let mut rgba = vec![0u8; w * h * 4];
                    for (texel, px) in rgba.chunks_exact_mut(4).enumerate() {
                        let byte = raw[t.texture_offset + texel / 2];
                        let idx = if texel % 2 == 0 {
                            byte & 0xF
                        } else {
                            byte >> 4
                        };
                        px.copy_from_slice(&pal[idx as usize]);
                    }
                    write_rgba_png(&path, w as u32, h as u32, &rgba)?;
                    println!("           -> {}", path.display());
                }
            }
            SlotKind::ActorRecord(r) => {
                println!(
                    "  slot {:3}: actor record  name {:?}  TMD @ +{:#06x}  pool @ +{:#06x}  {} part(s)",
                    slot.index,
                    r.name.as_deref().unwrap_or("-"),
                    r.tmd_offset,
                    r.texture_pool_offset,
                    r.part_count,
                );
            }
            SlotKind::MeArchive { count, compressed } => println!(
                "  slot {:3}: ME stream archive  {} entr{} ({} compressed)",
                slot.index,
                count,
                if *count == 1 { "y" } else { "ies" },
                compressed,
            ),
            SlotKind::Payload => println!("  slot {:3}: payload / raw", slot.index),
        }
    }
    Ok(())
}
