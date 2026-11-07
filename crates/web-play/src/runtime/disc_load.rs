//! `LegaiaRuntime::load_disc`: build the `SceneHost` from the visitor's disc
//! image, plus the disc-extent and loaded queries.

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Load a disc image from raw in-memory bytes.
    ///
    /// `raw_bytes` may be either a Mode2/2352 full disc image (`.bin`) - PROT.DAT
    /// and CDNAME.TXT are extracted via an ISO9660 walk - or the raw contents of
    /// `PROT.DAT`. `cdname_text` overrides any CDNAME.TXT found on the disc; pass
    /// an empty string to use the disc's own.
    ///
    /// Returns the number of PROT entries parsed. Nothing leaves the browser.
    pub fn load_disc(&mut self, raw_bytes: Vec<u8>, cdname_text: String) -> Result<u32, JsValue> {
        let (prot, cdname, scus) = self.read_disc_files(raw_bytes.as_slice())?;
        // `raw_bytes` is `PROT.DAT` itself when the walk found no disc.
        let prot = prot.unwrap_or(raw_bytes);
        self.install_disc(prot, cdname, scus, cdname_text)
    }

    /// [`Self::load_disc`] over the page's own copy of the image. The image
    /// stays on the page's side: only the files the runtime keeps cross into
    /// wasm memory, which a wasm instance cannot give back once it has grown
    /// to hold a whole disc.
    pub fn load_disc_image(
        &mut self,
        image: &js_sys::Uint8Array,
        cdname_text: String,
    ) -> Result<u32, JsValue> {
        let source = SpanImage::new(image.length() as usize, vec![(0, image.clone())]);
        let (prot, cdname, scus) = self.read_disc_files(&source)?;
        let prot = prot.unwrap_or_else(|| image.to_vec());
        self.install_disc(prot, cdname, scus, cdname_text)
    }

    /// [`Self::load_disc_image`] over the parts of an image the page has read:
    /// `blocks[i]` holds raw sectors from `lbas[i]`, of an image `image_len`
    /// bytes long. The walk and the three files it extracts read nothing else,
    /// which [`Self::disc_image_missing`] settles first.
    pub fn load_disc_spans(
        &mut self,
        image_len: f64,
        lbas: &[u32],
        blocks: Vec<js_sys::Uint8Array>,
        cdname_text: String,
    ) -> Result<u32, JsValue> {
        let source = SpanImage::from_page(image_len, lbas, blocks);
        let (prot, cdname, scus) = self.read_disc_files(&source)?;
        let prot = prot.ok_or_else(|| JsValue::from_str("load_disc: not a Mode 2/2352 image"))?;
        self.install_disc(prot, cdname, scus, cdname_text)
    }

    /// The first sectors [`Self::load_disc_spans`] would read that the blocks
    /// do not hold, as `[lba, count]`; empty once they hold all of them.
    pub fn disc_image_missing(
        &self,
        image_len: f64,
        lbas: &[u32],
        blocks: Vec<js_sys::Uint8Array>,
    ) -> Vec<u32> {
        let source = SpanImage::from_page(image_len, lbas, blocks);
        missing_span(&source).map_or(Vec::new(), |(lba, count)| vec![lba as u32, count as u32])
    }

    /// Raw-sector extent of a named file on the loaded disc image, as
    /// `{"lba": N, "size": bytes}` (`null` when the disc walk found no such
    /// path). `path` is relative to the ISO root, e.g. `XA2.XA` or
    /// `MOV/MV3.STR`; the page slices `discBytes` at `lba * 2352` for
    /// `ceil(size / 2048) * 2352` bytes and hands the sectors back through
    /// the consumer's own install call.
    pub fn disc_file_extent_json(&self, path: &str) -> String {
        let want = path.trim_start_matches('/').to_ascii_uppercase();
        self.disc_files
            .iter()
            .find(|f| f.path.to_ascii_uppercase() == want)
            .map(|f| serde_json::json!({ "lba": f.lba, "size": f.size }).to_string())
            .unwrap_or_else(|| "null".to_string())
    }

    /// `true` if a disc has been loaded.
    pub fn disc_loaded(&self) -> bool {
        self.scene_host.host().is_some()
    }
}

impl LegaiaRuntime {
    /// Walk a disc image: note its files, and hand back `PROT.DAT` (`None` when
    /// the bytes are not a disc image at all), `CDNAME.TXT` and the executable.
    fn read_disc_files<S: psx_iso::iso9660::ImageSource + ?Sized>(
        &mut self,
        image: &S,
    ) -> Result<DiscFiles, JsValue> {
        use psx_iso::image::{
            extract_cdname_txt_source, extract_prot_dat_source, extract_scus_source,
            is_mode2_2352_source, walk_iso_source,
        };
        self.disc_files = walk_iso_source(image);
        // A director built before this disc loaded learns its XA extents.
        self.sync_xa_file_extents();
        if !is_mode2_2352_source(image) {
            return Ok((None, None, None));
        }
        let prot = extract_prot_dat_source(image)
            .ok_or_else(|| JsValue::from_str("load_disc: PROT.DAT not found in disc image"))?;
        Ok((
            Some(prot),
            extract_cdname_txt_source(image),
            extract_scus_source(image),
        ))
    }

    /// Build the session over a disc's `PROT.DAT`, `CDNAME.TXT` and executable.
    fn install_disc(
        &mut self,
        prot_bytes: Vec<u8>,
        auto_cdname: Option<String>,
        scus: Option<Vec<u8>>,
        cdname_text: String,
    ) -> Result<u32, JsValue> {
        let cdname_resolved = if !cdname_text.is_empty() {
            Some(cdname_text.as_str())
        } else {
            auto_cdname.as_deref()
        };
        let mut host = SceneHost::from_prot_bytes(prot_bytes, cdname_resolved)
            .map_err(|e| JsValue::from_str(&format!("load_disc: {e}")))?;
        // Pack maps' exits prepare their maps ahead, in idle-tick slices
        // (`step_expansion_preload_when_idle`).
        host.set_expansion_preload(true);
        // Item-name labels for the field menu's Item screen (executable-only;
        // a PROT.DAT load has no SCUS and the menu shows raw ids instead).
        self.item_names = scus
            .as_ref()
            .and_then(|s| legaia_game_tables::item_names::ItemNameTable::from_scus(s));
        // The seru-trade offers' display names. The trade config itself, the
        // menu text and the static progression tables are the session's
        // installs (`BootSession::from_host`, below), as on the native boot.
        if let Some(s) = scus.as_ref() {
            self.seru_names = legaia_game_tables::spell_names::SpellNameTable::from_scus(s);
        }
        // Sound-effect descriptors from the same executable (`DAT_8006F198`,
        // see docs/formats/sfx-table.md). Data only - the program bank uploads
        // into the SPU lazily once audio is live ([`crate::play_sfx`]).
        if let Some(s) = scus.as_ref() {
            self.install_sfx_descriptors(s);
        }
        // The menu overlay (PROT 0899) also carries the Arrange display-order
        // table (FUN_801D64A8): install it so the Items screen's Arrange
        // command sorts by the retail rank rather than id order.
        if let Ok(overlay) = host
            .index
            .entry_bytes_extended(legaia_game_tables::menu_windows::MENU_OVERLAY_PROT_INDEX as u32)
        {
            host.world.install_menu_overlay_tables(&overlay);
        }
        // The real retail proportional dialog font, decoded straight from the
        // disc (no save state): the 4bpp font TIM in PROT.DAT + the SCUS width
        // table. This is the exact font the native pause menu draws; without it
        // the menu falls back to the built-in placeholder (fixed-width blocks).
        self.menu_font = host
            .index
            .prot_dat_raw_bytes(
                legaia_font::FONT_TIM_PROT_DAT_OFFSET,
                legaia_font::FONT_TIM_LEN,
            )
            .ok()
            .zip(scus.as_ref())
            .and_then(|(tim, scus)| {
                legaia_font::Font::from_disc_tim_and_scus(&tim, scus)
                    .map_err(|e| crate::console_log(&format!("dialog font decode failed: {e}")))
                    .ok()
                    .map(|font| {
                        // The `0xCE` escape sprites (buttons, icons), from
                        // the boot-resident TIMs at the head of PROT.DAT.
                        use legaia_font::escape_icons::{ICON_PROT_DAT_LEN, ICON_PROT_DAT_OFFSET};
                        match host
                            .index
                            .prot_dat_raw_bytes(ICON_PROT_DAT_OFFSET, ICON_PROT_DAT_LEN)
                        {
                            Ok(head) => font.with_escape_icons_from_disc(&head, scus),
                            Err(_) => font,
                        }
                    })
            });
        // Install the equipment / spell / item catalogs on the host world so the
        // pause menu's Equip / Magic / Items sub-screens read real disc data -
        // the same tables the native `play-window` boot installs in
        // `BootSession::enter_field_live`. Each prefers the disc-accurate
        // real-id table (parsed from `SCUS_942.54`) and falls back to the
        // fabricated-id vanilla catalog on a PROT.DAT-only load. Set once here;
        // they persist across scene entry (`enter_field_scene` never clears
        // them). Without them the sub-sessions build from empty catalogs and the
        // menu falls back to a generic frame.
        self.equip_stats = scus
            .as_ref()
            .and_then(|s| legaia_game_tables::equip_stats::EquipStatTable::from_scus(s));
        host.world.set_equipment_table(
            self.equip_stats
                .as_ref()
                .map(legaia_engine_menus::equipment::equip_modifier_table_from_disc)
                .unwrap_or_else(|| {
                    legaia_engine_menus::equipment::vanilla_equipment_catalog().to_modifier_table()
                }),
        );
        // The raw stat-bonus records as well: the Items screen's Throw Out
        // list reads each record's `+7` flags byte, which the derived modifier
        // table does not keep. Twin of the native boot's install in
        // `BootSession::open_with_source`.
        if let Some(table) = self.equip_stats.clone() {
            host.world.set_equip_stats(table);
        }
        // Retail-shaped equipment buy: this page draws the recipient picker
        // (window 36) and the stat-compare windows (25 / 41) over the parked
        // buy list ([`crate::play_shop`]), so opt into the flow and install
        // the disc restrictions the buy-list kind dispatch reads.
        if let Some(table) = self.equip_stats.as_ref() {
            let mut info = legaia_engine_menus::equipment::DiscEquipInfo::from_disc(table);
            // The three Goods rows browse a class-2 id space the equipment
            // stat table does not hold; without this index they offer no
            // candidates at all. Twin of the native boot's install in
            // `legaia_engine_session::native`.
            if let Some(effects) = scus
                .as_ref()
                .and_then(|s| legaia_game_tables::item_effect::ItemEffectTable::from_scus(s))
            {
                info.install_goods(&effects);
            }
            self.menu.install_equip_info(info);
            self.menu.retail_equipment_buy = true;
        }
        // Static per-monster steal table (`DAT_80077828`) - the same install the
        // native boot does, so PROT 0941's Steal resolves a monster-seat victim
        // on this host too rather than drawing nothing.
        if let Some(steal) = scus
            .as_ref()
            .and_then(|s| legaia_game_tables::steal_table::StealTable::from_scus(s))
        {
            host.world.set_steal_table(steal);
        }
        host.world.set_spell_catalog(
            scus.as_ref()
                .and_then(|s| legaia_engine_battle::retail_magic::seru_magic_catalog_from_scus(s))
                .unwrap_or_else(legaia_engine_battle::retail_magic::retail_seru_magic_catalog),
        );
        host.world
            .set_item_catalog(legaia_engine_menus::items::ItemCatalog::vanilla());
        // Real item-effect usability flags (cure/revive = battle-only, etc.);
        // applied after `set_item_catalog` since it rewrites the catalog. Absent
        // on a PROT.DAT-only load (catalog keeps its curated flags).
        if let Some(effects) = scus
            .as_ref()
            .and_then(|s| legaia_game_tables::item_effect::ItemEffectTable::from_scus(s))
        {
            host.world.set_item_effects(effects);
        }
        // Keep the executable bytes for the battle render's per-stage SCUS
        // tables (mirror list / outdoor-cue list). Nothing leaves the browser.
        self.scus = scus;

        // The session over the host: the native boot's own installs (menu
        // text, seru-trade config, progression tables, battle UI strings and
        // spell anim pairs, gold-shop item data, item effects, the new-game
        // defaults and the CDNAME map-id resolver) run here, through the one
        // constructor both hosts share.
        let count = host.index.entry_count() as u32;
        self.scene_host
            .install(host, self.scus.as_deref())
            .map_err(|e| JsValue::from_str(&format!("load_disc: {e:#}")))?;
        self.field = None;
        self.player = None;
        self.actors.clear();
        self.battle_render = None;
        // A new disc means a new PROT: drop any cached menu chrome / open menu /
        // title art.
        self.menu_assets = None;
        self.play_menu = None;
        self.boot_front_end = None;
        self.boot_continue = None;
        self.title_atlas = None;
        self.boot_logos_atlas = None;
        self.boot_logos_failed = false;
        self.menu_glyph_atlas = None;
        // A freshly installed world starts at `World::default()`'s toggles,
        // which are NOT the player's persisted options. The native window
        // re-asserts all four every tick precisely because a scene / New Game
        // transition reseeds world state; this page only pushed them on
        // Options-close, so a persisted "Run" or flash-guard-off never
        // applied at page load and was lost again after a door.
        self.apply_options_side_effects();
        Ok(count)
    }
}

/// `PROT.DAT`, `CDNAME.TXT` and the executable off a disc walk.
type DiscFiles = (Option<Vec<u8>>, Option<String>, Option<Vec<u8>>);

/// Bytes a [`SpanImage`] block copies raw sectors out of: the page's arrays, in
/// one crossing per call.
trait BlockBytes {
    fn byte_len(&self) -> usize;
    fn copy_range(&self, start: usize, out: &mut [u8]);
}

impl BlockBytes for js_sys::Uint8Array {
    fn byte_len(&self) -> usize {
        self.length() as usize
    }

    fn copy_range(&self, start: usize, out: &mut [u8]) {
        self.subarray(start as u32, (start + out.len()) as u32)
            .copy_to(out);
    }
}

/// An image the page holds in parts, as an
/// [`psx_iso::iso9660::ImageSource`]. A read the blocks do not cover fails,
/// and remembers what it asked for.
struct SpanImage<B> {
    len: usize,
    /// Each block's first sector and its raw sectors.
    blocks: Vec<(usize, B)>,
    missing: std::cell::Cell<Option<(usize, usize)>>,
}

impl<B> SpanImage<B> {
    fn new(len: usize, blocks: Vec<(usize, B)>) -> Self {
        Self {
            len,
            blocks,
            missing: std::cell::Cell::new(None),
        }
    }
}

impl SpanImage<js_sys::Uint8Array> {
    fn from_page(image_len: f64, lbas: &[u32], blocks: Vec<js_sys::Uint8Array>) -> Self {
        let blocks = lbas.iter().map(|&l| l as usize).zip(blocks).collect();
        Self::new(image_len as usize, blocks)
    }
}

impl<B: BlockBytes> SpanImage<B> {
    /// The block holding these sectors whole, and its first sector.
    fn holder(&self, lba: usize, count: usize) -> Option<&(usize, B)> {
        self.blocks.iter().find(|(first, bytes)| {
            *first <= lba && (lba + count - first) * 2352 <= bytes.byte_len()
        })
    }

    fn holds(&self, lba: usize, count: usize) -> bool {
        self.holder(lba, count).is_some()
    }
}

impl<B: BlockBytes> psx_iso::iso9660::ImageSource for SpanImage<B> {
    fn image_len(&self) -> usize {
        self.len
    }

    fn read_user_data(&self, lba: usize, count: usize, out: &mut Vec<u8>) -> bool {
        const RAW: usize = 2352;
        const DATA_OFFSET: usize = 24;
        const DATA: usize = 2048;
        // A batch per crossing: one `subarray` per sector costs more than the
        // copies do on a file the size of PROT.DAT.
        const BATCH: usize = 512;
        if (lba + count) * RAW > self.len {
            return false;
        }
        let Some((first, bytes)) = self.holder(lba, count) else {
            if self.missing.get().is_none() {
                self.missing.set(Some((lba, count)));
            }
            return false;
        };
        let mut raw = vec![0u8; BATCH.min(count) * RAW];
        let mut at = lba;
        let end = lba + count;
        while at < end {
            let n = BATCH.min(end - at);
            let slice = &mut raw[..n * RAW];
            bytes.copy_range((at - first) * RAW, slice);
            for sector in slice.as_chunks::<RAW>().0 {
                out.extend_from_slice(&sector[DATA_OFFSET..DATA_OFFSET + DATA]);
            }
            at += n;
        }
        true
    }
}

/// The first sectors a disc load would read that `image` lacks: the walk's own
/// reads, then the three files it extracts, from their directory records.
fn missing_span<B: BlockBytes>(image: &SpanImage<B>) -> Option<(usize, usize)> {
    let files = psx_iso::image::walk_iso_source(image);
    if image.missing.get().is_some() {
        return image.missing.get();
    }
    files
        .iter()
        .filter(|f| ["PROT.DAT", "CDNAME.TXT", "SCUS_942.54"].contains(&f.path.as_str()))
        .map(|f| (f.lba as usize, (f.size as usize).div_ceil(2048)))
        .find(|&(lba, count)| !image.holds(lba, count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use psx_iso::image::{extract_prot_dat_source, extract_scus_source, walk_iso_source};

    impl BlockBytes for &[u8] {
        fn byte_len(&self) -> usize {
            self.len()
        }

        fn copy_range(&self, start: usize, out: &mut [u8]) {
            out.copy_from_slice(&self[start..start + out.len()]);
        }
    }

    /// A real disc read in parts through the walk the page runs: sectors
    /// arrive only as the walk names them, and the load's reads come out as
    /// they do over the whole image.
    #[test]
    fn a_disc_read_in_parts_reads_as_the_whole_image() {
        let Ok(path) = std::env::var("LEGAIA_DISC_BIN") else {
            return;
        };
        let disc = std::fs::read(path).expect("disc");
        const RAW: usize = 2352;
        let mut blocks: Vec<(usize, &[u8])> = vec![(16, &disc[16 * RAW..17 * RAW])];
        let mut rounds = 0;
        let image = loop {
            let image = SpanImage::new(disc.len(), blocks.clone());
            let Some((lba, count)) = missing_span(&image) else {
                break image;
            };
            blocks.push((lba, &disc[lba * RAW..(lba + count) * RAW]));
            rounds += 1;
            assert!(rounds < 256, "the walk does not settle");
        };
        let held: usize = image.blocks.iter().map(|(_, b)| b.len()).sum();
        assert!(
            held < disc.len() / 2,
            "the load read {held} bytes of the image"
        );
        assert_eq!(walk_iso_source(&image), walk_iso_source(disc.as_slice()));
        assert!(extract_prot_dat_source(&image) == extract_prot_dat_source(disc.as_slice()));
        assert!(extract_scus_source(&image) == extract_scus_source(disc.as_slice()));
    }

    #[test]
    fn a_read_outside_the_blocks_names_its_sectors() {
        let raw = vec![0u8; 2352 * 4];
        let image = SpanImage::new(2352 * 100, vec![(10, raw.as_slice())]);
        let mut out = Vec::new();
        use psx_iso::iso9660::ImageSource;
        assert!(image.read_user_data(10, 4, &mut out));
        assert_eq!(out.len(), 2048 * 4);
        assert!(!image.read_user_data(12, 3, &mut out));
        assert_eq!(image.missing.get(), Some((12, 3)));
        // The first miss is the one the page reads next.
        assert!(!image.read_user_data(50, 1, &mut out));
        assert_eq!(image.missing.get(), Some((12, 3)));
        // Past the image's end is no miss.
        let past = SpanImage::new(2352, vec![(0, raw.as_slice())]);
        assert!(!past.read_user_data(5, 1, &mut out));
        assert_eq!(past.missing.get(), None);
    }
}
