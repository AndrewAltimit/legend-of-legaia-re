//! Effect-bundle / global-TMD-pool / VRAM-upload seeding constants and helpers.

use super::*;

/// PROT entry holding the runtime effect buffer `data\battle\efect.dat` - the
/// 2-pack wrapper (inline sprite atlas + pack0 anim batches + pack1 effect
/// scripts) the battle effect VM consumes. Stored uncompressed; the raw entry
/// bytes are byte-identical to the post-init runtime buffer (`docs/formats/effect.md`).
const PROT_EFECT_DAT_ENTRY: u32 = 873;

/// PROT entry holding the global monster stat archive (one `0x14000`-byte
/// LZS slot per monster id; the CDNAME label `battle_data` is shared across
/// 0865-0868). The misleading `monster_data` label (PROT 869) is a stub.
/// See [`legaia_actor_formats::monster_archive`] + `docs/subsystems/battle.md`.
pub(crate) const MONSTER_ARCHIVE_PROT_ENTRY: u32 = 867;

/// Number of slots PROT 0874 section 0 contributes to the head of the
/// global TMD pool. Set by the section's TMD-pack `count` field; the
/// retail pack carries exactly 5 character meshes.
pub(crate) const GLOBAL_TMD_POOL_HEAD_COUNT: usize = 5;

/// Index into [`legaia_engine_core::world::World::global_tmd_pool`] of the PROT 0874 §0
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
/// `container-formats::pack` of Legaia TMDs (`word[0]=30`, every entry magic `0x80000002`),
/// stored uncompressed. Retail registers all 30 verbatim into
/// `DAT_8007C018[3..=32]` at battle init (`FUN_800520F0` debug index `0x367` ->
/// `FUN_80026B4C`); the dev-path name is `h:\prot\battle\etmd.dat`. The CDNAME
/// label `sound_data` is misleading - this is the effect-model library, not
/// audio. See `docs/formats/effect.md`.
const PROT_EFFECT_MODEL_LIBRARY_ENTRY: u32 = 871;

use legaia_engine_core::world::EFFECT_MODEL_LIBRARY_BASE;

/// Number of TMDs in the PROT 0871 effect-model library (`word[0]`).
pub(crate) const EFFECT_MODEL_LIBRARY_COUNT: usize = 30;

/// Index in [`legaia_engine_core::world::World::global_tmd_pool`] of Gimard's *Tail Fire*
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
    world: &mut legaia_engine_core::world::World,
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
    world: &mut legaia_engine_core::world::World,
) -> Result<()> {
    let raw = index
        .entry_bytes(PROT_BEFECT_DATA_ENTRY)
        .with_context(|| format!("read PROT entry {} (befect_data)", PROT_BEFECT_DATA_ENTRY))?;
    let container = legaia_container_formats::parse_player_lzs(&raw, 3)
        .context("parse befect_data as a 3-descriptor player.lzs-shaped container")?;
    let section0 = container
        .descriptors
        .first()
        .ok_or_else(|| anyhow::anyhow!("befect_data has no section 0"))?;
    let decoded =
        legaia_container_formats::decode(&raw, section0, legaia_container_formats::DecodeMode::Lzs)
            .context("LZS-decode befect_data section 0")?;
    let pack_entries = legaia_container_formats::pack::extract_pack(&decoded)
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
        let g = std::sync::Arc::new(legaia_engine_scene::model_bank::GlobalTmd {
            tmd,
            raw: body.to_vec(),
        });
        if world.field_head_pool.len() <= i {
            world.field_head_pool.resize(i + 1, None);
        }
        world.field_head_pool[i] = Some(g.clone());
        // Never clobber a pool slot something already holds (the effect-model
        // library's `[3]` / `[4]` on a re-seed of the player bank alone).
        if world.global_tmd_pool.get(i).is_none_or(|s| s.is_none()) {
            world.set_global_tmd(i, g);
        }
    }
    Ok(())
}

/// Seed the battle effect-model library from PROT 0871 (`etmd.dat`) into
/// `World::global_tmd_pool[3..=32]` (retail `DAT_8007C018[3..=32]`).
///
/// PROT 0871 is an uncompressed 30-entry [`legaia_container_formats::pack`] of Legaia
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
    world: &mut legaia_engine_core::world::World,
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
    let pack_entries = legaia_container_formats::pack::extract_pack(&raw)
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
            std::sync::Arc::new(legaia_engine_scene::model_bank::GlobalTmd {
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

/// PROT entry carrying `vdf.dat`, the battle VDF morph pack
/// (`docs/formats/effect.md`: case `0xb` of `FUN_800520F0`).
const PROT_BATTLE_VDF_ENTRY: u32 = 872;

/// Load the battle VDF morph pack (PROT 0872, `vdf.dat`) into
/// [`legaia_engine_core::world::DiscTables::battle_vdf`] - what a battle effect part's
/// op-`0x0A` morph lanes index (the Spirit charge's aura cones are lanes on
/// its entry 12). Retail loads it at battle init beside `etmd.dat`; the engine
/// keeps it resident like the effect-model library. Soft-fails on a missing
/// entry or a header that is not a VDF offset pack.
pub(crate) fn seed_battle_vdf(
    index: &ProtIndex,
    world: &mut legaia_engine_core::world::World,
) -> Result<()> {
    let raw = index
        .entry_bytes(PROT_BATTLE_VDF_ENTRY)
        .with_context(|| format!("read PROT entry {PROT_BATTLE_VDF_ENTRY} (vdf.dat)"))?;
    anyhow::ensure!(
        legaia_engine_scene::scene_bundle::vdf_entry(&raw, 0).is_some(),
        "PROT {PROT_BATTLE_VDF_ENTRY} is not a VDF offset pack"
    );
    world.tables.battle_vdf = Some(std::sync::Arc::from(raw.as_slice()));
    Ok(())
}

/// True when the PROT 0871 effect-model library is already resident in the
/// pool (every slot in `[3..=32]` populated). Used to keep
/// [`seed_effect_model_library_from_etmd`] idempotent across scene
/// transitions, mirroring the field-head guard.
pub(crate) fn effect_model_library_loaded(world: &legaia_engine_core::world::World) -> bool {
    let end = EFFECT_MODEL_LIBRARY_BASE + EFFECT_MODEL_LIBRARY_COUNT;
    world.global_tmd_pool.len() >= end
        && world.global_tmd_pool[EFFECT_MODEL_LIBRARY_BASE..end]
            .iter()
            .all(|s| s.is_some())
}
