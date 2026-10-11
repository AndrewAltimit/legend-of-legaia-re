//! Effect-texture VRAM uploads: the `befect_data` `etim` section, the flame
//! atlas, a CDNAME block's image rects and the battle-stage TIMs, each read
//! through a [`ProtIndex`] into a software VRAM.

use super::*;

/// PROT entry index for `befect_data` carrying the global TMD-pool head
/// (the 5 character-mesh TMDs at retail `DAT_8007C018[0..4]`). Pinned by
/// byte-equality against a Drake post-warp RAM snapshot.
pub const PROT_BEFECT_DATA_ENTRY: u32 = 874;

/// PROT entry holding the battle effect-texture atlas (the "flame atlas"):
/// three 64x256 4bpp PSX TIMs blitted to VRAM `(320,0)`, `(384,0)`, `(448,0)`
/// with CLUTs in rows 474..=476 (the effect-CLUT band). Stored uncompressed,
/// back-to-back behind a 16-byte prefix. Despite its CDNAME label
/// (`sound_data`, shared with PROT 871) it carries no audio - the label is one
/// of the documented CDNAME mislabels. Byte-verified pixel-exact in VRAM
/// against every stable Rim Elm battle capture (command-menu / submenu /
/// pre- and post-Seru-capture frames); the partial match in a still-loading
/// frame is just the mid-DMA snapshot. Unlike `etim.dat` (PROT 874 section 2,
/// pages at `fb_y=256`), these pages sit at `fb_y=0` in the same VRAM columns
/// the field uses for town stage textures, so they are *battle-only* uploads -
/// the field captures hold unrelated town texels there. Retail blits them at
/// battle load (not by the `FUN_800520F0` etmd/befect path, which pulls
/// indices `0x367..=0x36d` - PROT 870 = index `0x366` is loaded by a separate
/// site). See `docs/formats/effect.md`.
const PROT_FLAME_ATLAS_ENTRY: u32 = 870;

/// PROT 0874 (`befect_data`) section index carrying `etim.dat` - the battle
/// effect-sprite TIMs. The three LZS sections are: 0 = effect 3D models
/// (`etmd.dat`, the global-TMD-pool head), 1 = `vdf.dat`, 2 = `etim.dat`.
const BEFECT_ETIM_SECTION: usize = 2;

/// Upload the player `player.lzs` texture section (PROT 0874 section 2) into
/// `vram`. This 8-TIM pack carries **both** the 3D effect-model textures
/// (`etim.dat`, the texel source for `etmd.dat` / section 0's global-TMD-pool
/// head) **and the field-character texture atlas**: entries 1/2/3 are the
/// Vahn/Noa/Gala atlas pages at texpage `(832, 256)` with per-character CLUTs
/// on row 478 (the field-form player meshes sample exactly these). Retail
/// uploads the whole section at field-init via `FUN_8001E890 → FUN_800198E0`
/// (`LoadImage`) and keeps it resident across the battle scene-mode overlay, so
/// uploading at scene entry is equivalent. See
/// [`docs/formats/character-mesh.md` § Textures (field form)] for the full
/// entry table.
///
/// CLUT blocks are uploaded as **flat horizontal strips** (`FUN_800198e0`:
/// `LoadImage(rect = { x, y, w*h, 1 })`), not as the declared `w x h`
/// rectangle - see the inline note. (`legaia_actor_formats::field_char_textures` is the
/// standalone parser + verifier for the same section, byte-exact against a live
/// field VRAM dump.)
///
/// This makes the texels resident for effect-model rendering. (It does *not*
/// feed the 2D `efect.dat` sprite-atlas billboards, which sample a separate
/// page-`(0,0)` 8bpp source - see `World::active_effect_sprites`
/// and the open atlas-source thread in `docs/formats/effect.md`.)
///
/// Mirrors `seed_global_tmd_pool_from_befect_data`'s LZS path. Soft-fails;
/// returns the number of TIMs uploaded.
///
/// Public so the VRAM-parity oracle's lightweight pre-pass can apply the same
/// effect-texture upload the live field-entry path performs - without it the
/// oracle reports the `fb_y=256` effect pages (fb_x 320/384/832/852/872/880)
/// as a phantom static gap that the real engine never has.
///
/// `upload_clut` controls whether the TIMs' CLUT rows (473..=478) are written
/// alongside the image pages. Retail keeps the effect-texture *pixel* pages
/// (fb_y=256) resident from field through battle, but uploads their CLUTs at
/// battle entry - so a field-VRAM parity build wants `upload_clut = false`
/// (image pages only) while the live field-entry seed passes `true` to keep
/// the CLUTs resident through the battle scene-mode overlay.
pub fn upload_effect_textures_into_vram(
    index: &ProtIndex,
    vram: &mut legaia_tim::Vram,
    upload_clut: bool,
) -> Result<usize> {
    let decoded = befect_etim_section_bytes(index)?;
    let mut uploaded = 0;
    for target in legaia_actor_formats::befect_cluster::scan_tims(&decoded) {
        match legaia_tim::parse(&decoded[target.offset..]) {
            Ok(tim) => {
                // Image page: declared rect, verbatim.
                vram.upload_tim_partial(&tim, true, false);
                // CLUT: `FUN_800198e0` uploads the CLUT block as a FLAT
                // horizontal strip - `LoadImage(rect = { x, y, w*h, 1 })` -
                // not the declared `w x h` rectangle. This matters for §2's
                // field-character TIMs (entries 1/2/3, CLUT `w=16 h=4`): a
                // rect upload puts Vahn's four 16-colour palettes at rows
                // 478..481 col 0, but the meshes sample them as columns
                // 0/16/32/48 of row 478. The strip places them correctly.
                // (Field upload runs with STP off, `_DAT_8007b998 == 0`.)
                if upload_clut && let Some(clut) = tim.clut.as_ref() {
                    let strip: Vec<u8> =
                        clut.entries.iter().flat_map(|c| c.to_le_bytes()).collect();
                    vram.write_clut_row(clut.fb_x, clut.fb_y, &strip);
                }
                uploaded += 1;
            }
            Err(err) => {
                eprintln!(
                    "[scene] etim TIM @0x{:x} did not parse ({err:#}); skipping",
                    target.offset
                );
            }
        }
    }
    if uploaded == 0 {
        anyhow::bail!("etim section carried no uploadable TIMs");
    }
    Ok(uploaded)
}

/// Decoded `befect_data` (PROT 874) etim-section bytes - the shared
/// effect-texture TIM pool [`upload_effect_textures_into_vram`] and
/// [`effect_texture_image_rects`] both walk.
fn befect_etim_section_bytes(index: &ProtIndex) -> Result<Vec<u8>> {
    let raw = index
        .entry_bytes(PROT_BEFECT_DATA_ENTRY)
        .with_context(|| format!("read PROT entry {} (befect_data)", PROT_BEFECT_DATA_ENTRY))?;
    let container = legaia_container_formats::parse_player_lzs(&raw, 3)
        .context("parse befect_data as a 3-descriptor player.lzs-shaped container")?;
    let section = container
        .descriptors
        .get(BEFECT_ETIM_SECTION)
        .ok_or_else(|| {
            anyhow::anyhow!("befect_data has no section {BEFECT_ETIM_SECTION} (etim)")
        })?;
    legaia_container_formats::decode(&raw, section, legaia_container_formats::DecodeMode::Lzs)
        .with_context(|| format!("LZS-decode befect_data section {BEFECT_ETIM_SECTION} (etim)"))
}

/// VRAM image rects `(fb_x, fb_y, width_in_words, height)` of the
/// `befect_data` effect-texture TIMs - the upload set of
/// [`upload_effect_textures_into_vram`].
///
/// The band is **global shared state**, not per-scene texture: one disc
/// source is resident across every field scene. A handful of its pixels are
/// *history-dependent* - the pause-menu entry path writes an F-variant of
/// three row-271 words (pinned at `(853, 271)`: pause-menu-lineage captures
/// hold `0xFFFF` where the disc TIM carries `0x3333`; each variant word
/// equals the same TIM's row-273 value), and the first battle effect use
/// restores the disc bytes. A per-scene static mask misclassifies those
/// pixels as static whenever a scene's captures share menu/battle history,
/// so the VRAM parity oracle uses these rects to demand staticity across
/// **all** scenes' captures instead.
pub fn effect_texture_image_rects(index: &ProtIndex) -> Result<Vec<(u16, u16, u16, u16)>> {
    let decoded = befect_etim_section_bytes(index)?;
    let mut rects = Vec::new();
    for target in legaia_actor_formats::befect_cluster::scan_tims(&decoded) {
        if let Ok(tim) = legaia_tim::parse(&decoded[target.offset..]) {
            let img = &tim.image;
            rects.push((img.fb_x, img.fb_y, img.fb_w, img.h));
        }
    }
    Ok(rects)
}

/// VRAM image rects `(fb_x, fb_y, width_in_words, height)` of every TIM a
/// CDNAME block carries, via the same scanner the scene VRAM build uses.
///
/// Sibling of [`effect_texture_image_rects`] for the **shared-block** upload
/// set ([`crate::scene_resources::FIELD_SHARED_BLOCKS`]): `init_data`'s UI
/// tile pages at `fb=(704, 0)` / `fb=(704, 256)` are *journey-dependent*
/// residency, not per-scene texture - an overworld transit leaves kingdom-
/// bundle content over parts of the rect, so a field scene reached through
/// the world map holds kingdom bytes there while a boot-fresh scene holds
/// the disc tiles. A per-scene static mask misclassifies those words as
/// static whenever a scene's captures share route history; the VRAM parity
/// oracle pools captures across all scenes against these rects instead.
pub fn block_image_rects(index: &ProtIndex, block: &str) -> Result<Vec<(u16, u16, u16, u16)>> {
    let scene = Scene::load(index, block)?;
    let mut rects = Vec::new();
    for entry in &scene.entries {
        let scan = legaia_container_formats::tim_scan::scan_entry(&entry.bytes);
        for (source, hit) in &scan.hits {
            let payload: &[u8] = match source {
                legaia_container_formats::tim_scan::Source::Raw => &entry.bytes,
                legaia_container_formats::tim_scan::Source::Lzs(idx) => {
                    scan.lzs_sections[*idx].as_slice()
                }
            };
            if let Some(slice) = payload.get(hit.offset..)
                && let Ok(tim) = legaia_tim::parse(slice)
            {
                let img = &tim.image;
                rects.push((img.fb_x, img.fb_y, img.fb_w, img.h));
            }
        }
    }
    Ok(rects)
}

/// Upload the battle effect-texture atlas (PROT 870, the "flame atlas") into
/// `vram`. These three 64x256 4bpp TIMs (pages at `(320,0)`, `(384,0)`,
/// `(448,0)`, CLUTs in rows 474..=476) are the texel source for the
/// fire/flame effect meshes during battle, byte-verified against live battle
/// VRAM (see `PROT_FLAME_ATLAS_ENTRY`).
///
/// Call this on **battle entry**, not field entry: the pages land in the same
/// VRAM columns (`fb_x` 320..512, `fb_y` 0) the field stage textures occupy,
/// so uploading them while a field scene is resident would clobber town
/// rendering. Retail overwrites that region for battle and the field reloads
/// its textures on return - the play-window battle path mirrors this by
/// blitting into a throwaway VRAM copy that battle exit discards.
///
/// `upload_clut` writes the CLUT rows (474..=476) alongside the image pages.
/// Mirrors [`upload_effect_textures_into_vram`]; PROT 870 is uncompressed, so
/// the TIMs are walked straight out of the entry bytes (read via the extended
/// footprint, like the PROT 871 effect-model library - the indexed size can
/// truncate the trailing TIM). Soft-fails; returns the number of TIMs uploaded.
pub fn upload_flame_atlas_into_vram(
    index: &ProtIndex,
    vram: &mut legaia_tim::Vram,
    upload_clut: bool,
) -> Result<usize> {
    let raw = index
        .entry_bytes_extended(PROT_FLAME_ATLAS_ENTRY)
        .with_context(|| format!("read PROT entry {PROT_FLAME_ATLAS_ENTRY} (flame atlas)"))?;
    let mut uploaded = 0;
    for target in legaia_actor_formats::befect_cluster::scan_tims(&raw) {
        match legaia_tim::parse(&raw[target.offset..]) {
            Ok(tim) => {
                vram.upload_tim_partial(&tim, true, upload_clut);
                uploaded += 1;
            }
            Err(err) => {
                eprintln!(
                    "[scene] flame-atlas TIM @0x{:x} did not parse ({err:#}); skipping",
                    target.offset
                );
            }
        }
    }
    if uploaded == 0 {
        anyhow::bail!("flame atlas (PROT {PROT_FLAME_ATLAS_ENTRY}) carried no uploadable TIMs");
    }
    Ok(uploaded)
}

/// Re-upload the **chosen** battle-stage stream's own TIMs on top of `vram`,
/// so a scene whose bundle carries several stage streams renders the one it
/// actually fights in.
///
/// Rim Elm is the case that makes this load-bearing. Its bundle carries four
/// backdrop streams (extraction entries 6..9, the Tetsu spar is fought in
/// `7` - see `ProtIndex::battle_stage_entry_for_scene`) and **all four target
/// the identical VRAM addresses**: a 4bpp page at `(768, 0)` under the CLUT
/// row at `(0, 473)`, and a second at `(832, 0)` under `(0, 479)`. So do the
/// stage streams of every other multi-sub-area scene, and the field texture
/// pack puts a page at `(768, 0)` too.
///
/// Retail never has to resolve that: the type-`0x01` chunk walker records one
/// stream in `_DAT_8007B864` and only that one's pages are resident. The
/// port's battle resource build instead DMAs every TIM in the bundle
/// (`BuildOptions::upload_all_tims`), so the last sibling written wins the
/// shared page and CLUT row. The shell then draws its own geometry through a
/// neighbour's texels and palette: on `town01` the semi-transparent cloud
/// quads - `(768, 0)`, palette `1` of row 473, a greyscale + STP ramp - came
/// out as flat green rectangles standing on the arena wall, because the
/// palette that won the row was one of the rainbow CLUT-cycling ramps a
/// sibling stream parks at the same index.
///
/// Re-uploading the selected entry last restores retail's residency without
/// touching the general build: everything the bundle contributed stays, and
/// only the addresses this stage owns are taken back. Returns the number of
/// TIMs re-uploaded (`0` when the entry carries none, which is not an error).
pub fn upload_battle_stage_tims_into_vram(
    scene: &Scene,
    stage_entry: u32,
    vram: &mut legaia_tim::Vram,
) -> usize {
    let Some(entry) = scene.entries.iter().find(|e| e.idx == stage_entry) else {
        return 0;
    };
    let bytes: &[u8] = &entry.bytes;
    let mut uploaded = 0;
    for hit in legaia_container_formats::tim_scan::scan_buffer(bytes) {
        let end = hit.offset + hit.byte_len;
        if end > bytes.len() {
            continue;
        }
        if let Ok(tim) = legaia_tim::parse(&bytes[hit.offset..end]) {
            vram.upload_tim_partial(&tim, true, true);
            uploaded += 1;
        }
    }
    uploaded
}
