//! Effect-bundle / global-TMD-pool / VRAM-upload seeding constants and helpers.
//!
//! Extracted verbatim from `scene/host.rs`.

use super::*;

/// PROT entry index for `befect_data` carrying the global TMD-pool head
/// (the 5 character-mesh TMDs at retail `DAT_8007C018[0..4]`). Pinned in
/// `project_global_tmd_pool_source.md` via byte-equality vs a Drake post-warp
/// RAM snapshot.
const PROT_BEFECT_DATA_ENTRY: u32 = 874;

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

/// PROT entry holding the runtime effect buffer `data\battle\efect.dat` - the
/// 2-pack wrapper (inline sprite atlas + pack0 anim batches + pack1 effect
/// scripts) the battle effect VM consumes. Stored uncompressed; the raw entry
/// bytes are byte-identical to the post-init runtime buffer (`docs/formats/effect.md`).
const PROT_EFECT_DAT_ENTRY: u32 = 873;

/// PROT entry holding the global monster stat archive (one `0x14000`-byte
/// LZS slot per monster id; the CDNAME label `battle_data` is shared across
/// 0865-0868). The misleading `monster_data` label (PROT 869) is a stub.
/// See [`legaia_asset::monster_archive`] + `docs/subsystems/battle.md`.
pub(crate) const MONSTER_ARCHIVE_PROT_ENTRY: u32 = 867;

/// Number of slots PROT 0874 section 0 contributes to the head of the
/// global TMD pool. Set by the section's TMD-pack `count` field; the
/// retail pack carries exactly 5 character meshes.
pub(crate) const GLOBAL_TMD_POOL_HEAD_COUNT: usize = 5;

/// Index into [`crate::world::World::global_tmd_pool`] of the PROT 0874 §0
/// *preview* flame mesh - the smallest of that section's five TMDs (2 objects,
/// 18 verts, 25 prims). It bakes the `etim` CLUT (`cba=0x778E@(224,478)`,
/// `tsb=0x001D@(832,256)`) and looks flame-shaped, so the engine could render
/// it through the standard VRAM-mesh pipeline as a stand-in.
///
/// **This is a preview mesh, not the model retail draws.** The real battle
/// flame is [`GIMARD_TAIL_FIRE_MODEL_INDEX`], pulled from the PROT 0871
/// effect-model library (`seed_effect_model_library_from_etmd`). The
/// stand-in is kept only as a fallback when that library isn't loaded (e.g.
/// raw-PROT.DAT inspection without the battle assets). See
/// `docs/formats/effect.md`.
pub const ETMD_TAIL_FIRE_MODEL_INDEX: usize = 4;

/// PROT entry holding the battle effect-model library (`etmd.dat`): a 30-entry
/// `asset::pack` of Legaia TMDs (`word[0]=30`, every entry magic `0x80000002`),
/// stored uncompressed. Retail registers all 30 verbatim into
/// `DAT_8007C018[3..=32]` at battle init (`FUN_800520F0` debug index `0x367` ->
/// `FUN_80026B4C`); the dev-path name is `h:\prot\battle\etmd.dat`. The CDNAME
/// label `sound_data` is misleading - this is the effect-model library, not
/// audio. See `docs/formats/effect.md`.
const PROT_EFFECT_MODEL_LIBRARY_ENTRY: u32 = 871;

/// Base index in [`crate::world::World::global_tmd_pool`] (= `DAT_8007C018`)
/// where the PROT 0871 effect-model library registers. Its 30 models occupy
/// `[3..=32]`, overwriting the two trailing slots of the PROT 0874 §0 field
/// head (`[3]`, `[4]`) - exactly retail's temporal layout (the field head
/// seeds `[0..=4]`; battle init reloads `[3..=32]`).
///
/// This is the engine's analogue of the retail **battle `gp[0x754]` value** -
/// the additive base `FUN_80021B04` applies to a move-FX / summon part record's
/// `model_sel` (`DAT_8007C018[model_sel + gp[0x754]]`). In retail that base is
/// *not* a constant: it is `party_count + 2` (the two fixed pool slots + the live
/// party-character meshes precede the library), i.e. `3` for the 1-member
/// training party and `5` for the full 3-member party - save-corpus-pinned by
/// `crates/mednafen/tests/summon_model_base.rs` (see `docs/formats/move-power.md`).
/// The engine instead registers the library at a *fixed* `[3..=32]` and keeps
/// `model_sel` library-relative, so `model_sel + 3` lands on the same library
/// model retail reaches via `model_sel + gp[0x754]` - the library content is
/// identical, only its pool offset shifts with party size, so the two layouts are
/// equivalent. `World::spawn_move_fx` uses this fixed base.
pub(crate) const EFFECT_MODEL_LIBRARY_BASE: usize = 3;

/// Number of TMDs in the PROT 0871 effect-model library (`word[0]`).
pub(crate) const EFFECT_MODEL_LIBRARY_COUNT: usize = 30;

/// Index in [`crate::world::World::global_tmd_pool`] of Gimard's *Tail Fire*
/// flame model (`DAT_8007C018[26]`) - the model retail draws for the Gimard
/// Seru cast. Equals `EFFECT_MODEL_LIBRARY_BASE`` + 23` (pack entry 23). Its
/// fire flicker is CLUT/palette cycling driven by the summon stager overlay (extraction PROT 0903)
/// (the model geometry is static). Supersedes the PROT 0874 §0 preview
/// stand-in at [`ETMD_TAIL_FIRE_MODEL_INDEX`]. See `docs/formats/effect.md`.
pub const GIMARD_TAIL_FIRE_MODEL_INDEX: usize = 26;

/// Seed `World::global_tmd_pool[0..=4]` from PROT 0874 (`befect_data`)
/// section 0. Soft-fails (returns `Err`) when the entry is missing, the
/// section header is malformed, the LZS decode fails, or the inner
/// TMD-pack walk fails - the field-VM `0x4C 0xD8` host hook then leaves
/// `Actor::tmd_ref` at `None` rather than aborting scene-load.
///
/// The retail loader chain that produces these 5 entries via
/// `FUN_8001F05C case 2 → FUN_80026B4C` is not yet pinned (see open work
/// item in `docs/formats/world-map-overlay.md`); this routes the disc
/// bytes directly through the `parse_player_lzs + pack` parsers and
/// installs the parsed TMDs onto the world.
/// Load the effect-script catalog from PROT 0873 (`efect.dat`) into
/// `World::effect_catalog`. Soft-fails when the entry is missing or the
/// 2-pack is malformed (the catalog stays empty and nothing spawns). Parsing
/// itself never errors - [`EffectCatalog::from_efect_dat_bytes`] returns an
/// empty catalog on bad data - so the only error is the disc read.
pub(crate) fn seed_effect_catalog_from_efect_dat(
    index: &ProtIndex,
    world: &mut crate::world::World,
) -> Result<()> {
    let raw = index
        .entry_bytes(PROT_EFECT_DAT_ENTRY)
        .with_context(|| format!("read PROT entry {} (efect.dat)", PROT_EFECT_DAT_ENTRY))?;
    let catalog = legaia_engine_vm::effect_vm::EffectCatalog::from_efect_dat_bytes(&raw);
    if catalog.is_empty() {
        anyhow::bail!("efect.dat parsed to an empty catalog (unexpected 2-pack shape)");
    }
    world.effect_catalog = catalog;
    Ok(())
}

pub(crate) fn seed_global_tmd_pool_from_befect_data(
    index: &ProtIndex,
    world: &mut crate::world::World,
) -> Result<()> {
    let raw = index
        .entry_bytes(PROT_BEFECT_DATA_ENTRY)
        .with_context(|| format!("read PROT entry {} (befect_data)", PROT_BEFECT_DATA_ENTRY))?;
    let container = legaia_asset::parse_player_lzs(&raw, 3)
        .context("parse befect_data as a 3-descriptor player.lzs-shaped container")?;
    let section0 = container
        .descriptors
        .first()
        .ok_or_else(|| anyhow::anyhow!("befect_data has no section 0"))?;
    let decoded = legaia_asset::decode(&raw, section0, legaia_asset::DecodeMode::Lzs)
        .context("LZS-decode befect_data section 0")?;
    let pack_entries = legaia_asset::pack::extract_pack(&decoded)
        .context("walk befect_data section 0 as a TMD-pack")?;
    let head = pack_entries
        .into_iter()
        .take(GLOBAL_TMD_POOL_HEAD_COUNT)
        .enumerate();
    for (i, body) in head {
        let tmd = match legaia_tmd::parse(body) {
            Ok(t) => t,
            Err(err) => {
                eprintln!("[scene] befect_data slot {i} did not parse as TMD ({err:#}); skipping");
                continue;
            }
        };
        world.set_global_tmd(
            i,
            std::sync::Arc::new(crate::world::GlobalTmd {
                tmd,
                raw: body.to_vec(),
            }),
        );
    }
    Ok(())
}

/// Seed the battle effect-model library from PROT 0871 (`etmd.dat`) into
/// `World::global_tmd_pool[3..=32]` (retail `DAT_8007C018[3..=32]`).
///
/// PROT 0871 is an uncompressed 30-entry [`legaia_asset::pack`] of Legaia
/// TMDs; the engine walks it directly (no LZS) and parses each entry, mapping
/// pack entry `i` -> pool index [`EFFECT_MODEL_LIBRARY_BASE`]` + i`. This is
/// the library retail loads at battle init (`FUN_800520F0`); the live
/// Tail-Fire RAM confirms these 30 models are resident during a Seru cast
/// while PROT 0874 §0's five TMDs are not - so this supersedes the §0 preview
/// head for the effect-model render path ([`GIMARD_TAIL_FIRE_MODEL_INDEX`] is
/// the flame retail draws).
///
/// Soft-fails (returns `Err`) when the entry is missing or the pack walk
/// fails; entries that don't parse as TMDs are skipped individually. The two
/// overlapping slots (`[3]`, `[4]`) from the PROT 0874 §0 head are overwritten
/// here, matching retail's temporal load order.
pub(crate) fn seed_effect_model_library_from_etmd(
    index: &ProtIndex,
    world: &mut crate::world::World,
) -> Result<()> {
    // The pack body spans PROT 0871's full on-disc footprint (the last TMD
    // sits past the TOC-indexed end), so read the extended footprint - the
    // indexed-only view truncates the pack mid-table.
    let raw = index
        .entry_bytes_extended(PROT_EFFECT_MODEL_LIBRARY_ENTRY)
        .with_context(|| {
            format!(
                "read PROT entry {} (etmd.dat effect-model library)",
                PROT_EFFECT_MODEL_LIBRARY_ENTRY
            )
        })?;
    let pack_entries = legaia_asset::pack::extract_pack(&raw)
        .context("walk PROT 0871 (etmd.dat) as a TMD pack")?;
    let mut loaded = 0usize;
    for (i, body) in pack_entries
        .iter()
        .enumerate()
        .take(EFFECT_MODEL_LIBRARY_COUNT)
    {
        let tmd = match legaia_tmd::parse(body) {
            Ok(t) => t,
            Err(err) => {
                eprintln!("[scene] etmd library slot {i} did not parse as TMD ({err:#}); skipping");
                continue;
            }
        };
        world.set_global_tmd(
            EFFECT_MODEL_LIBRARY_BASE + i,
            std::sync::Arc::new(crate::world::GlobalTmd {
                tmd,
                raw: body.to_vec(),
            }),
        );
        loaded += 1;
    }
    if loaded == 0 {
        anyhow::bail!("etmd library (PROT 0871) carried no parseable TMDs");
    }
    Ok(())
}

/// True when the PROT 0871 effect-model library is already resident in the
/// pool (every slot in `[3..=32]` populated). Used to keep
/// [`seed_effect_model_library_from_etmd`] idempotent across scene
/// transitions, mirroring the field-head guard.
pub(crate) fn effect_model_library_loaded(world: &crate::world::World) -> bool {
    let end = EFFECT_MODEL_LIBRARY_BASE + EFFECT_MODEL_LIBRARY_COUNT;
    world.global_tmd_pool.len() >= end
        && world.global_tmd_pool[EFFECT_MODEL_LIBRARY_BASE..end]
            .iter()
            .all(|s| s.is_some())
}

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
/// rectangle - see the inline note. (`legaia_asset::field_char_textures` is the
/// standalone parser + verifier for the same section, byte-exact against a live
/// field VRAM dump.)
///
/// This makes the texels resident for effect-model rendering. (It does *not*
/// feed the 2D `efect.dat` sprite-atlas billboards, which sample a separate
/// page-`(0,0)` 8bpp source - see [`crate::world::World::active_effect_sprites`]
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
    for target in legaia_asset::befect_cluster::scan_tims(&decoded) {
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
    let container = legaia_asset::parse_player_lzs(&raw, 3)
        .context("parse befect_data as a 3-descriptor player.lzs-shaped container")?;
    let section = container
        .descriptors
        .get(BEFECT_ETIM_SECTION)
        .ok_or_else(|| {
            anyhow::anyhow!("befect_data has no section {BEFECT_ETIM_SECTION} (etim)")
        })?;
    legaia_asset::decode(&raw, section, legaia_asset::DecodeMode::Lzs)
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
    for target in legaia_asset::befect_cluster::scan_tims(&decoded) {
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
        let scan = legaia_asset::tim_scan::scan_entry(&entry.bytes);
        for (source, hit) in &scan.hits {
            let payload: &[u8] = match source {
                legaia_asset::tim_scan::Source::Raw => &entry.bytes,
                legaia_asset::tim_scan::Source::Lzs(idx) => scan.lzs_sections[*idx].as_slice(),
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
    for target in legaia_asset::befect_cluster::scan_tims(&raw) {
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
