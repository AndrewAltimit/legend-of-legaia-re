//! Top-level engine boot session.
//!
//! Composes the per-crate primitives ([`legaia_engine_core::scene::SceneHost`],
//! [`legaia_engine_core::camera::Camera`], the BGM director from
//! [`crate::bgm::AudioBgmDirector`]) into one struct the binary drives per
//! frame. Mirrors the retail boot flow:
//!
//! 1. Open the extracted PROT + CDNAME map.
//! 2. Load a starting scene (the binary defaults to `town01`).
//! 3. Pick the scene's primary VAB bank, upload it to the SPU, and stash
//!    in the BGM director for subsequent op-`0x35` triggers.
//! 4. Drive the world tick + camera tick + event routing each frame.
//!
//! No window / renderer here - the binary owns winit + wgpu (or in headless
//! CI mode, no window). [`BootSession::tick`] is the per-frame driver
//! callable from either path.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use legaia_engine_audio::{AudioOut, Spu, SpuAllocator, VabBank};
use legaia_engine_core::camera::Camera;
use legaia_engine_core::field_menu::{FieldMenuInput, FieldMenuSession};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::{BgmDirector, DefaultMapIdResolver, SceneHost, SceneTickEvent};
use legaia_engine_core::world::SceneMode;

use crate::bgm::AudioBgmDirector;

/// Options for [`BootSession::enter_field_live`] - how much of the live
/// gameplay loop to arm when dropping into a field scene.
#[derive(Debug, Clone, Default)]
pub struct FieldLiveOpts {
    /// Arm the Field<->Battle live gameplay loop (`World::live_gameplay_loop`).
    /// `player_battle` implies this.
    pub live_loop: bool,
    /// Make battles player-driven (command menu). Implies `live_loop` and
    /// installs the Seru-learning registry a player-driven battle needs. (The
    /// item / spell / equipment catalogs are installed unconditionally now -
    /// the field pause-menu reads them regardless of these flags.)
    pub player_battle: bool,
    /// Optional Battle<->Field BGM swap id (resolved through the scene's BGM
    /// table by the live loop).
    pub battle_bgm: Option<u16>,
}

/// Default scene the binary boots into when no `--scene` is supplied. Uses
/// the canonical first-town label from CDNAME.TXT.
pub const DEFAULT_BOOT_SCENE: &str = "town01";

/// Total SPU RAM in bytes (PSX hardware constant).
pub(crate) const SPU_RAM_BYTES: u32 = 512 * 1024;
/// Byte offset reserved for voice-0 / scratchpad - banks are allocated
/// above this. Mirrors the asset-viewer SEQ playback path.
pub(crate) const SPU_RESERVED_BYTES: u32 = 0x1000;
/// SPU RAM reserved at the TOP of the map for the resident class-2 SFX bank
/// (PROT 0869). Its VAG bodies total ~184 KiB, so a 192 KiB window holds it
/// with headroom. On real hardware the SFX bank and the BGM VAB coexist in
/// the 512 KiB SPU RAM; carving a dedicated top region models that so a
/// scene-BGM upload can't stomp the SFX samples. The BGM region is capped
/// below it (staged scene VABs run well under the remaining ~316 KiB).
pub(crate) const SFX_BANK_SPU_BYTES: u32 = 0x30000;
/// The class-2 SFX program bank the battle scene loader (`FUN_800520F0`,
/// `a1 = 2`) and the Baka Fighter init (`FUN_801CF00C`) load explicitly -
/// extraction PROT 0869 (raw loader index `0x367`). Its low programs carry
/// the battle strike + duel-hit cues. See `docs/formats/sfx-table.md`.
const SFX_BANK_PROT_INDEX: u32 = 869;

/// One-time configuration for [`BootSession::open`].
#[derive(Debug, Clone)]
pub struct BootConfig {
    /// Starting scene name (CDNAME label).
    pub scene: String,
    /// Whether to open the audio output. Set `false` for headless tests
    /// (cpal will fail to enumerate devices in CI).
    pub enable_audio: bool,
}

impl Default for BootConfig {
    fn default() -> Self {
        Self {
            scene: DEFAULT_BOOT_SCENE.to_string(),
            enable_audio: true,
        }
    }
}

/// Source of PROT.DAT + CDNAME.TXT bytes for a [`BootSession::open*`]
/// call. Internal - public construction is via the typed entry points
/// [`BootSession::open`] and [`BootSession::open_disc`].
enum SceneSource<'a> {
    Extracted(&'a Path),
    #[cfg(not(target_arch = "wasm32"))]
    Disc(&'a Path),
}

/// Per-frame session bundle. The binary owns one of these and calls
/// [`tick`](Self::tick) every frame.
pub struct BootSession {
    pub host: SceneHost,
    pub camera: Camera,
    pub audio: Option<Arc<AudioOut>>,
    pub bgm: Option<AudioBgmDirector>,
    /// Wall-clock frame counter, separate from `host.world.frame` (which
    /// includes pause-time skips when those land).
    pub frames: u64,
    /// New-game starting-party template parsed from the boot source's
    /// `SCUS_942.54`, if present. Used by [`BootSession::begin_new_game`] to
    /// seed a faithful starting roster; `None` when the executable couldn't be
    /// read (e.g. a raw PROT.DAT-only source), in which case New Game keeps the
    /// world's default scaffold party.
    pub starting_party: Option<legaia_asset::new_game::StartingParty>,
    /// New-game starting inventory decoded from the boot source's `SCUS_942.54`
    /// seed code (`FUN_80034A6C`), if present. Vanilla retail is Healing Leaf
    /// ×5; the starting-item randomizer rewrites it. Used by
    /// [`BootSession::begin_new_game`] to seed the opening bag faithfully.
    pub starting_inventory: Option<legaia_asset::new_game::StartingInventory>,
    /// Disc-accurate equipment modifier table keyed by real item ids, parsed
    /// from the boot source's `SCUS_942.54` ([`legaia_asset::equip_stats`]).
    /// Preferred over the fabricated-id vanilla catalog when installing the
    /// battle-stat equipment table; `None` on disc-free builds.
    pub equip_modifier_table: Option<legaia_engine_core::battle_stats::EquipmentTable>,
    /// Disc-pinned equip restrictions (character mask `+6` + slot category
    /// `+7`) keyed by real item ids, parsed from the same equipment stat-bonus
    /// table ([`legaia_asset::equip_stats`]). Drives the equip screen's
    /// per-character item gate
    /// ([`legaia_engine_core::equip_session::EquipSession::new_with_restrictions`]);
    /// `None` on disc-free builds.
    pub equip_restrictions: Option<legaia_engine_core::equipment::DiscEquipInfo>,
    /// Player Seru-magic catalog with MP cost + target shape read from the
    /// boot source's `SCUS_942.54` spell table ([`legaia_asset::spell_names`]).
    /// Preferred over the pinned `retail_seru_magic_catalog` when installing the
    /// battle spell catalog, so a randomized / translated disc is honoured;
    /// `None` on disc-free builds.
    pub spell_catalog: Option<legaia_engine_core::spells::SpellCatalog>,
    /// The real retail proportional dialog font, decoded straight from the
    /// boot source (`PROT.DAT`'s 4bpp font TIM + the `SCUS_942.54` advance
    /// table at `0x80073F1C`) - **no mednafen save state required**.
    ///
    /// Every native text draw goes through this font's metrics, and retail's
    /// glyph advance is proportional (`pen_x += widths[c] + 1`, pinned at
    /// `FUN_80036888` body `0x80036B9C`). The `extracted/font/` artifacts the
    /// legacy loader wants only exist after a `font-extract` run, which needs
    /// a save state - so a disc-only boot used to silently fall back to the
    /// fixed-width placeholder and rendered every string ~35% too wide. This
    /// field is the disc-derived fallback that keeps a plain
    /// `--disc <image>` boot on the retail metrics. `None` when the source
    /// carries neither the font TIM nor the executable.
    pub dialog_font: Option<legaia_font::Font>,
    /// In-field pause-menu session, when open. Retail runs the pause menu
    /// under the CARD mode pair (`_DAT_8007B83C = 0x17`, `CARD MODE`, in
    /// every menu-open capture); the session-hosted equivalent holds
    /// [`World::mode`](legaia_engine_core::world::World::mode) at
    /// [`SceneMode::Menu`] while `Some`, suspending field dispatch
    /// underneath. Opened via [`BootSession::open_field_menu`] (or the
    /// Start-edge path inside [`BootSession::tick`]); the windowed host
    /// layers its sub-session UI stack on top of this same session.
    pub field_menu: Option<FieldMenuSession>,
    /// Scene mode the world ran before the pause menu opened, restored by
    /// [`BootSession::close_field_menu`].
    field_menu_resume: SceneMode,
}

/// Read + parse the new-game starting-party template from a boot source's
/// `SCUS_942.54`. Returns `None` (not an error) when the executable isn't
/// reachable or doesn't parse, so a boot never fails just because the seed
/// data is unavailable.
fn read_starting_party(source: &SceneSource<'_>) -> Option<legaia_asset::new_game::StartingParty> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    legaia_asset::new_game::StartingParty::from_scus(&scus)
}

/// Read + decode the new-game starting-inventory seed from a boot source's
/// `SCUS_942.54` (`FUN_80034A6C`). Returns `None` when the executable isn't
/// reachable or doesn't decode, so a boot never fails on missing seed data.
fn read_starting_inventory(
    source: &SceneSource<'_>,
) -> Option<legaia_asset::new_game::StartingInventory> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    legaia_asset::new_game::StartingInventory::from_scus(&scus)
}

/// Read + parse the retail XP-to-next-level curve from a boot source's
/// `SCUS_942.54` (the static `DAT_80076AF4` table + `FUN_801E9504`'s formula).
/// Returns `None` (not an error) when the executable isn't reachable, so the
/// tracker keeps its placeholder curve rather than failing the boot.
fn read_retail_xp_curve(source: &SceneSource<'_>) -> Option<(Vec<u32>, Option<Vec<i16>>)> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    let curve = legaia_asset::level_up_tables::xp_thresholds_from_scus(&scus)?;
    // The slots-1/2 threshold-correction divisor table rides along: its
    // runtime pointer (_DAT_8007B81C) is constant across the whole save
    // corpus, so the table is plain static SCUS data.
    let corrections = legaia_asset::level_up_tables::xp_correction_divisors_from_scus(&scus);
    Some((curve, corrections))
}

/// Read + parse the static-SCUS per-character stat-growth tables (`DAT_800769CC`
/// curves + `DAT_80076918` parameter block) read by `FUN_801E9504`. Returns
/// `None` (not an error) when the executable isn't reachable, so the tracker
/// keeps its flat-rate placeholder growth rather than failing the boot.
fn read_retail_growth_tables(
    source: &SceneSource<'_>,
) -> Option<legaia_asset::level_up_tables::GrowthTables> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    legaia_asset::level_up_tables::growth_tables_from_scus(&scus)
}

/// Read the raw `SCUS_942.54` bytes from a boot source. Returns `None`
/// (not an error) when the executable isn't reachable.
fn read_scus(source: &SceneSource<'_>) -> Option<Vec<u8>> {
    use legaia_engine_core::Vfs;
    match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok(),
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok(),
    }
}

/// Build the retail proportional dialog font straight from the boot source:
/// the 4bpp font TIM at [`legaia_font::FONT_TIM_PROT_DAT_OFFSET`] inside
/// `PROT.DAT` supplies the glyph bitmaps and `SCUS_942.54` the per-character
/// advance table (`0x80073F1C`).
///
/// This is the disc-only path - it needs no `extracted/font/` artifacts and no
/// save state, so a `--disc <image>` boot renders text on retail metrics
/// instead of the fixed-width placeholder. Returns `None` (never an error)
/// when either half is unreachable.
fn read_dialog_font(
    index: &legaia_engine_core::scene::ProtIndex,
    source: &SceneSource<'_>,
) -> Option<legaia_font::Font> {
    let tim = index
        .prot_dat_raw_bytes(
            legaia_font::FONT_TIM_PROT_DAT_OFFSET,
            legaia_font::FONT_TIM_LEN,
        )
        .ok()?;
    let scus = read_scus(source)?;
    legaia_font::Font::from_disc_tim_and_scus(&tim, &scus).ok()
}

/// Read + decode the sound-effect descriptor bank from a boot source's
/// `SCUS_942.54` (`DAT_8006F198`, see `sfx-table.md`). Returns `None` when the
/// executable isn't reachable or the table doesn't decode, so a boot never
/// fails on missing SFX data - the director just keeps its empty bank and
/// resolved cues no-op until one is staged.
fn read_sfx_bank(source: &SceneSource<'_>) -> Option<legaia_engine_audio::SfxBank> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    let table = legaia_asset::sfx_table::SfxTable::from_scus(&scus)?;
    Some(legaia_engine_audio::SfxBank::from_descriptors(
        table
            .active()
            .map(|(id, d)| (id, d.program, d.tone, d.note, d.voice_count())),
    ))
}

/// Demux + decode the battle **arts-voice shout** banks from a disc image:
/// the per-character CD-XA clip files (`XA2.XA` Vahn / `XA4.XA` Noa /
/// `XA6.XA` Gala, 16-channel short-mono banks) plus the `SCUS_942.54`
/// cue tables that map each art's action constant to its candidate-channel
/// pool (`legaia_art::arts_voice`, the `FUN_8004C140` tables).
///
/// Channel demux needs the raw 2352-byte sectors (the CD-XA subheaders carry
/// the channel number; a 2048-byte ISO view strips them), so this reads the
/// disc through [`legaia_iso::raw::RawDisc`] - extracted-directory boots
/// can't stage a shout bank. Returns `None` when the disc / executable /
/// tables don't resolve; the caller degrades to silent arts.
///
/// Public so disc-gated tests can build the same bank the boot path stages.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_arts_shout_bank(disc: &Path) -> Option<legaia_engine_audio::ArtsShoutBank> {
    use legaia_engine_audio::{ArtsShoutBank, ShoutClip};
    let scus = read_scus(&SceneSource::Disc(disc))?;
    let table = legaia_art::arts_voice::ArtsVoiceTable::parse_from_scus(&scus)?;
    let mut raw = legaia_iso::raw::RawDisc::open(disc).ok()?;
    let volume = legaia_iso::iso9660::read_volume(&mut raw).ok()?;
    let files = legaia_iso::iso9660::walk_files(&mut raw, &volume.root).ok()?;
    let mut bank = ArtsShoutBank::new();
    for cslot in 0u8..3 {
        let name = legaia_art::arts_voice::clip_file(cslot as usize)?;
        // ISO paths look like `XA/XA2.XA;1` - match on the file name.
        let rec = files.iter().find_map(|(path, rec)| {
            let base = path.rsplit('/').next().unwrap_or(path);
            let base = base.split(';').next().unwrap_or(base);
            base.eq_ignore_ascii_case(name).then_some(rec)
        })?;
        let sectors = rec.size.div_ceil(legaia_iso::raw::USER_DATA_SIZE as u32);
        let streams = legaia_xa::demux::demux_disc_range(&mut raw, rec.lba, sectors).ok()?;
        for s in &streams {
            // The shout banks are 4-bit mono; skip anything else (a stereo or
            // 8-bit stream here would be a mis-identified file).
            if s.stereo || s.bits_per_sample != 4 {
                continue;
            }
            let (pcm, _) = legaia_xa::decode(
                &s.audio,
                legaia_xa::DecodeOptions {
                    channels: legaia_xa::Channels::Mono,
                    sample_rate: s.sample_rate,
                    bits: legaia_xa::BitsPerSample::Four,
                },
            )
            .ok()?;
            // Trim the trailing channel-padding silence so a clip's audible
            // end matches the retail read-span cutoff closely enough for the
            // back-to-back promotion queue.
            let mut end = pcm.len();
            while end > 0 && pcm[end - 1].unsigned_abs() < 8 {
                end -= 1;
            }
            let mut pcm = pcm;
            pcm.truncate(end);
            if pcm.is_empty() {
                continue;
            }
            bank.insert_clip(
                cslot,
                s.ch_no,
                ShoutClip {
                    pcm,
                    sample_rate: s.sample_rate,
                },
            );
        }
        for (action, pool) in table.pools(cslot as usize) {
            bank.set_pool(cslot, action, pool.to_vec());
        }
    }
    bank.has_clips().then_some(bank)
}

/// Read the gold-shop item data (per-id buy price + "names a real item" mask)
/// from a boot source's `SCUS_942.54` item table. Returns `None` when the
/// executable isn't reachable or its item table doesn't parse, so a boot never
/// fails on missing shop data - the engine then leaves shop stock host-supplied
/// and unpriced. See [`legaia_engine_core::shop_catalog`].
fn read_shop_item_data(
    source: &SceneSource<'_>,
) -> Option<legaia_engine_core::shop_catalog::ShopItemData> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    legaia_engine_core::shop_catalog::ShopItemData::from_scus(&scus)
}

/// Read + parse the static item-effect descriptor table (`DAT_800752C0`, see
/// `item-effect-table.md`) from a boot source's `SCUS_942.54`. Returns `None`
/// when the executable isn't reachable or the table doesn't parse, so a boot
/// never fails on missing item-effect data - the engine then keeps the curated
/// usability flags on its item catalog.
fn read_retail_item_effects(
    source: &SceneSource<'_>,
) -> Option<legaia_asset::item_effect::ItemEffectTable> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    legaia_asset::item_effect::ItemEffectTable::from_scus(&scus)
}

/// Read + parse the accessory ("Goods") passive-effect tables (the
/// descriptor-`+3` / equip-`+5` index bytes + the `0x8007625C` scope records,
/// see `accessory-passive-table.md`) from a boot source's `SCUS_942.54` and
/// build the engine catalog
/// ([`legaia_engine_core::accessory_passives::AccessoryPassives`]). Returns
/// `None` when the executable isn't reachable or the tables don't parse, so a
/// boot never fails on missing passive data - the engine then grants no
/// accessory passives (the disc-free default).
fn read_accessory_passives(
    source: &SceneSource<'_>,
) -> Option<legaia_engine_core::accessory_passives::AccessoryPassives> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    let table = legaia_asset::accessory_passive::AccessoryPassiveTable::from_scus(&scus)?;
    Some(legaia_engine_core::accessory_passives::AccessoryPassives::from_disc(&table))
}

/// Read the static equipment stat-bonus table (`DAT_80074F68`, see
/// `equipment-table.md`) from a boot source's `SCUS_942.54` and build both the
/// disc-accurate equipment modifier table (stat bonuses keyed by real item ids)
/// and the per-item equip restrictions (character mask `+6` + slot category
/// `+7`) from the single parse. Returns `None` when the executable isn't
/// reachable or the table doesn't parse, so a boot never fails on missing
/// equipment data - the engine then falls back to the (fabricated-id) vanilla
/// equipment catalog.
fn read_retail_equip_tables(
    source: &SceneSource<'_>,
) -> Option<(
    legaia_engine_core::battle_stats::EquipmentTable,
    legaia_engine_core::equipment::DiscEquipInfo,
)> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    let table = legaia_asset::equip_stats::EquipStatTable::from_scus(&scus)?;
    let modifiers = legaia_engine_core::equipment::equip_modifier_table_from_disc(&table);
    let restrictions = legaia_engine_core::equipment::DiscEquipInfo::from_disc(&table);
    Some((modifiers, restrictions))
}

/// Read the player Seru-magic catalog (MP cost + target shape from the spell
/// table, see `spell-table.md`) from a boot source's `SCUS_942.54`. Returns
/// `None` when the executable isn't reachable or doesn't parse, so a boot falls
/// back to the pinned `retail_seru_magic_catalog`.
fn read_retail_spell_catalog(
    source: &SceneSource<'_>,
) -> Option<legaia_engine_core::spells::SpellCatalog> {
    use legaia_engine_core::Vfs;
    let scus = match source {
        SceneSource::Extracted(root) => legaia_engine_core::DirVfs::new(*root)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
        #[cfg(not(target_arch = "wasm32"))]
        SceneSource::Disc(path) => legaia_engine_core::DiscVfs::open(path)
            .ok()?
            .read("SCUS_942.54")
            .ok()?,
    };
    legaia_engine_core::retail_magic::seru_magic_catalog_from_scus(&scus)
}

impl BootSession {
    /// Open an extracted disc tree and load the configured scene. Errors if
    /// the directory isn't an extracted PROT or the scene name isn't in
    /// CDNAME.TXT.
    pub fn open(extracted_root: &Path, cfg: &BootConfig) -> Result<Self> {
        Self::open_with_source(SceneSource::Extracted(extracted_root), cfg)
    }

    /// Open the engine straight from a `.bin` disc image. The disc is walked
    /// once to extract `PROT.DAT` and `CDNAME.TXT`; no on-disk extraction
    /// step is required. Native targets only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_disc(disc_bin: &Path, cfg: &BootConfig) -> Result<Self> {
        Self::open_with_source(SceneSource::Disc(disc_bin), cfg)
    }

    fn open_with_source(source: SceneSource<'_>, cfg: &BootConfig) -> Result<Self> {
        // Parse the new-game starting-party template from the same source
        // (best-effort; never fails the boot).
        let starting_party = read_starting_party(&source);
        let starting_inventory = read_starting_inventory(&source);
        let (equip_modifier_table, equip_restrictions) = match read_retail_equip_tables(&source) {
            Some((m, r)) => (Some(m), Some(r)),
            None => (None, None),
        };
        let spell_catalog = read_retail_spell_catalog(&source);
        let mut host = match source {
            SceneSource::Extracted(root) => SceneHost::open_extracted(root)
                .with_context(|| format!("open extracted dir {}", root.display()))?,
            #[cfg(not(target_arch = "wasm32"))]
            SceneSource::Disc(path) => SceneHost::open_disc(path)
                .with_context(|| format!("open disc image {}", path.display()))?,
        };
        // Wire the CDNAME-derived map-id resolver so field-VM scene
        // transitions resolve to the right CDNAME label.
        host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));

        // Retail proportional dialog font off the disc (no save state). See
        // `BootSession::dialog_font`.
        let dialog_font = read_dialog_font(&host.index, &source);
        if dialog_font.is_none() {
            log::warn!(
                "dialog font not decodable from the boot source; \
                 text falls back to extracted/ or the placeholder"
            );
        }

        // Hand the host the retail new-game defaults so a cold `--scene X`
        // boot (no New Game confirm, no save loaded) seeds the template party
        // + starting bag at scene entry instead of leaving a zeroed scaffold
        // roster behind the pause menu. Guarded inside `enter_field_scene` -
        // it never fires once a party or save is installed.
        host.new_game_defaults =
            starting_party
                .clone()
                .map(|party| legaia_engine_core::new_game::NewGameDefaults {
                    party,
                    inventory: starting_inventory.clone(),
                });

        // Install the real retail XP curve (static SCUS table + FUN_801E9504
        // formula) over the tracker's fabricated sin-LUT placeholder, when the
        // executable is reachable, together with the slots-1/2 threshold
        // correction divisors (Noa levels slightly earlier, Gala slightly
        // later). Set once at boot; begin_new_game doesn't reset the tracker,
        // so it persists across New Game.
        if let Some((curve, corrections)) = read_retail_xp_curve(&source) {
            host.world.level_up_tracker.xp_table = curve;
            host.world.level_up_tracker.xp_corrections = corrections;
        }

        // Install the real per-character HP/MP growth curves (static SCUS
        // DAT_800769CC / DAT_80076918 via FUN_801E9504's validated jitter-free
        // core) over the flat 10/5 placeholder, when the executable is
        // reachable. Persists across New Game like the XP curve.
        if let Some(tables) = read_retail_growth_tables(&source) {
            let tracker = std::mem::take(&mut host.world.level_up_tracker);
            host.world.level_up_tracker = tracker.with_growth_tables(&tables);
        }

        // Install the summon-magic spell-XP level-up thresholds (the static
        // SCUS table the battle overlay's level-up check reads) so Seru-magic
        // casts accrue spell XP against the retail curve and level the
        // record's spell-level byte. Best-effort: absent on disc-free builds,
        // where no spell XP accrues. Persists across New Game.
        if let Some(scus) = read_scus(&source) {
            host.world.install_magic_xp_thresholds(&scus);
            // Pause-menu text: item names + info-window descriptions,
            // spell names / descriptions, accessory passive lines. The
            // Items / Magic pause screens resolve their strings here.
            host.world.install_menu_text(&scus);
            // Install the randomizer's seru-trade config (the `--seru-trade`
            // blob in preserved rodata). No-op / disabled on a vanilla disc;
            // when present, vendors offer seru-for-seru trades. Persists across
            // New Game.
            host.world.install_seru_trade_config(&scus);
        }

        // Install the gold-shop item data (per-id buy price + name mask) from the
        // SCUS item table, so each field scene's merchant offers its real stock
        // at real prices (populated per scene by `enter_field_scene`). Persists
        // across New Game; absent on disc-free builds (stock stays host-supplied).
        if let Some(shop_data) = read_shop_item_data(&source) {
            host.world.item_shop_data = Some(shop_data);
        }

        // Install the real item-effect descriptor table so the item catalog's
        // field/battle usability gating matches retail (e.g. cure/revive items
        // are battle-only). Best-effort: absent on disc-free builds, where the
        // catalog keeps its curated usability flags.
        if let Some(effects) = read_retail_item_effects(&source) {
            host.world.set_item_effects(effects);
        }

        // Install the accessory ("Goods") passive-effect catalog so equipped
        // accessories grant their ability bits (MP savers, guards, party-wide
        // reward/encounter modifiers) and percent stat boosts through
        // `World::refresh_party_ability_bits` / `seed_party_battle_stats`.
        // Best-effort: absent on disc-free builds, where no passives fire.
        if let Some(passives) = read_accessory_passives(&source) {
            host.world.set_accessory_passives(passives);
        }

        host.load_scene(&cfg.scene)
            .with_context(|| format!("load scene '{}'", cfg.scene))?;

        // Audio + BGM director (optional - disabled for headless tests).
        let (audio, bgm) = if cfg.enable_audio {
            match AudioOut::new() {
                Ok(audio) => {
                    // AudioOut owns a cpal::Stream which is Send but not Sync.
                    // BootSession is single-threaded (binary + WASM both
                    // tick on one thread); the Arc just gives the BGM
                    // director a refcounted handle.
                    #[allow(clippy::arc_with_non_send_sync)]
                    let audio = Arc::new(audio);
                    let mut director = AudioBgmDirector::new(audio.clone());
                    if let Err(e) = stage_scene_vab(&mut director, audio.as_ref(), &host) {
                        log::warn!("BGM bank not staged (scene VAB resolution failed): {e:#}");
                    }
                    // Decode the static SFX descriptor bank from the same
                    // executable once; it names the program/tone/voice-count
                    // for each cue id. Best-effort - an empty bank just no-ops
                    // resolved cues.
                    if let Some(sfx) = read_sfx_bank(&source) {
                        director.set_sfx_bank(sfx);
                    }
                    // Stage the resident class-2 SFX program bank (PROT 0869)
                    // into its own SPU region so battle / minigame cues resolve
                    // against the bank the retail battle loader loads, not
                    // whatever BGM VAB is open. Best-effort.
                    if let Err(e) = stage_sfx_vab(&mut director, audio.as_ref(), &host) {
                        log::warn!(
                            "class-2 SFX bank (PROT {SFX_BANK_PROT_INDEX}) not staged: {e:#}"
                        );
                    }
                    // Demux + decode the arts-voice shout banks (XA2/XA4/XA6)
                    // and the SCUS cue tables. Disc-image boots only (channel
                    // demux needs the raw CD-XA subheaders). Best-effort - an
                    // absent bank leaves arts silent.
                    #[cfg(not(target_arch = "wasm32"))]
                    if let SceneSource::Disc(path) = &source {
                        match read_arts_shout_bank(path) {
                            Some(bank) => director.set_shout_bank(bank),
                            None => log::warn!("arts-voice shout bank not staged"),
                        }
                    }
                    (Some(audio), Some(director))
                }
                Err(e) => {
                    log::warn!("audio disabled - open failed: {e:#}");
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        Ok(Self {
            host,
            camera: Camera::default(),
            audio,
            bgm,
            frames: 0,
            starting_party,
            starting_inventory,
            equip_modifier_table,
            equip_restrictions,
            spell_catalog,
            dialog_font,
            field_menu: None,
            field_menu_resume: SceneMode::Field,
        })
    }

    /// Begin a New Game: clear the world to a fresh slate
    /// ([`legaia_engine_core::world::World::begin_new_game`]) and seed the
    /// starting party (Vahn) from the boot source's `SCUS_942.54` template.
    ///
    /// Mirrors the retail NEW GAME → field-launch chain (master mode 2 → 3,
    /// see `docs/subsystems/boot.md`). The opening scene
    /// ([`legaia_asset::new_game::OPENING_CUTSCENE_SCENE`] = `opdeene`, the
    /// prologue cutscene, which hands off to `town01`) is entered through the
    /// usual [`BootSession::enter_field_live`] path; this call only resets and
    /// seeds the world state. When the SCUS template isn't available the world
    /// keeps its default scaffold party so the slice stays runnable.
    pub fn begin_new_game(&mut self) {
        self.host.world.begin_new_game();
        if let Some(starting) = &self.starting_party {
            self.host.world.seed_starting_party(starting);
        }
        if let Some(inv) = &self.starting_inventory {
            self.host.world.seed_starting_inventory(inv);
        }
    }

    /// Open the in-field pause menu (the retail Start-press path into the
    /// CARD mode pair, `game_mode 0x17`). Builds a [`FieldMenuSession`]
    /// seeded with the world's money + play time - the same construction the
    /// windowed host uses - then remembers the current
    /// [`SceneMode`] and switches the world into [`SceneMode::Menu`], so
    /// field dispatch suspends while the menu owns the frame. Idempotent
    /// while a menu is already open.
    pub fn open_field_menu(&mut self) {
        if self.field_menu.is_some() {
            return;
        }
        let world = &mut self.host.world;
        let mut session = FieldMenuSession::new();
        session.money = world.money.max(0) as u32;
        session.play_time_seconds = world.play_time_seconds;
        self.field_menu_resume = world.mode;
        world.mode = SceneMode::Menu;
        self.field_menu = Some(session);
    }

    /// Close the pause menu and restore the suspended scene mode (the mode
    /// the world ran when [`Self::open_field_menu`] fired). No-op when no
    /// menu is open.
    pub fn close_field_menu(&mut self) {
        if self.field_menu.take().is_some() {
            self.host.world.mode = self.field_menu_resume;
        }
    }

    /// Whether the in-field pause menu is open (the engine equivalent of
    /// retail `game_mode 0x17`; [`World::mode`](legaia_engine_core::world::World::mode)
    /// is [`SceneMode::Menu`] while `true`).
    pub fn field_menu_is_open(&self) -> bool {
        self.field_menu.is_some()
    }

    /// Start a **global-pool** `music_01` track (`bgm_id >= 2000`) through the
    /// BGM director: resolve the bank entry, upload its own VAB, and play its
    /// SEQ. This is how a minigame (or any caller with a disc-pinned track id)
    /// starts music that doesn't live in the current scene's sound bank -
    /// the dance overlay's chart loops, the Baka Fighter overture, the Muscle
    /// Dome battle theme. Returns `false` when audio is off, the id isn't a
    /// bank slot, or the entry doesn't decode. The slot machine + fishing
    /// deliberately don't call this: retail inherits the host scene's BGM.
    pub fn start_global_bgm(&mut self, bgm_id: u16) -> bool {
        let Ok(Some(entry)) = self.host.music_bank_entry_bytes(bgm_id) else {
            return false;
        };
        let Some(bgm) = self.bgm.as_mut() else {
            return false;
        };
        bgm.start_owned_vab(bgm_id, &entry);
        true
    }

    /// Restart the field scene's BGM after a minigame that took over the
    /// director with its own global track (dance / Baka Fighter / Muscle
    /// Dome). Re-plays whatever op-`0x35` track the scene had running
    /// ([`World::current_bgm`](legaia_engine_core::world::World::current_bgm)),
    /// re-uploading its VAB. No-op when the scene had no track or it isn't a
    /// global-pool id. The slot machine + fishing don't need this: they never
    /// replaced the director's bank.
    pub fn restore_field_bgm(&mut self) {
        if let Some(id) = self.host.world.current_bgm {
            self.start_global_bgm(id);
        }
    }

    /// One per-frame step: tick the world, route field-VM camera + BGM
    /// events, advance the camera follow, return the [`SceneTickEvent`] for
    /// engines that want to react to scene transitions.
    pub fn tick(&mut self) -> Result<SceneTickEvent> {
        // In-field pause menu (retail CARD pair, game_mode 0x17). Mirror the
        // windowed host's Start-edge open from the field, then drive an open
        // menu from the same pad edges; field dispatch is suspended
        // (SceneMode::Menu) while the menu owns the frame. The windowed host
        // never reaches this auto-path (it handles the Start edge itself and
        // skips `tick` while its boot-UI owns the frame), so the two hosts
        // can't double-drive the session.
        let menu_opened_this_tick = if self.field_menu.is_none()
            && matches!(self.host.world.mode, SceneMode::Field)
            && self.host.world.input.just_pressed(PadButton::Start)
        {
            self.open_field_menu();
            true
        } else {
            false
        };
        if !menu_opened_this_tick && self.field_menu.is_some() {
            let pad = &self.host.world.input;
            let input = FieldMenuInput {
                up: pad.just_pressed(PadButton::Up),
                down: pad.just_pressed(PadButton::Down),
                cross: pad.just_pressed(PadButton::Cross),
                circle: pad.just_pressed(PadButton::Circle),
                start: pad.just_pressed(PadButton::Start),
            };
            let close = {
                let menu = self.field_menu.as_mut().expect("field_menu is Some");
                let _ = menu.tick(input);
                // Headless hosting has no sub-session UI stack: a confirmed
                // row suspends the menu awaiting one, so resume straight back
                // into browsing. Windowed hosts drive the session themselves
                // and push the real sub-session instead.
                if menu.is_suspended() {
                    let _ = menu.resume(false);
                }
                menu.outcome().is_some()
            };
            if close {
                self.close_field_menu();
            }
        }
        // Snap the camera controller back to the follow default whenever the
        // field is in free-roam. A cutscene's op-0x45 Camera Configure events
        // leave `self.camera` in Cinematic mode at the shot's yaw, but the
        // renderer frames free-roam field with the FIXED follow camera (which
        // never reads `self.camera`), so the stale cinematic yaw would feed
        // `field_camera_azimuth` below and rotate the d-pad → direction remap
        // ~180deg off the on-screen camera (the New Game prologue → Rim Elm
        // hand-off left the controls inverted). See `Camera::reset_for_free_roam`.
        self.camera.reset_for_free_roam(&self.host.world);
        // Feed the previous frame's camera azimuth into the world so the
        // field free-movement controller remaps the d-pad camera-relative
        // ("screen up" walks away from the camera). The compass sums the
        // scripted yaw, the user's manual drag-orbit, and the host
        // renderer's fixed framing bias (`Camera::compass_azimuth_units`);
        // all three default to 0, which maps straight to world +Z.
        self.host.world.field_camera_azimuth = self.camera.compass_azimuth_units();
        let event = self.host.tick()?;
        self.camera.route_camera_events(&mut self.host.world);
        if let Some(bgm) = self.bgm.as_mut() {
            // SceneHost::route_bgm_events drains the world's pending BGM
            // events and dispatches into the director.
            let _ = self.host.route_bgm_events(bgm)?;
        }
        // After events: camera tick + scene-transition BGM rebind.
        self.camera.tick(&self.host.world);
        if let SceneTickEvent::SceneEntered { .. } = &event {
            // Field entry resets the camera globals (`FUN_80025C24`) and kills
            // any mover in flight, so a departing scene's shot can't leak its
            // eye-space depth or focus into the next one. The sibling reset of
            // the op-0x45 param set lives in `SceneHost`'s scene entry.
            self.camera.reset_globals_for_scene_entry();
        }
        if let SceneTickEvent::SceneEntered { .. } = &event
            && let (Some(bgm), Some(audio)) = (self.bgm.as_mut(), self.audio.as_ref())
        {
            // New scene -> upload its VAB bank and drop any SFX cues that
            // were queued against the previous scene's VAB.
            bgm.clear_sfx();
            if let Err(e) = stage_scene_vab(bgm, audio.as_ref(), &self.host) {
                log::warn!("BGM bank not staged after scene enter: {e:#}");
            }
        }
        self.frames += 1;
        Ok(event)
    }

    /// Drop the world into a live field scene: run the scene's event-script
    /// record 0 (the init prologue) so the field VM actually ticks, install
    /// the per-scene encounter table, and arm the live gameplay loop per
    /// `opts`.
    ///
    /// [`BootSession::open`] only calls `load_scene`, which leaves the world
    /// in [`SceneMode::Title`] with no field events firing. This is the
    /// reusable core of the windowed host's `--live-loop` setup, shared so the
    /// v0.1 oracle and headless drivers reach Field/Battle the same way the
    /// window does.
    ///
    /// Soft-fails the same way the window does: a scene with no event script
    /// logs and continues (the world stays in whatever mode it was in).
    /// Returns the active [`SceneMode`] after the attempt.
    pub fn enter_field_live(&mut self, scene: &str, opts: &FieldLiveOpts) -> Result<SceneMode> {
        match self.host.enter_field_scene(scene, 0) {
            Ok(()) => log::info!("entered field scene '{scene}' record 0 (field VM live)"),
            Err(e) => log::warn!(
                "enter_field_scene('{scene}', 0) failed ({e:#}); staying on the load_scene-only \
                 path (field VM will not tick)"
            ),
        }

        let world = &mut self.host.world;
        world.set_active_scene_label(scene);

        // `enter_field_scene` already installs the disc-resident per-scene
        // encounter table from the MAN asset. Only fall back to the synthetic
        // registry + vanilla tables when no MAN encounter was installed.
        if world.encounter.is_none() && matches!(world.mode, SceneMode::Field) {
            world.set_formation_table(
                legaia_engine_core::monster_catalog::vanilla_formation_table(),
                legaia_engine_core::monster_catalog::vanilla_monster_catalog(),
            );
            let registry = legaia_engine_core::encounter_registry::vanilla_encounter_registry();
            world.install_encounter_for_scene(&registry, scene);
        }

        // Install the equipment / spell / item catalogs unconditionally so
        // every consumer - not just the battle loop - sees real data. The
        // field pause-menu (Equip / Magic / Items screens) reads these off
        // the world; before they were flag-gated and the menu fell back to
        // throwaway vanilla()/new() placeholders that ignored disc data.
        // Each prefers the disc-accurate real-id table and falls back to the
        // fabricated-id vanilla catalog on disc-free builds.
        world.set_equipment_table(self.equip_modifier_table.clone().unwrap_or_else(|| {
            legaia_engine_core::equipment::vanilla_equipment_catalog().to_modifier_table()
        }));
        world.set_spell_catalog(
            self.spell_catalog
                .clone()
                .unwrap_or_else(legaia_engine_core::retail_magic::retail_seru_magic_catalog),
        );
        world.set_item_catalog(legaia_engine_core::items::ItemCatalog::vanilla());

        if opts.live_loop || opts.player_battle {
            world.live_gameplay_loop = true;
        }
        world.set_battle_bgm(opts.battle_bgm);
        if opts.player_battle {
            world.battle_player_driven = true;
            world.set_seru_registry(legaia_engine_core::seru_learning::SeruRegistry::retail());
        }

        Ok(world.mode)
    }

    /// Enter a world-map scene live: load the scene's resources, route its
    /// region-keyed encounter table onto the overworld, install the player
    /// actor, and switch into [`SceneMode::WorldMap`].
    ///
    /// The window's `--world-map` flag used to call [`World::enter_world_map`]
    /// directly, which only installs the camera controller (a camera-only
    /// debug viewer). This is the playable counterpart to [`Self::enter_field_live`]:
    /// it loads the scene through [`SceneHost::enter_field_scene`] (which seeds
    /// the formation table + monster catalog from the MAN, so overworld
    /// encounters resolve to real monsters), builds the
    /// [`RegionEncounterTable`](legaia_engine_core::region_encounter::RegionEncounterTable)
    /// from the same MAN, routes it via
    /// [`World::set_world_map_regions`], installs the field player so
    /// `tick_world_map`'s locomotion + per-tile encounter roll run, and enters
    /// world-map mode with the live loop armed.
    ///
    /// Soft-fails like [`Self::enter_field_live`]: a scene that fails to load
    /// logs and continues into world-map mode without a region table (camera
    /// only). Returns the active [`SceneMode`].
    pub fn enter_world_map_live(&mut self, scene: &str, opts: &FieldLiveOpts) -> Result<SceneMode> {
        // The scene load + region routing + world-map mode now live in
        // `SceneHost::enter_world_map_scene`, so the natural boot/transition
        // path (`SceneHost::tick` auto-routing an overworld scene) and this
        // explicit `--world-map` entry seed the overworld identically. This
        // wrapper only layers the live-loop / battle options on top.
        match self.host.enter_world_map_scene(scene) {
            Ok(()) => log::info!("entered world-map scene '{scene}' (overworld seeded)"),
            Err(e) => {
                log::warn!(
                    "enter_world_map_scene('{scene}') failed ({e:#}); world map camera-only"
                );
                // Still switch into world-map mode so the window has a camera.
                self.host.world.set_active_scene_label(scene);
                self.host.world.enter_world_map();
            }
        }

        let equip_table = self.equip_modifier_table.clone().unwrap_or_else(|| {
            legaia_engine_core::equipment::vanilla_equipment_catalog().to_modifier_table()
        });
        let world = &mut self.host.world;
        world.live_gameplay_loop = true;
        world.set_equipment_table(equip_table);
        world.set_battle_bgm(opts.battle_bgm);
        if opts.player_battle {
            world.battle_player_driven = true;
            world.set_item_catalog(legaia_engine_core::items::ItemCatalog::vanilla());
            world.set_spell_catalog(
                self.spell_catalog
                    .clone()
                    .unwrap_or_else(legaia_engine_core::retail_magic::retail_seru_magic_catalog),
            );
            world.set_seru_registry(legaia_engine_core::seru_learning::SeruRegistry::retail());
        }
        Ok(world.mode)
    }

    /// Enter a field scene live, then seed the world from a saved game.
    ///
    /// [`Self::enter_field_live`] cold-boots the scene at record 0 (a fresh
    /// party, no story progress). This variant runs that path and then
    /// hydrates the world from `save` via [`legaia_engine_core::World::load_full`]
    /// (party records, story flags, money, inventory) so the field VM sees the
    /// saved story state on its first tick. It is the building block for
    /// "continue a saved game" and for the story-gated paths that a cold boot
    /// into record 0 can't reach, such as a scripted-encounter trigger armed
    /// by story state.
    ///
    /// The save is applied *after* the scene is entered, so the scene record
    /// is still 0; selecting the story-appropriate record from the seeded
    /// flags is a separate concern (the field VM's record picker).
    ///
    /// To seed from a retail memory-card SC block, parse it first with
    /// [`legaia_save::SaveFile::from_retail_sc_block`].
    pub fn enter_field_live_from_save(
        &mut self,
        scene: &str,
        opts: &FieldLiveOpts,
        save: legaia_save::SaveFile,
    ) -> Result<SceneMode> {
        self.enter_field_live(scene, opts)?;
        self.host.world.load_full(save);
        log::info!("seeded world from save ({} party records)", {
            self.host.world.party_count
        });
        Ok(self.host.world.mode)
    }

    /// Shut down the audio stream and clear the scene. Idempotent.
    pub fn shutdown(&mut self) {
        if let Some(audio) = self.audio.take() {
            audio.detach_sequencer();
        }
        self.bgm = None;
    }
}

impl Drop for BootSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Pull the scene's first VAB-bearing entry through the scene host, parse
/// it, upload its samples into the SPU, and stash the resulting [`VabBank`]
/// in the director.
fn stage_scene_vab(
    director: &mut AudioBgmDirector,
    audio: &AudioOut,
    host: &SceneHost,
) -> Result<()> {
    let Some(bytes) = host.scene_vab_bytes()? else {
        return Ok(());
    };
    let report = legaia_vab::parse(&bytes, 0).context("parse scene VAB header")?;
    let bank = audio.with_spu(|spu: &mut Spu| {
        // Cap the BGM region below the resident class-2 SFX bank at the top of
        // SPU RAM, so a scene-BGM upload never stomps the SFX samples.
        let mut alloc = SpuAllocator::new(
            SPU_RESERVED_BYTES,
            SPU_RAM_BYTES - SPU_RESERVED_BYTES - SFX_BANK_SPU_BYTES,
        );
        VabBank::upload(spu, &mut alloc, &report, &bytes)
    });
    director.set_bank(bank);
    Ok(())
}

/// Read the class-2 SFX program bank (PROT [`SFX_BANK_PROT_INDEX`]), parse its
/// VAB, upload the samples into the dedicated top region of SPU RAM, and stash
/// the resulting [`VabBank`] in the director. The entry is a scene-VAB-style
/// stream (`[u32 chunk header][VAB]...`), so the VAB starts at `+4` (with a
/// `+0` fallback for a bare bank). Uploaded once at boot; it stays resident
/// across scene transitions because the BGM region is capped below it.
fn stage_sfx_vab(
    director: &mut AudioBgmDirector,
    audio: &AudioOut,
    host: &SceneHost,
) -> Result<()> {
    let bytes = host
        .index
        .entry_bytes_extended(SFX_BANK_PROT_INDEX)
        .with_context(|| format!("read PROT {SFX_BANK_PROT_INDEX}"))?;
    let (report, vab_off) = [4usize, 0]
        .into_iter()
        .find_map(|o| legaia_vab::parse(&bytes, o).ok().map(|r| (r, o)))
        .context("no VAB header at +4 or +0 in the class-2 SFX bank")?;
    let body = &bytes[vab_off..];
    let bank = audio.with_spu(|spu: &mut Spu| {
        let mut alloc = SpuAllocator::new(SPU_RAM_BYTES - SFX_BANK_SPU_BYTES, SFX_BANK_SPU_BYTES);
        VabBank::upload(spu, &mut alloc, &report, body)
    });
    director.set_sfx_vab(bank);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_boot_config_uses_town01() {
        let c = BootConfig::default();
        assert_eq!(c.scene, "town01");
        assert!(c.enable_audio);
    }
}
