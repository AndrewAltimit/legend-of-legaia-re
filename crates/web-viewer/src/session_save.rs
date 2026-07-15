//! Browser save import / export: the wasm boundary for session state.
//!
//! Two save families cross this boundary, and each **leaves in the format it
//! arrived in**:
//!
//! - **LGSF** (`.lgsf`) - the engine's own save format
//!   ([`legaia_save::SaveFile`], LGSF v2+). [`LegaiaRuntime::export_save`] /
//!   [`LegaiaRuntime::import_save`] wrap the engine-core round-trip
//!   (`World::save_full` / `World::load_full`) with magic/version validation,
//!   so a corrupt upload fails with a visible message instead of a panic.
//! - **Retail emulator saves** - a memory-card image exported from the
//!   player's emulator (raw `.mcr`/`.mcd`, DexDrive `.gme`, single-save
//!   `.mcs`; PS3 `.psv` is rejected - it's cryptographically signed).
//!   [`card_saves_json`] lists the Legaia saves inside;
//!   [`LegaiaRuntime::import_card_save`] lifts one into the live engine
//!   world ([`legaia_save::SaveFile::from_retail_sc_block`]); and
//!   [`card_patch_coins`] banks minigame coin winnings into the pinned
//!   retail coin slot (`RETAIL_COINS_OFFSET`, RAM `0x800845A4`) **in
//!   place** - every other byte is preserved, so exporting an untouched
//!   card is byte-identical and the result loads in the emulator again.
//!
//! Persistence (localStorage, base64) and file downloads stay on the JS
//! side (`site/js/legaia-saves.js`) - this module is serialization only.

use legaia_save::{SaveFile, emu};
use wasm_bindgen::prelude::*;

use crate::runtime::LegaiaRuntime;

/// JSON summary of a parsed LGSF save - what the "your games" strip shows.
fn lgsf_summary(sf: &SaveFile) -> serde_json::Value {
    let names: Vec<String> = sf
        .party
        .members
        .iter()
        .map(|m| {
            let n = m.name();
            if n.is_empty() { "?".to_string() } else { n }
        })
        .collect();
    // The retail displayed level lives at record `+0x130` (`magic_rank`);
    // the save bar shows the lead's.
    let level = sf.party.members.first().map(|m| m.magic_rank());
    serde_json::json!({
        "kind": "lgsf",
        "party": names,
        "party_count": sf.party.members.len(),
        "level": level,
        "money": sf.ext.money,
        "play_time_seconds": sf.ext_v2.play_time_seconds,
        "items": sf.ext.inventory.len(),
    })
}

/// Read a NUL-terminated ASCII string out of an SC block.
fn sc_ascii(block: &[u8], offset: usize, max: usize) -> String {
    block
        .get(offset..offset + max)
        .map(|b| {
            b.iter()
                .take_while(|&&c| c != 0)
                .map(|&c| {
                    if (0x20..=0x7E).contains(&c) {
                        c as char
                    } else {
                        '?'
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Per-save JSON for one SC block inside a card container.
fn card_save_summary(block: &[u8], save: &emu::SaveRef) -> serde_json::Value {
    let parsed = SaveFile::from_retail_sc_block(block, 4).ok();
    let names: Vec<String> = parsed
        .as_ref()
        .map(|sf| sf.party.members.iter().map(|m| m.name()).collect())
        .unwrap_or_default();
    // The lead's retail displayed level (record `+0x130`, `magic_rank`).
    let level = parsed
        .as_ref()
        .and_then(|sf| sf.party.members.first())
        .map(|m| m.magic_rank());
    serde_json::json!({
        "kind": "card",
        "block": save.block,
        "product_code": save.product_code,
        "valid": parsed.is_some() && save.has_sc_magic,
        "party": names,
        "level": level,
        "money": legaia_save::read_retail_gold(block),
        "coins": legaia_save::read_retail_coins(block),
        "location": sc_ascii(block, legaia_save::card::RETAIL_LOCATION_NAME_OFFSET, 0x40),
        "scene": sc_ascii(block, legaia_save::card::RETAIL_SCENE_LABEL_OFFSET, 0x10),
    })
}

// ---------------------------------------------------------------------------
// JsValue-free cores (testable natively; JsValue panics off-wasm).
// ---------------------------------------------------------------------------

fn card_saves_json_core(bytes: &[u8]) -> Result<String, String> {
    let view = emu::detect(bytes).map_err(|e| format!("{e}"))?;
    let saves = view.saves(bytes).map_err(|e| format!("{e}"))?;
    let entries: Vec<serde_json::Value> = saves
        .iter()
        .filter_map(|s| {
            view.sc_block(bytes, s.block)
                .map(|block| card_save_summary(block, s))
        })
        .collect();
    Ok(serde_json::json!({
        "format": view.format.label(),
        "saves": entries,
    })
    .to_string())
}

fn card_read_coins_core(bytes: &[u8], block: u8) -> Result<u32, String> {
    let view = emu::detect(bytes).map_err(|e| format!("{e}"))?;
    let sc = view
        .sc_block(bytes, block)
        .ok_or_else(|| format!("card_read_coins: no block {block}"))?;
    legaia_save::read_retail_coins(sc).ok_or_else(|| "card_read_coins: block too small".into())
}

fn card_patch_coins_core(bytes: Vec<u8>, block: u8, coins: u32) -> Result<Vec<u8>, String> {
    let mut out = bytes;
    let view = emu::detect(&out).map_err(|e| format!("{e}"))?;
    let sc = view
        .sc_block_mut(&mut out, block)
        .ok_or_else(|| format!("card_patch_coins: no block {block}"))?;
    legaia_save::write_retail_coins(sc, coins).map_err(|e| format!("{e}"))?;
    Ok(out)
}

fn save_summary_json_core(bytes: &[u8]) -> Result<String, String> {
    if bytes.starts_with(b"LGSF") {
        let sf = SaveFile::parse(bytes).map_err(|e| format!("not a valid LGSF save: {e}"))?;
        return Ok(lgsf_summary(&sf).to_string());
    }
    card_saves_json_core(bytes)
}

/// Byte offset of the SC block's 16-colour icon palette (16 x u16 LE
/// PSX 15-bit colours).
const SC_ICON_CLUT_OFFSET: usize = 0x60;

/// Byte offset of the SC block's 16x16 4bpp icon pixels (128 bytes,
/// low nibble = left pixel).
const SC_ICON_PIXELS_OFFSET: usize = 0x80;

/// Decode the SC block's own 16x16 memory-card icon to RGBA8 (1024 bytes).
///
/// This is the icon the PSX memory-card browser shows for the save - and for
/// Legaia it is a **character portrait**: the save writer bakes the lead
/// character's 16x16 load-screen portrait TIM into the block (palette at
/// `+0x60`, 4bpp pixels at `+0x80`), so the icon *is* the lead's face.
/// Palette entry 0 decodes transparent (PSX colour 0 == not drawn).
fn sc_icon_rgba_core(bytes: &[u8], block: u8) -> Result<Vec<u8>, String> {
    let view = emu::detect(bytes).map_err(|e| format!("{e}"))?;
    let sc = view
        .sc_block(bytes, block)
        .ok_or_else(|| format!("sc_icon: no block {block}"))?;
    if sc.len() < SC_ICON_PIXELS_OFFSET + 128 {
        return Err("sc_icon: block too small".into());
    }
    let clut: Vec<[u8; 4]> = (0..16)
        .map(|i| {
            let p = SC_ICON_CLUT_OFFSET + i * 2;
            let v = u16::from_le_bytes([sc[p], sc[p + 1]]);
            let scale5 = |c: u16| ((c & 0x1F) * 255 / 31) as u8;
            let a = if v == 0 { 0 } else { 255 };
            [scale5(v), scale5(v >> 5), scale5(v >> 10), a]
        })
        .collect();
    let mut out = Vec::with_capacity(16 * 16 * 4);
    for byte in &sc[SC_ICON_PIXELS_OFFSET..SC_ICON_PIXELS_OFFSET + 128] {
        out.extend_from_slice(&clut[(byte & 0x0F) as usize]);
        out.extend_from_slice(&clut[(byte >> 4) as usize]);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// wasm exports
// ---------------------------------------------------------------------------

/// List the Legaia saves inside an emulator save container.
///
/// Accepts raw `.mcr`/`.mcd` card images, DexDrive `.gme`, and single-save
/// `.mcs`. Returns
/// `{"format": "mcr"|"gme"|"mcs", "saves": [{block, product_code, valid,
/// party, money, coins, location, scene}, ...]}`. Errors (thrown as JS
/// strings) on unknown containers and on signed `.psv` exports.
#[wasm_bindgen]
pub fn card_saves_json(bytes: Vec<u8>) -> Result<String, JsValue> {
    card_saves_json_core(&bytes).map_err(|e| JsValue::from_str(&e))
}

/// Read the casino coin bank from save block `block` of a card container.
#[wasm_bindgen]
pub fn card_read_coins(bytes: Vec<u8>, block: u8) -> Result<u32, JsValue> {
    card_read_coins_core(&bytes, block).map_err(|e| JsValue::from_str(&e))
}

/// Bank a coin balance into save block `block` of a card container,
/// returning the whole container with **only those 4 bytes changed** - the
/// same format it came in, still a valid retail save (the retail payload
/// carries no checksum; the card's directory-frame checksums are untouched).
#[wasm_bindgen]
pub fn card_patch_coins(bytes: Vec<u8>, block: u8, coins: u32) -> Result<Vec<u8>, JsValue> {
    card_patch_coins_core(bytes, block, coins).map_err(|e| JsValue::from_str(&e))
}

/// Summarise save bytes of either family (LGSF or an emulator card
/// container) without touching the runtime - what the "your games" strip
/// uses to describe a stored slot. Throws on unrecognised bytes.
#[wasm_bindgen]
pub fn save_summary_json(bytes: Vec<u8>) -> Result<String, JsValue> {
    save_summary_json_core(&bytes).map_err(|e| JsValue::from_str(&e))
}

/// The 16x16 memory-card icon baked into save block `block` of a card
/// container, as 1024 RGBA8 bytes. For Legaia saves this is the lead
/// character's portrait - the retail save writer copies the load-screen
/// portrait TIM into the SC block (palette `+0x60`, 4bpp pixels `+0x80`).
/// The site's save bar draws it as the slot's face.
#[wasm_bindgen]
pub fn card_icon_rgba(bytes: Vec<u8>, block: u8) -> Result<Vec<u8>, JsValue> {
    sc_icon_rgba_core(&bytes, block).map_err(|e| JsValue::from_str(&e))
}

/// One of the three retail 16x16 **save-file portrait** TIMs decoded to a
/// 1024-byte RGBA8 buffer: `0` = Vahn, `1` = Noa, `2` = Gala. Accepts either
/// a full Mode2/2352 disc image or raw `PROT.DAT` bytes - the same input
/// [`LegaiaRuntime::load_disc`] takes - so the play page can draw the party
/// roster faces beside each save tile from the disc it already loaded, exactly
/// as the minigames save bar does from its `LegaiaMinigames`
/// (`save_portrait_rgba`). These are the load-screen slot-grid portraits
/// pinned in the pre-`init_data` gap of `PROT.DAT` (offset `0x1AC90`, 192-byte
/// stride); retail bakes the lead's copy into every SC block, so they are the
/// exact faces a retail save carries. Empty when no PROT is found or the TIM
/// doesn't parse - the bar falls back to initial chips.
#[wasm_bindgen]
pub fn disc_portrait_rgba(bytes: Vec<u8>, char_id: usize) -> Vec<u8> {
    use crate::disc::{extract_prot_dat, is_mode2_2352_disc};
    use legaia_asset::title_pak::{
        OVERLAY_LOAD_PORTRAIT_COUNT, OVERLAY_LOAD_PORTRAIT_STRIDE, OVERLAY_LOAD_PORTRAIT_TIM_OFFSET,
    };
    if char_id >= OVERLAY_LOAD_PORTRAIT_COUNT {
        return Vec::new();
    }
    let prot = if is_mode2_2352_disc(&bytes) {
        match extract_prot_dat(&bytes) {
            Some(p) => p,
            None => return Vec::new(),
        }
    } else {
        bytes
    };
    let off = OVERLAY_LOAD_PORTRAIT_TIM_OFFSET + char_id * OVERLAY_LOAD_PORTRAIT_STRIDE;
    let Some(tim_bytes) = prot.get(off..off + OVERLAY_LOAD_PORTRAIT_STRIDE) else {
        return Vec::new();
    };
    let Ok(parsed) = legaia_tim::parse(tim_bytes) else {
        return Vec::new();
    };
    legaia_tim::decode_rgba8(&parsed, 0).unwrap_or_default()
}

impl LegaiaRuntime {
    /// JsValue-free core of [`Self::import_save`].
    fn import_save_core(&mut self, bytes: &[u8]) -> Result<String, String> {
        if !bytes.starts_with(b"LGSF") {
            return Err(
                "import_save: not an LGSF save (missing magic) - retail memory-card saves go \
                 through import_card_save"
                    .to_string(),
            );
        }
        let sf =
            SaveFile::parse(bytes).map_err(|e| format!("import_save: invalid LGSF file: {e}"))?;
        let summary = lgsf_summary(&sf).to_string();
        self.world_mut().load_full(sf);
        Ok(summary)
    }

    /// JsValue-free core of [`Self::import_card_save`].
    fn import_card_save_core(&mut self, bytes: &[u8], block: u8) -> Result<String, String> {
        let view = emu::detect(bytes).map_err(|e| format!("{e}"))?;
        let saves = view.saves(bytes).map_err(|e| format!("{e}"))?;
        let save_ref = saves
            .iter()
            .find(|s| s.block == block)
            .ok_or_else(|| format!("import_card_save: no save block {block}"))?
            .clone();
        let sc = view
            .sc_block(bytes, block)
            .ok_or_else(|| "import_card_save: block out of range".to_string())?;
        let sf = SaveFile::from_retail_sc_block(sc, 4)
            .map_err(|e| format!("import_card_save: not a valid retail save: {e}"))?;
        if sf.party.members.is_empty() {
            return Err("import_card_save: save block holds no character records".to_string());
        }
        let summary = card_save_summary(sc, &save_ref).to_string();
        self.world_mut().load_full(sf);
        Ok(summary)
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Export the current engine session as LGSF bytes
    /// (`World::save_full().write()`). The page offers this as a `.lgsf`
    /// download and persists it (base64) in localStorage.
    pub fn export_save(&mut self) -> Vec<u8> {
        self.world_mut().save_full().write()
    }

    /// Import an LGSF save into the live engine session. Validates the
    /// magic/version envelope before touching the world; a bad file leaves
    /// the session unchanged and throws a readable message. Returns the
    /// same summary JSON as [`save_summary_json`].
    pub fn import_save(&mut self, bytes: Vec<u8>) -> Result<String, JsValue> {
        self.import_save_core(&bytes)
            .map_err(|e| JsValue::from_str(&e))
    }

    /// Import a **retail emulator save** (block `block` of a card container)
    /// into the live engine session: party records, story flags, inventory,
    /// and gold, via [`SaveFile::from_retail_sc_block`]. Returns the block's
    /// summary JSON (including the save's own `scene` label, so the page can
    /// drop the player into the scene the save was made in).
    pub fn import_card_save(&mut self, bytes: Vec<u8>, block: u8) -> Result<String, JsValue> {
        self.import_card_save_core(&bytes, block)
            .map_err(|e| JsValue::from_str(&e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip_runtime() -> LegaiaRuntime {
        LegaiaRuntime::new()
    }

    #[test]
    fn lgsf_export_import_round_trips_via_runtime() {
        let mut rt = roundtrip_runtime();
        {
            let w = rt.world_mut();
            w.money = 777;
            w.inventory.insert(0x77, 5);
            w.load_party(legaia_save::Party {
                members: vec![legaia_save::CharacterRecord::zeroed()],
            });
        }
        let bytes = rt.export_save();
        assert_eq!(&bytes[..4], b"LGSF");

        let mut rt2 = roundtrip_runtime();
        let summary = rt2.import_save_core(&bytes).expect("import");
        assert!(summary.contains("\"money\":777"), "{summary}");
        assert_eq!(rt2.world_mut().money, 777);
        assert_eq!(rt2.world_mut().inventory.get(&0x77).copied(), Some(5));
    }

    #[test]
    fn import_save_rejects_garbage_without_touching_the_world() {
        let mut rt = roundtrip_runtime();
        rt.world_mut().money = 1234;
        assert!(rt.import_save_core(&[0u8; 64]).is_err());
        assert!(rt.import_save_core(b"LGSFgarbage").is_err());
        assert_eq!(rt.world_mut().money, 1234, "failed import changes nothing");
    }

    #[test]
    fn card_json_and_coin_patch_round_trip() {
        // Synthesise a one-save card with a party record + coins.
        let mut card = vec![0u8; legaia_save::CARD_SIZE];
        card[..2].copy_from_slice(&legaia_save::CARD_MAGIC);
        let f = 0x80;
        card[f..f + 4].copy_from_slice(&0x51u32.to_le_bytes());
        card[f + 8..f + 10].copy_from_slice(&0xFFFFu16.to_le_bytes());
        card[f + 10..f + 22].copy_from_slice(b"BASCUS-94254");
        let b = legaia_save::BLOCK_SIZE;
        card[b..b + 2].copy_from_slice(&legaia_save::SAVE_BLOCK_MAGIC);
        let mut rec = legaia_save::CharacterRecord::zeroed();
        rec.set_name("Vahn");
        rec.set_hp_mp_sp(legaia_save::HpMpSp {
            hp_cur: 180,
            hp_max: 180,
            mp_cur: 20,
            mp_max: 20,
            sp_cur: 0,
            sp_max: 100,
        });
        legaia_save::write_retail_char_records(
            &mut card[b..b + legaia_save::BLOCK_SIZE],
            std::slice::from_ref(&rec.raw),
        )
        .unwrap();
        legaia_save::write_retail_gold(&mut card[b..b + legaia_save::BLOCK_SIZE], 900).unwrap();
        legaia_save::write_retail_coins(&mut card[b..b + legaia_save::BLOCK_SIZE], 70).unwrap();

        let listing = card_saves_json_core(&card).expect("list");
        assert!(listing.contains("\"format\":\"mcr\""), "{listing}");
        assert!(listing.contains("Vahn"), "{listing}");
        assert!(listing.contains("\"coins\":70"), "{listing}");

        // No-op patch = byte-identical.
        let same = card_patch_coins_core(card.clone(), 1, 70).expect("noop patch");
        assert_eq!(same, card);

        let patched = card_patch_coins_core(card.clone(), 1, 1234).expect("patch");
        assert_eq!(card_read_coins_core(&patched, 1).unwrap(), 1234);
        let base = legaia_save::BLOCK_SIZE + legaia_save::RETAIL_COINS_OFFSET;
        let diff: Vec<usize> = card
            .iter()
            .zip(patched.iter())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i)
            .collect();
        assert!(!diff.is_empty());
        assert!(
            diff.iter().all(|&i| (base..base + 4).contains(&i)),
            "only the coin dword may change: {diff:?}"
        );

        // Import into a runtime: party + gold land in the world.
        let mut rt = roundtrip_runtime();
        let summary = rt.import_card_save_core(&patched, 1).expect("card import");
        assert!(summary.contains("\"kind\":\"card\""), "{summary}");
        assert!(summary.contains("\"coins\":1234"), "{summary}");
        assert_eq!(rt.world_mut().money, 900);
        assert_eq!(rt.world_mut().roster.members.len(), 1);
    }

    #[test]
    fn sc_icon_decodes_and_summary_carries_level() {
        // Synthesise a one-save card with an icon + a levelled lead.
        let mut card = vec![0u8; legaia_save::CARD_SIZE];
        card[..2].copy_from_slice(&legaia_save::CARD_MAGIC);
        let f = 0x80;
        card[f..f + 4].copy_from_slice(&0x51u32.to_le_bytes());
        card[f + 8..f + 10].copy_from_slice(&0xFFFFu16.to_le_bytes());
        card[f + 10..f + 22].copy_from_slice(b"BASCUS-94254");
        let b = legaia_save::BLOCK_SIZE;
        card[b..b + 2].copy_from_slice(&legaia_save::SAVE_BLOCK_MAGIC);
        // Icon: palette slot 1 = pure red (PSX 15-bit, R in the low 5 bits),
        // pixels alternate colour 0 / colour 1 (low nibble = left pixel).
        let clut = b + SC_ICON_CLUT_OFFSET;
        card[clut + 2..clut + 4].copy_from_slice(&0x001Fu16.to_le_bytes());
        for px in 0..128 {
            card[b + SC_ICON_PIXELS_OFFSET + px] = 0x10; // left px colour 0, right colour 1
        }
        let mut rec = legaia_save::CharacterRecord::zeroed();
        rec.set_name("Vahn");
        rec.set_magic_rank(7);
        legaia_save::write_retail_char_records(
            &mut card[b..b + legaia_save::BLOCK_SIZE],
            std::slice::from_ref(&rec.raw),
        )
        .unwrap();

        let rgba = sc_icon_rgba_core(&card, 1).expect("icon decode");
        assert_eq!(rgba.len(), 16 * 16 * 4);
        // Left pixel of each pair: colour 0 = transparent; right: opaque red.
        assert_eq!(&rgba[0..4], &[0, 0, 0, 0]);
        assert_eq!(&rgba[4..8], &[255, 0, 0, 255]);

        let listing = card_saves_json_core(&card).expect("list");
        assert!(listing.contains("\"level\":7"), "{listing}");
    }

    #[test]
    fn save_summary_handles_both_families() {
        let mut rt = roundtrip_runtime();
        rt.world_mut().load_party(legaia_save::Party {
            members: vec![legaia_save::CharacterRecord::zeroed()],
        });
        let lgsf = rt.export_save();
        let s = save_summary_json_core(&lgsf).unwrap();
        assert!(s.contains("\"kind\":\"lgsf\""), "{s}");
        assert!(save_summary_json_core(&[0u8; 32]).is_err());
    }
}
