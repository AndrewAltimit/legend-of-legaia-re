//! `tim-list` / `tim-export` / `tim-replace` - texture replacement.
//!
//! The modding loop: `tim-list` catalogs every texture on the disc with its
//! replacement coordinates, `tim-export` decodes one to a PNG for editing,
//! and `tim-replace` encodes an edited PNG back into an in-place disc patch
//! (EDC/ECC re-encoded per touched sector).
//!
//! Three tiers share these commands. The raw and LZS tiers are keyed by a
//! byte offset (`--offset`, plus `--lzs-section`); the **battle** tier is
//! keyed by `--entry` + `--battle-slot`, because its blocks are not TIMs
//! and have no magic word to be an offset *of* - see
//! [`legaia_patcher::battle_texture`].

use std::path::Path;

use anyhow::{Context, Result, bail};

use legaia_asset::battle_texture_catalog::BattleTextureSlot;
use legaia_patcher::battle_texture::{self, BattleTextureTarget};
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::monster_texture::{self, MonsterTextureTarget};
use legaia_patcher::ppf;
use legaia_patcher::texture::{
    ExportFormat, TextureTarget, export_texture_png, read_texture, replace_texture_png,
    texture_catalogs,
};
use legaia_patcher::texture_palettes::texture_palettes;
use legaia_tim::encode::{EncodeOptions, decode_png_rgba};
use legaia_tim::multi_palette::View;

use crate::cli::TimTierArg;
use crate::util::{cue_contents, load_image, note_overwrite};

pub(crate) fn target(entry: Option<u32>, offset: u64, lzs_section: Option<u32>) -> TextureTarget {
    TextureTarget {
        entry,
        lzs_section,
        offset,
    }
}

pub(crate) fn cmd_tim_list(input: &Path, entry: Option<u32>, tier: TimTierArg) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let (raw, deep) = texture_catalogs(&patcher)?;

    println!("tier  entry  section  offset/slot  size       bpp  cluts  bytes    label");
    let mut raw_n = 0usize;
    if matches!(tier, TimTierArg::Raw | TimTierArg::All) {
        for t in &raw {
            if let Some(e) = entry
                && t.entry_index != Some(e)
            {
                continue;
            }
            raw_n += 1;
            println!(
                "raw   {:>5}  {:>7}  0x{:08X}  {:>4}x{:<4}  {:>3}  {:>5}  {:>7}  {}",
                t.entry_index
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "gap".into()),
                "-",
                t.offset_in_entry,
                t.width,
                t.height,
                t.bpp,
                t.clut_count,
                t.byte_len,
                t.label.unwrap_or(""),
            );
        }
    }
    let mut deep_n = 0usize;
    if matches!(tier, TimTierArg::Lzs | TimTierArg::All) {
        for t in &deep {
            if let Some(e) = entry
                && t.entry_index != e
            {
                continue;
            }
            deep_n += 1;
            println!(
                "lzs   {:>5}  {:>7}  0x{:08X}  {:>4}x{:<4}  {:>3}  {:>5}  {:>7}  {}",
                t.entry_index,
                t.lzs_section,
                t.offset_in_section,
                t.width,
                t.height,
                t.bpp,
                t.clut_count,
                t.byte_len,
                t.label.unwrap_or(""),
            );
        }
    }
    let mut battle_n = 0usize;
    if matches!(tier, TimTierArg::Battle | TimTierArg::All) {
        for b in &battle_texture::catalog(&patcher)? {
            if let Some(e) = entry
                && b.entry_index != e
            {
                continue;
            }
            battle_n += 1;
            println!(
                "battle{:>4}  {:>7}  {:>10}  {:>4}x{:<4}  {:>3}  {:>5}  {:>7}  {}",
                b.entry_index,
                b.section,
                b.slot().to_string(),
                b.width,
                b.height,
                b.bpp,
                b.clut_count,
                b.byte_len,
                b.label,
            );
        }
    }
    let mut monster_n = 0usize;
    if matches!(tier, TimTierArg::Monster | TimTierArg::All)
        && entry.is_none_or(|e| e == legaia_patcher::disc::MONSTER_ARCHIVE_ENTRY as u32)
    {
        for m in &monster_texture::catalog(&patcher)? {
            monster_n += 1;
            println!(
                "monst{:>5}  {:>7}  {:>10}  {:>4}x{:<4}  {:>3}  {:>5}  {:>7}  {} #{}",
                legaia_patcher::disc::MONSTER_ARCHIVE_ENTRY,
                m.id,
                m.id,
                m.width(),
                m.height(),
                4,
                m.palettes_populated(),
                m.byte_len(),
                m.name,
                m.id,
            );
        }
    }
    println!(
        "\n{raw_n} raw texture(s) (always replaceable in place) + {deep_n} LZS-compressed \
         (replaceable when the edited section recompresses into its footprint) + {battle_n} \
         battle block(s) (party character art; not TIMs, replaceable within the record's \
         slot footprint) + {monster_n} monster skin(s) (one page per enemy; not TIMs, \
         replaceable within the monster's own archive slot)."
    );
    println!(
        "Export one:  legaia-patcher tim-export --input DISC.bin --entry N --offset 0xHEX \
         [--lzs-section S] -o out.png"
    );
    println!(
        "Replace it:  legaia-patcher tim-replace --input DISC.bin --entry N --offset 0xHEX \
         [--lzs-section S] --png edited.png --patch out.ppf"
    );
    println!(
        "Battle art:  legaia-patcher tim-export --input DISC.bin --entry 864 \
         --battle-slot 14 -o armband.png"
    );
    println!(
        "Monster:     legaia-patcher tim-export --input DISC.bin --monster-id 179 -o songi.png"
    );
    Ok(())
}

/// Resolve the battle-tier target from `--entry` + `--battle-slot`, or
/// report what a caller who passed only one of them is missing.
fn battle_target(entry: Option<u32>, slot: BattleTextureSlot) -> Result<BattleTextureTarget> {
    let entry = entry.context(
        "--battle-slot needs --entry: the slot selector is per player file. Retail's are \
863 (Vahn) / 864 (Noa) / 865 (Gala) / 866. Run `tim-list --tier battle` to see them.",
    )?;
    Ok(BattleTextureTarget { entry, slot })
}

/// A texture coordinate as the CLI takes it (one of the three addressing
/// shapes).
pub(crate) struct TimCoord {
    pub(crate) entry: Option<u32>,
    pub(crate) offset: Option<u64>,
    pub(crate) lzs_section: Option<u32>,
    pub(crate) battle_slot: Option<BattleTextureSlot>,
    pub(crate) monster_id: Option<u16>,
}

pub(crate) fn cmd_tim_export(
    input: &Path,
    at: TimCoord,
    clut: Option<usize>,
    in_game: bool,
    format: ExportFormat,
    output: &Path,
) -> Result<()> {
    if let Some(slot) = at.battle_slot {
        return cmd_battle_export(
            input,
            battle_target(at.entry, slot)?,
            clut.unwrap_or(0),
            output,
        );
    }
    if let Some(id) = at.monster_id {
        return cmd_monster_export(input, MonsterTextureTarget { id }, output);
    }
    let offset = at.offset.context(
        "pass --offset (the byte offset tim-list prints), --battle-slot for the battle tier, \
         or --monster-id for a monster skin",
    )?;
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let t = target(at.entry, offset, at.lzs_section);
    let orig = read_texture(&patcher, &t)?;
    let pals = texture_palettes(&patcher, &orig.tim)?;
    let view = match clut {
        Some(k) if !in_game => View::Palette(k),
        _ if in_game || pals.has_map() => View::InGame,
        _ => View::Palette(0),
    };
    if let Some(k) = clut
        && k >= orig.tim.palette_count().max(1)
    {
        bail!(
            "--clut {k}: {t} has {} palette(s)",
            orig.tim.palette_count()
        );
    }
    let ex = export_texture_png(&patcher, &t, format, view)?;
    std::fs::write(output, &ex.png).with_context(|| format!("write {}", output.display()))?;
    println!(
        "wrote {} - {t}: {} ({} bpp, {} palette(s), {} bytes on disc)",
        output.display(),
        ex.description,
        bpp_name(&orig.tim),
        orig.tim.palette_count(),
        orig.tim_bytes.len(),
    );
    if in_game && !pals.has_map() {
        println!(
            "  note: no per-region palette map is known for this texture, so it was drawn \
             through palette 0 (try --clut N, or --format composite to see every palette)"
        );
    }
    if orig.tim.palette_count() > 1
        && format == ExportFormat::Image
        && ex.view != Some(View::InGame)
    {
        println!(
            "  this texture has {} palettes; --format composite also writes every palette below \
             the image, and tim-palette-map says which region the game draws with which",
            orig.tim.palette_count()
        );
    }
    println!(
        "Edit it (same dimensions!) and feed it back through tim-replace - it recognises \
         the view, a composite, a palette strip or an indexed PNG on its own."
    );
    Ok(())
}

fn bpp_name(tim: &legaia_tim::Tim) -> u32 {
    match tim.mode {
        legaia_tim::PixelMode::Bpp4 => 4,
        legaia_tim::PixelMode::Bpp8 => 8,
        legaia_tim::PixelMode::Bpp16 => 16,
        legaia_tim::PixelMode::Bpp24 => 24,
        legaia_tim::PixelMode::Mixed => 0,
    }
}

/// `tim-palette-map` - print which palette each region is drawn through.
pub(crate) fn cmd_tim_palette_map(
    input: &Path,
    entry: Option<u32>,
    offset: u64,
    lzs_section: Option<u32>,
) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let t = target(entry, offset, lzs_section);
    let orig = read_texture(&patcher, &t)?;
    let pals = texture_palettes(&patcher, &orig.tim)?;
    let n = orig.tim.palette_count();
    println!(
        "{t}: {}x{} {} bpp, {n} palette(s)",
        orig.tim.pixel_width(),
        orig.tim.pixel_height(),
        bpp_name(&orig.tim)
    );
    if !pals.has_map() {
        println!(
            "No per-region palette map is known for this texture: the file does not say which \
             palette a region is drawn with, the draw code does, and only the menu / battle UI \
             sheet has its draw table decoded. Every palette is an equally valid view - use \
             tim-export --format composite to see them all."
        );
        return Ok(());
    }
    println!("Source: {}", pals.source);
    println!("    x    y    w    h  palette  part   widget(s)");
    // One row per distinct (rect, palette): several records draw the same
    // sprite (the four party slots, the two framed-window styles).
    let mut rows: Vec<(&legaia_patcher::texture_palettes::PaletteRegion, Vec<u8>)> = Vec::new();
    for r in &pals.regions {
        match rows
            .iter_mut()
            .find(|(k, _)| k.rect == r.rect && k.palette == r.palette && k.part == r.part)
        {
            Some((_, ids)) => ids.push(r.widget),
            None => rows.push((r, vec![r.widget])),
        }
    }
    for (r, ids) in &rows {
        let pal = if r.palette < n {
            format!("{:>7}", r.palette)
        } else {
            format!("sub {:>3}*", r.subpalette)
        };
        let ids: Vec<String> = ids.iter().map(|i| format!("0x{i:02X}")).collect();
        println!(
            "  {:>3}  {:>3}  {:>3}  {:>3}  {pal}  {:<5}  {}",
            r.rect.0,
            r.rect.1,
            r.rect.2,
            r.rect.3,
            format!("{:?}", r.part).to_lowercase(),
            ids.join(" "),
        );
    }
    println!(
        "{} region(s). `sub N*` = a palette of the sibling extension TIM (PROT.DAT 0x{:X}), \
         read-only here. {} pixel(s) no widget draws are shown through palette 0 (overlay code \
         may draw some of them with a palette of its own). Where regions overlap, single \
         sprites win over plates, plates over bars, bars over windows, then the smaller rect; \
         {} pixel(s) are drawn through more than one palette.",
        rows.len(),
        legaia_asset::ui_widgets::SUBPALETTE_EXT_TIM_PROT_OFFSET,
        pals.unclaimed_pixels,
        pals.contested_pixels
    );
    for note in &pals.notes {
        println!("note: {note}");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_tim_replace(
    input: &Path,
    entry: Option<u32>,
    offset: Option<u64>,
    lzs_section: Option<u32>,
    battle_slot: Option<BattleTextureSlot>,
    monster_id: Option<u16>,
    clut: usize,
    png: &Path,
    quantize: bool,
    output: Option<&Path>,
    patch: Option<&Path>,
    dry_run: bool,
) -> Result<()> {
    if !dry_run && output.is_none() && patch.is_none() {
        bail!(
            "pass --output <patched.bin> and/or --patch <out.ppf> (or --dry-run to only validate)"
        );
    }
    if let Some(id) = monster_id {
        return cmd_monster_replace(
            input,
            MonsterTextureTarget { id },
            png,
            quantize,
            output,
            patch,
            dry_run,
        );
    }
    if let Some(slot) = battle_slot {
        return cmd_battle_replace(
            input,
            battle_target(entry, slot)?,
            clut,
            png,
            quantize,
            output,
            patch,
            dry_run,
        );
    }
    let offset = offset.context(
        "pass --offset (the byte offset tim-list prints), --battle-slot for the battle tier, \
         or --monster-id for a monster skin",
    )?;
    let original = load_image(input)?;
    let mut patcher = DiscPatcher::open(original.clone()).context("parse disc image")?;
    let t = target(entry, offset, lzs_section);

    let png_bytes = std::fs::read(png).with_context(|| format!("read {}", png.display()))?;
    let opts = EncodeOptions {
        quantize,
        ..Default::default()
    };
    let outcome = replace_texture_png(&mut patcher, &t, &png_bytes, &opts, dry_run)?;

    println!(
        "{}: {}x{} {} bpp, {} palette(s), {} bytes - encoded in place",
        t, outcome.width, outcome.height, outcome.bpp, outcome.clut_count, outcome.byte_len
    );
    if let Some(kind) = outcome.import {
        println!("  recognised: {kind}");
    }
    if outcome.new_palette_entries > 0 {
        println!(
            "  {} new palette color(s) written into free slot(s)",
            outcome.new_palette_entries
        );
    }
    if outcome.palette_entries_changed > 0 {
        println!(
            "  {} palette entr(y/ies) changed in total; every other palette entry is untouched",
            outcome.palette_entries_changed
        );
    }
    if outcome.quantized_pixels > 0 {
        println!(
            "  {} pixel(s) quantized to the nearest palette color",
            outcome.quantized_pixels
        );
    }
    if let Some(fit) = outcome.lzs {
        println!(
            "  LZS fit: recompressed {} bytes into the {}-byte retail stream",
            fit.recompressed, fit.capacity
        );
    }
    if dry_run {
        println!("dry run: validated only, nothing written");
        return Ok(());
    }

    // Verify off the patched image: the texture reads back strict-valid and
    // byte-for-byte what the encoder produced from the input image's
    // original.
    let after = read_texture(&patcher, &t).context("re-read patched texture")?;
    let before = read_texture(&DiscPatcher::open(original.clone())?, &t)?;
    let pals = texture_palettes(&patcher, &before.tim)?;
    let expected =
        legaia_tim::multi_palette::import_png(&before.tim, &png_bytes, &pals.context, &opts)
            .context("re-encode for verification")?;
    if expected.encoded.bytes != after.tim_bytes {
        bail!("verification failed: the patched texture is not what the encoder produced");
    }
    println!("verified: the patched texture reads back exactly as encoded");

    let patched = patcher.into_image();
    if let Some(ppf_path) = patch {
        let runs = ppf::diff_runs(&original, &patched);
        let desc = format!("Legaia texture replace {t}");
        let bytes = ppf::write_ppf3(&desc, &runs);
        note_overwrite(ppf_path);
        std::fs::write(ppf_path, &bytes)
            .with_context(|| format!("write {}", ppf_path.display()))?;
        println!(
            "wrote {} ({} change run(s)) - safe to share, carries only your edit",
            ppf_path.display(),
            runs.len()
        );
    }
    if let Some(out) = output {
        note_overwrite(out);
        std::fs::write(out, &patched).with_context(|| format!("write {}", out.display()))?;
        let bin_name = out
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "patched.bin".into());
        let cue_path = out.with_extension("cue");
        std::fs::write(&cue_path, cue_contents(&bin_name))
            .with_context(|| format!("write {}", cue_path.display()))?;
        println!(
            "wrote {} + {} (contains Sony bytes - local play only, never redistribute)",
            out.display(),
            cue_path.display()
        );
    }
    Ok(())
}

/// `tim-export --battle-slot` - decode one player-file battle block.
fn cmd_battle_export(
    input: &Path,
    target: BattleTextureTarget,
    palette: usize,
    output: &Path,
) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let ex = battle_texture::export_block(&patcher, &target, palette)?;
    legaia_tim::write_png(output, ex.width, ex.height, &ex.rgba)?;
    println!(
        "wrote {} - {} ({}x{}, 4 bpp, decoded through {})",
        output.display(),
        target,
        ex.width,
        ex.height,
        ex.palette,
    );
    if ex.palette_count > 1 {
        println!(
            "  this block ships {} palettes; --clut 0..{} picks which one you see (the mesh \
             samples them per primitive)",
            ex.palette_count,
            ex.palette_count - 1
        );
    } else if ex.palette_count == 0 {
        println!(
            "  this block ships no palette of its own - it samples the CLUT row its \
             sibling blocks assemble, so --clut indexes that row"
        );
    }
    println!("Edit it (same dimensions!) and feed it back through tim-replace --battle-slot.");
    Ok(())
}

/// `tim-replace --battle-slot` - write an edited PNG back into a
/// player-file battle block, recompressing the record into its own slot.
#[allow(clippy::too_many_arguments)]
fn cmd_battle_replace(
    input: &Path,
    target: BattleTextureTarget,
    palette: usize,
    png: &Path,
    quantize: bool,
    output: Option<&Path>,
    patch: Option<&Path>,
    dry_run: bool,
) -> Result<()> {
    let original = load_image(input)?;
    let mut patcher = DiscPatcher::open(original.clone()).context("parse disc image")?;

    let png_bytes = std::fs::read(png).with_context(|| format!("read {}", png.display()))?;
    let (w, h, rgba) = decode_png_rgba(&png_bytes)?;

    let outcome = battle_texture::replace_block(
        &mut patcher,
        &target,
        &rgba,
        w,
        h,
        palette,
        quantize,
        dry_run,
    )?;
    println!(
        "{}: {}x{} 4 bpp via {} - {} palette entr{} changed",
        target,
        outcome.width,
        outcome.height,
        outcome.palette,
        outcome.palette_entries_changed,
        if outcome.palette_entries_changed == 1 {
            "y"
        } else {
            "ies"
        },
    );
    if outcome.unchanged {
        println!(
            "  the image is already what is on disc - nothing written (re-encoding an \
             unchanged record would still move disc bytes, since our LZS encoder is not \
             the one the mastering used)"
        );
    } else {
        println!(
            "  slot fit: recompressed {} bytes into the {}-byte allocation (retail used {})",
            outcome.fit.recompressed, outcome.fit.capacity, outcome.fit.retail,
        );
    }
    if outcome.quantized_pixels > 0 {
        println!(
            "  {} pixel(s) quantized to the nearest kept colour",
            outcome.quantized_pixels
        );
    }
    if dry_run {
        println!("dry run: validated only, nothing written");
        return Ok(());
    }

    // Verify off the patched image, and prove the edit was surgical: this
    // block decodes to the requested pixels and no sibling block moved.
    let before = battle_texture::catalog(&DiscPatcher::open(original.clone())?)?;
    let after = battle_texture::catalog(&patcher).context("re-read patched catalog")?;
    if outcome.quantized_pixels == 0 {
        let got = battle_texture::export_block(&patcher, &target, palette)?;
        let want: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let stored = if p[3] == 0 {
                    0
                } else {
                    let e = legaia_tim::encode::rgba8_to_bgr555(*p);
                    if e == 0 { 0x8000 } else { e }
                };
                legaia_tim::bgr555_to_rgba8(stored)
            })
            .collect();
        if got.rgba != want {
            bail!("verification failed: patched block does not decode to the input image");
        }
        println!("verified: the block decodes pixel-exactly to the input image");
    }
    // The write must have touched this block and nothing else. A no-op
    // edit legitimately touches none, so the rule is containment, not a
    // count: an unedited round-trip is a zero-run patch.
    let want = target.slot.to_string();
    let strays: Vec<_> = before
        .iter()
        .zip(&after)
        .filter(|(b, a)| b.fnv1a != a.fnv1a || b.entry_index != a.entry_index)
        .map(|(b, _)| (b.entry_index, b.slot().to_string()))
        .filter(|(e, s)| (*e, s.as_str()) != (target.entry, want.as_str()))
        .collect();
    if !strays.is_empty() {
        bail!(
            "verification failed: the write also changed {} other block(s): {}",
            strays.len(),
            strays
                .iter()
                .map(|(e, s)| format!("entry {e} slot {s}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    println!("verified: every other battle block is byte-identical");

    let patched = patcher.into_image();
    if let Some(ppf_path) = patch {
        let runs = ppf::diff_runs(&original, &patched);
        let desc = format!("Legaia battle texture replace {target}");
        let bytes = ppf::write_ppf3(&desc, &runs);
        note_overwrite(ppf_path);
        std::fs::write(ppf_path, &bytes)
            .with_context(|| format!("write {}", ppf_path.display()))?;
        println!(
            "wrote {} ({} change run(s)) - safe to share, carries only your edit",
            ppf_path.display(),
            runs.len()
        );
    }
    if let Some(out) = output {
        note_overwrite(out);
        std::fs::write(out, &patched).with_context(|| format!("write {}", out.display()))?;
        let bin_name = out
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "patched.bin".into());
        let cue_path = out.with_extension("cue");
        std::fs::write(&cue_path, cue_contents(&bin_name))
            .with_context(|| format!("write {}", cue_path.display()))?;
        println!(
            "wrote {} + {} (contains Sony bytes - local play only, never redistribute)",
            out.display(),
            cue_path.display()
        );
    }
    Ok(())
}

// --- Monster battle skins ----------------------------------------------------

/// `tim-export --monster-id` - decode one monster's page.
///
/// The decode is a composite: every texel through the palette of the polygon
/// that samples it, and a texel no polygon samples transparent. There is no
/// `--clut` here on purpose - a monster page has no single colouring, so a
/// page-wide palette choice would be a claim the bytes do not make.
fn cmd_monster_export(input: &Path, target: MonsterTextureTarget, output: &Path) -> Result<()> {
    let image = load_image(input)?;
    let patcher = DiscPatcher::open(image).context("parse disc image")?;
    let ex = monster_texture::export_page(&patcher, &target)?;
    legaia_tim::write_png(output, ex.width, ex.height, &ex.rgba)?;
    println!(
        "wrote {} - {} #{} ({}x{}, 4 bpp, {} populated palette(s))",
        output.display(),
        ex.name,
        ex.id,
        ex.width,
        ex.height,
        ex.palettes_populated,
    );
    println!("Blank areas are texels no polygon samples - dead bytes; painting there is ignored.");
    println!(
        "Edit it (same dimensions, and only with colours already in the sheet) and feed it \
         back through tim-replace --monster-id."
    );
    Ok(())
}

/// `tim-replace --monster-id` - write an edited page back, recompressing the
/// monster's whole block into its own `0x14000` archive slot.
fn cmd_monster_replace(
    input: &Path,
    target: MonsterTextureTarget,
    png: &Path,
    quantize: bool,
    output: Option<&Path>,
    patch: Option<&Path>,
    dry_run: bool,
) -> Result<()> {
    let original = load_image(input)?;
    let mut patcher = DiscPatcher::open(original.clone()).context("parse disc image")?;

    let png_bytes = std::fs::read(png).with_context(|| format!("read {}", png.display()))?;
    let (w, h, rgba) = decode_png_rgba(&png_bytes)?;

    let outcome =
        monster_texture::replace_page(&mut patcher, &target, &rgba, w, h, quantize, dry_run)?;
    println!(
        "{} #{}: {}x{} 4 bpp - {} texel(s) re-indexed",
        outcome.name, outcome.id, outcome.width, outcome.height, outcome.texels_changed,
    );
    if outcome.unchanged {
        println!(
            "  the image is already what is on disc - nothing written (re-encoding an \
             unchanged block would still move disc bytes, since our LZS encoder is not \
             the one the mastering used)"
        );
    } else {
        println!(
            "  slot fit: recompressed {} bytes into the {}-byte allocation (retail used {})",
            outcome.fit.recompressed, outcome.fit.capacity, outcome.fit.retail,
        );
    }
    if outcome.quantized_texels > 0 {
        println!(
            "  {} texel(s) folded onto the nearest colour their own palette holds",
            outcome.quantized_texels
        );
    }
    if outcome.dead_texels_ignored > 0 {
        println!(
            "  {} painted texel(s) fall where nothing samples the page - left as retail wrote them",
            outcome.dead_texels_ignored
        );
    }
    if dry_run {
        println!("dry run: validated only, nothing written");
        return Ok(());
    }

    // Verify off the patched image: the page reads back as the write said it
    // would, and no other monster moved.
    let before = monster_texture::catalog(&DiscPatcher::open(original.clone())?)?;
    let after = monster_texture::catalog(&patcher).context("re-read patched catalog")?;
    if before.len() != after.len() {
        bail!(
            "verification failed: the archive held {} monsters before and {} after",
            before.len(),
            after.len()
        );
    }
    for (b, a) in before.iter().zip(&after) {
        if b.id != a.id || b.name != a.name {
            bail!("verification failed: monster {} changed identity", b.id);
        }
        if b.id != target.id && b.pool_bytes() != a.pool_bytes() {
            bail!(
                "verification failed: monster {}'s page changed, and it was not the target",
                b.id
            );
        }
    }
    println!(
        "verified: only {} #{}'s page changed",
        outcome.name, outcome.id
    );

    let patched = patcher.into_image();
    if let Some(ppf_path) = patch {
        let runs = ppf::diff_runs(&original, &patched);
        let desc = format!(
            "Legaia monster skin replace ({} #{})",
            outcome.name, outcome.id
        );
        note_overwrite(ppf_path);
        std::fs::write(ppf_path, ppf::write_ppf3(&desc, &runs))
            .with_context(|| format!("write {}", ppf_path.display()))?;
        println!(
            "wrote {} ({} change run(s)) - safe to share, carries only your edit",
            ppf_path.display(),
            runs.len()
        );
    }
    if let Some(out) = output {
        note_overwrite(out);
        std::fs::write(out, &patched).with_context(|| format!("write {}", out.display()))?;
        let bin_name = out
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "patched.bin".into());
        let cue_path = out.with_extension("cue");
        std::fs::write(&cue_path, cue_contents(&bin_name))
            .with_context(|| format!("write {}", cue_path.display()))?;
        println!(
            "wrote {} + {} (contains Sony bytes - local play only, never redistribute)",
            out.display(),
            cue_path.display()
        );
    }
    Ok(())
}
