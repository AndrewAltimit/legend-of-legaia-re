//! The browser's **memory-card rack**: two card slots the page fills with
//! the player's own card images, and which the in-canvas Load / Save
//! screens read and write.
//!
//! REF: FUN_801E3294 (libcd I/O state machine - `chan = port * 16 + sub_op`)
//! REF: FUN_801E1208 (save-block directory enumeration, 15 entries per card)
//!
//! A PSX has two memory-card ports, which is why retail's save screen shows
//! exactly two `SLOT 1` / `SLOT 2` pills and why this rack holds
//! [`CARD_SLOTS`] cards. Picking a pill reads that card and shows its
//! fifteen blocks as the 5x3 preview grid ([`docs/subsystems/save-screen.md`]);
//! this module is the data half of that flow, [`crate::play_menu`] the UI
//! half.
//!
//! What the rack owns:
//!
//! - The container bytes **verbatim**, in whatever container they arrived in
//!   (`.mcr` / `.mcd` / `.gme` / `.mcs`, normalised by [`legaia_save::emu`]).
//!   Saving edits the SC block in place, so [`LegaiaRuntime::export_card`]
//!   hands back a container the player's emulator still accepts - and a card
//!   that was never saved into exports byte-identical.
//! - A `dirty` flag per slot so the page knows a card has unexported writes.
//!
//! Card-format knowledge lives in [`legaia_save`], not here: block and
//! directory-frame addressing come from [`CardView`], and claiming a block
//! for a save is [`CardView::claim_block`]. This module only decides *which*
//! block and *when*.
//!
//! Nothing here is uploaded; the bytes live in the tab for the session.

use legaia_engine_core::save_select::SlotSnapshot;
use legaia_save::SaveResume;
use legaia_save::emu;
use wasm_bindgen::prelude::*;

use crate::runtime::LegaiaRuntime;

/// Memory-card ports the console (and so this rack) has. Retail's save
/// screen draws one pill per port - see `SAVE_SELECT_SLOT1_POS` +
/// `SAVE_SELECT_SLOT_PITCH_Y` in `legaia-engine-ui`, which only ever step
/// two rows.
pub const CARD_SLOTS: usize = 2;

/// Save blocks a PSX memory card holds (block 0 is the directory). The
/// retail load screen lays these out as its 5x3 preview grid.
pub const CARD_BLOCKS: u8 = 15;

/// A freshly formatted 128 KiB memory-card image with no saves on it
/// ([`legaia_save::card::formatted_card_image`]).
///
/// The page's **browser card**: when the player has no card of their own, the
/// page formats one, keeps it in the browser's storage and mounts it in port
/// 1 - the twin of the native window, whose save directory is always the card
/// in port 1. Without it the retail Save / Load screens had nothing to read or
/// write until the player imported an emulator card, so a fresh player's save
/// point did nothing and Continue never lit.
#[wasm_bindgen]
pub fn formatted_memory_card() -> Vec<u8> {
    legaia_save::card::formatted_card_image()
}

/// The mounted-card type, shared with the native window's card port.
///
/// Both hosts hold the same struct now: it caches the detected [`CardView`]
/// rather than re-detecting on every access, carries the dirty bit, and
/// answers `save_at` / `block_is_save_start` / `dir_frame` off its own bytes.
/// The two hosts used to carry near-identical copies that drifted.
pub use legaia_save::emu::MountedCard;

impl LegaiaRuntime {
    /// The card in rack slot `slot`, if one is inserted.
    pub(crate) fn card(&self, slot: usize) -> Option<&MountedCard> {
        self.cards.get(slot).and_then(|c| c.as_ref())
    }

    /// Per-**card-slot** snapshots: the pill row of the retail save screen.
    /// `present` means "a card is inserted here", not "this holds a save" -
    /// in card-slots mode that is what the session gates its confirm on
    /// (see `SaveRack::CardPorts` and `save_screen::SaveScreenFlow`).
    ///
    /// The label carries the card's own name so the page can surface which
    /// image is in which port.
    pub(crate) fn card_slot_snapshots(&self) -> Vec<SlotSnapshot> {
        (0..CARD_SLOTS)
            .map(|i| match self.card(i) {
                Some(c) => SlotSnapshot {
                    slot: i as u8,
                    present: true,
                    damaged: false,
                    label: c.label.clone(),
                    ..SlotSnapshot::empty(i as u8)
                },
                None => SlotSnapshot::empty(i as u8),
            })
            .collect()
    }

    /// Per-**block** snapshots for the card in rack slot `slot`: the fifteen
    /// cells of the retail 5x3 preview grid, in block order (grid cell `i`
    /// = card block `i + 1`).
    ///
    /// Each present block is lifted through [`legaia_save::SaveFile::from_retail_sc_block`]
    /// so the grid's portraits and the info panel's name / level / HP / MP /
    /// location rows come off the real save, exactly as retail reads them
    /// out of its per-slot buffer at `0x801EF1B8 + N * 0x100`.
    /// The card's fifteen blocks as the 5x3 preview grid reads them, through
    /// the shared `engine_core::save_select::card_block_snapshots` kernel the
    /// native window calls too.
    pub(crate) fn card_block_snapshots(&self, slot: usize) -> Vec<SlotSnapshot> {
        match self.card(slot) {
            Some(card) => legaia_engine_core::save_select::card_block_snapshots(card),
            None => (0..CARD_BLOCKS).map(SlotSnapshot::empty).collect(),
        }
    }

    /// Write the live session into `block` of the card in rack slot `slot`.
    ///
    /// The bytes are decided by the shared kernel
    /// [`legaia_engine_core::card_write::write_save_into_card`] - the same one
    /// the native window's `--card` port writes through - and stamped **in
    /// place**, so every byte outside the block is the player's own card. The
    /// card is left dirty for the page to export.
    pub(crate) fn write_session_into_card(&mut self, slot: usize, block: u8) -> Result<(), String> {
        let sf = self.world_mut().save_full();
        let resume = self.current_resume();
        let index = self.scene_host.host().map(|h| h.index.clone());
        let card_slot = self
            .cards
            .get_mut(slot)
            .and_then(|c| c.as_mut())
            .ok_or_else(|| format!("no memory card in slot {}", slot + 1))?;
        let wrote = legaia_engine_core::card_write::write_save_into_card(
            card_slot,
            block,
            &sf,
            &resume,
            index.as_deref(),
        )?;
        if !wrote.ext_written {
            crate::console_log("play menu: engine ext too large for the card block; withheld");
        }
        if let Some(w) = self.cards_written.get_mut(slot) {
            *w = true;
        }
        Ok(())
    }

    /// JsValue-free core of [`Self::insert_card`] (JsValue panics off-wasm,
    /// so the testable body lives here - same split as
    /// [`crate::session_save`]).
    pub(crate) fn insert_card_core(
        &mut self,
        slot: u8,
        bytes: Vec<u8>,
        label: String,
    ) -> Result<String, String> {
        let slot = slot as usize;
        if slot >= CARD_SLOTS {
            return Err(format!(
                "insert_card: slot {slot} out of range (the console has {CARD_SLOTS} card ports)"
            ));
        }
        // Reject up front rather than at first Load: a card that can't be
        // parsed must never occupy a port.
        emu::detect(&bytes).map_err(|e| format!("insert_card: {e}"))?;
        self.cards[slot] =
            Some(MountedCard::from_bytes(bytes, label).map_err(|e| format!("insert_card: {e}"))?);
        Ok(self.card_slot_json(slot))
    }

    /// Load `block` of the card in rack slot `slot` into the live session.
    ///
    /// The block is read through [`MountedCard::save_at`] - the reader the
    /// native window's `--card` port loads with - so a block that does not
    /// open a save chain (a mid-chain continuation) is refused on both hosts
    /// alike. This used to re-detect the container and slice the SC block by
    /// hand, without the chain-start test. The save is parked for
    /// [`Self::play_resume_save`] ([`crate::resume`]); the return is the
    /// scene it names (empty when none).
    pub(crate) fn load_session_from_card(
        &mut self,
        slot: usize,
        block: u8,
    ) -> Result<String, String> {
        let card = self
            .card(slot)
            .ok_or_else(|| format!("no memory card in slot {}", slot + 1))?;
        let cell = block
            .checked_sub(1)
            .ok_or_else(|| "block 0 is the card directory, not a save".to_string())?;
        let (sf, resume) = card
            .save_at(cell)
            .ok_or_else(|| format!("card block {block} holds no save"))?;
        if sf.party.members.is_empty() {
            return Err("that block holds no character records".to_string());
        }
        self.park_loaded_save(sf, &resume.scene);
        Ok(resume.scene)
    }

    /// Where a save written now would resume - the engine's
    /// `SceneHost::current_resume`, the call the native session makes;
    /// empty with no scene loaded.
    pub(crate) fn current_resume(&self) -> SaveResume {
        self.scene_host
            .host()
            .map(|h| h.current_resume())
            .unwrap_or_default()
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Insert a memory-card image into rack slot `slot` (0 or 1 - the
    /// console's two ports).
    ///
    /// `bytes` is the container exactly as the player exported it from their
    /// emulator (`.mcr` / `.mcd` / `.gme` / `.mcs`); it is validated here and
    /// then kept verbatim, so [`Self::export_card`] can hand it back in the
    /// same shape. Returns the slot's JSON (same shape as one entry of
    /// [`Self::card_slots_json`]); throws on an unrecognised container.
    pub fn insert_card(
        &mut self,
        slot: u8,
        bytes: Vec<u8>,
        label: String,
    ) -> Result<String, JsValue> {
        self.insert_card_core(slot, bytes, label)
            .map_err(|e| JsValue::from_str(&e))
    }

    /// Remove the card from rack slot `slot`. Unexported writes are lost -
    /// the page warns before calling this.
    pub fn eject_card(&mut self, slot: u8) {
        if let Some(c) = self.cards.get_mut(slot as usize) {
            *c = None;
        }
    }

    /// `true` when the card in `slot` holds in-game writes the page has not
    /// exported yet.
    pub fn card_slot_dirty(&self, slot: u8) -> bool {
        self.card(slot as usize).map(|c| c.dirty).unwrap_or(false)
    }

    /// `true` once per in-game Save into the card in `slot`: the page polls
    /// this and stores the card back over its browser session, so a save
    /// survives a reload the way the native window's `card.persist()`
    /// survives a restart. Clears the latch; leaves the export `dirty` bit.
    pub fn card_take_written(&mut self, slot: u8) -> bool {
        self.cards_written
            .get_mut(slot as usize)
            .map(std::mem::take)
            .unwrap_or(false)
    }

    /// The card in `slot` as container bytes, without clearing its export
    /// `dirty` bit - what [`Self::card_take_written`]'s store uses. Empty
    /// when no card is in that slot.
    pub fn card_bytes(&self, slot: u8) -> Vec<u8> {
        self.card(slot as usize)
            .map(|c| c.bytes.clone())
            .unwrap_or_default()
    }

    /// The card in rack slot `slot`, as container bytes ready to download.
    ///
    /// Byte-identical to what was inserted apart from the SC blocks the
    /// player saved into, so the player's emulator loads it straight back.
    /// Empty when no card is in that slot. Clears the slot's dirty flag.
    pub fn export_card(&mut self, slot: u8) -> Vec<u8> {
        match self.cards.get_mut(slot as usize).and_then(|c| c.as_mut()) {
            Some(c) => {
                c.dirty = false;
                c.bytes.clone()
            }
            None => Vec::new(),
        }
    }

    /// The whole rack as JSON - what the page's card picker renders:
    /// ```text
    /// [ { "slot": 0, "inserted": true, "label": "my card", "format": "mcr",
    ///     "dirty": false,
    ///     "blocks": [ { "block": 1, "present": true, "name": "Vahn",
    ///                   "level": 12, "location": "Rim Elm", "money": 900 }, ... ] },
    ///   { "slot": 1, "inserted": false, ... } ]
    /// ```
    pub fn card_slots_json(&self) -> String {
        let slots: Vec<serde_json::Value> = (0..CARD_SLOTS)
            .map(|i| {
                serde_json::from_str(&self.card_slot_json(i)).unwrap_or(serde_json::Value::Null)
            })
            .collect();
        serde_json::Value::Array(slots).to_string()
    }
}

impl LegaiaRuntime {
    /// One rack slot's JSON (see [`Self::card_slots_json`]).
    fn card_slot_json(&self, slot: usize) -> String {
        let Some(c) = self.card(slot) else {
            return serde_json::json!({
                "slot": slot,
                "inserted": false,
                "label": "",
                "format": serde_json::Value::Null,
                "dirty": false,
                "blocks": [],
            })
            .to_string();
        };
        let format = c.view.format.label();
        let blocks: Vec<serde_json::Value> = self
            .card_block_snapshots(slot)
            .iter()
            .map(|s| {
                serde_json::json!({
                    "block": s.slot + 1,
                    "present": s.present,
                    "name": s.leader_name,
                    "level": s.party_lv,
                    "location": s.location,
                    "money": s.money,
                })
            })
            .collect();
        serde_json::json!({
            "slot": slot,
            "inserted": true,
            "label": c.label,
            "format": format,
            "dirty": c.dirty,
            "blocks": blocks,
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_engine_core::save_select::SlotContent;
    use legaia_save::card;

    /// A raw 128 KiB card with every block free - the same formatted image
    /// the page mounts as its browser card.
    fn blank_card() -> Vec<u8> {
        super::formatted_memory_card()
    }

    /// A card carrying one Legaia save in `block`, filed under save number
    /// `block - 1` (the number this rack derives).
    fn card_with_save(block: u8, name: &str, gold: i32) -> Vec<u8> {
        card_with_numbered_save(block, u32::from(block.saturating_sub(1)), name, gold)
    }

    /// A card carrying one Legaia save in `block` filed under an arbitrary
    /// save number - what a card written by **retail** looks like, since
    /// retail numbers a save by the save-select list position the player
    /// picked and not by the block the BIOS placed it in.
    fn card_with_numbered_save(block: u8, save_index: u32, name: &str, gold: i32) -> Vec<u8> {
        let mut buf = blank_card();
        let f = card::DIR_FRAME_SIZE * block as usize;
        buf[f..f + 4].copy_from_slice(&card::state::FIRST_BLOCK.to_le_bytes());
        buf[f + 8..f + 10].copy_from_slice(&0xFFFFu16.to_le_bytes());
        let filename = card::legaia_save_filename(save_index);
        buf[f + 10..f + 10 + filename.len()].copy_from_slice(filename.as_bytes());
        let b = card::BLOCK_SIZE * block as usize;
        let sc = &mut buf[b..b + card::BLOCK_SIZE];
        sc[..2].copy_from_slice(&card::SAVE_BLOCK_MAGIC);
        // A retail block carries a Shift-JIS title whose two digit cells
        // spell the save number; the identity writer patches those cells
        // in place and deliberately refuses to stamp them into an empty
        // title, so a fixture without one cannot exercise the patch.
        let title = &mut sc[card::RETAIL_TITLE_OFFSET..card::RETAIL_ICON_CLUT_OFFSET];
        title.fill(0x20);
        let digits = card::save_title_digits(save_index);
        for (off, d) in card::RETAIL_TITLE_DIGIT_OFFSETS.iter().zip(digits.iter()) {
            title[*off] = *d;
        }
        let mut rec = legaia_save::CharacterRecord::zeroed();
        rec.set_name(name);
        rec.set_magic_rank(12);
        rec.set_hp_mp_sp(legaia_save::HpMpSp {
            hp_cur: 180,
            hp_max: 200,
            mp_cur: 20,
            mp_max: 30,
            sp_cur: 0,
            sp_max: 0,
        });
        legaia_save::write_retail_char_records(sc, std::slice::from_ref(&rec.raw)).unwrap();
        legaia_save::write_retail_gold(sc, gold).unwrap();
        buf
    }

    fn rt_with_card(slot: u8, bytes: Vec<u8>) -> LegaiaRuntime {
        let mut rt = LegaiaRuntime::new();
        rt.insert_card_core(slot, bytes, "test card".into())
            .unwrap();
        rt
    }

    #[test]
    fn insert_rejects_garbage_and_out_of_range_slots() {
        let mut rt = LegaiaRuntime::new();
        assert!(
            rt.insert_card_core(0, vec![0u8; 64], "junk".into())
                .is_err()
        );
        assert!(
            rt.insert_card_core(2, blank_card(), "third port".into())
                .is_err()
        );
        assert!(
            rt.card(0).is_none(),
            "a rejected card must not occupy a port"
        );
    }

    #[test]
    fn card_slot_snapshots_track_insertion() {
        let mut rt = rt_with_card(0, blank_card());
        let snaps = rt.card_slot_snapshots();
        assert_eq!(snaps.len(), CARD_SLOTS, "one pill per console card port");
        assert!(snaps[0].present, "slot 1 holds a card");
        assert!(!snaps[1].present, "slot 2 is empty");
        rt.eject_card(0);
        assert!(!rt.card_slot_snapshots()[0].present);
    }

    #[test]
    fn block_snapshots_read_the_cards_real_saves() {
        let rt = rt_with_card(0, card_with_save(3, "Vahn", 900));
        let blocks = rt.card_block_snapshots(0);
        assert_eq!(blocks.len(), CARD_BLOCKS as usize, "5x3 preview grid");
        // Grid cell i = card block i+1, so block 3 is cell 2.
        let cell = &blocks[2];
        assert!(cell.present);
        assert_eq!(cell.leader_name, "Vahn");
        assert_eq!(cell.party_lv, 12);
        assert_eq!(cell.money, 900);
        assert_eq!(cell.leader_hp, (180, 200));
        assert_eq!(cell.leader_mp, (20, 30));
        assert!(!blocks[0].present, "block 1 is free");
        assert!(!blocks[14].present, "block 15 is free");
        assert!(
            blocks
                .iter()
                .filter(|b| !b.present)
                .all(|b| b.content == SlotContent::Free),
            "a well-formed card's unclaimed cells caption free (budget affords them)"
        );
    }

    /// The retail free-block budget (`card_directory_scan` ->
    /// `card_free_blocks`): a card whose directory declares more used bytes
    /// than its claims account for stops paying for "free" captions, and the
    /// unafforded cells caption foreign instead of inviting an overwrite.
    #[test]
    fn block_snapshots_spend_the_retail_free_block_budget() {
        let mut bytes = card_with_save(3, "Vahn", 900);
        // Declare the save as spanning the whole card (15 blocks) in its
        // directory frame's size field - the card now reports zero free
        // blocks even though 14 cells carry no claim.
        let f = card::DIR_FRAME_SIZE * 3;
        bytes[f + 4..f + 8].copy_from_slice(&(15u32 * card::BLOCK_SIZE as u32).to_le_bytes());
        let ck = bytes[f..f + 0x7F].iter().fold(0u8, |a, &b| a ^ b);
        bytes[f + 0x7F] = ck;
        let rt = rt_with_card(0, bytes);
        let blocks = rt.card_block_snapshots(0);
        assert_eq!(blocks[2].content, SlotContent::LegaiaSave);
        assert!(
            blocks
                .iter()
                .filter(|b| b.content != SlotContent::LegaiaSave)
                .all(|b| b.content == SlotContent::Foreign),
            "no free-block budget left: unclaimed cells caption foreign"
        );
    }

    #[test]
    fn save_into_existing_block_round_trips_and_preserves_the_rest() {
        let original = card_with_save(3, "Vahn", 900);
        let mut rt = rt_with_card(0, original.clone());
        rt.world_mut().party.money = 4321;
        rt.world_mut().load_party(legaia_save::Party {
            members: vec![{
                let mut r = legaia_save::CharacterRecord::zeroed();
                r.set_name("Noa");
                r.set_magic_rank(31);
                r
            }],
        });
        rt.write_session_into_card(0, 3).unwrap();
        assert!(rt.card_slot_dirty(0), "an in-game save dirties the card");
        // The page's store latch fires once per save and leaves the export bit.
        assert!(rt.card_take_written(0), "the save raises the store latch");
        assert!(!rt.card_take_written(0), "the latch is taken once");
        assert!(!rt.card_take_written(1), "the other port saw no save");
        assert_eq!(rt.card_bytes(0).len(), original.len());
        assert!(rt.card_slot_dirty(0), "storing does not count as an export");

        // Re-parse off the exported container: this is what an emulator sees.
        let exported = rt.export_card(0);
        assert!(!rt.card_slot_dirty(0), "export clears the dirty flag");
        assert_eq!(exported.len(), original.len(), "container shape preserved");
        let rt2 = rt_with_card(0, exported.clone());
        let cell = &rt2.card_block_snapshots(0)[2];
        assert!(cell.present);
        assert_eq!(cell.leader_name, "Noa");
        assert_eq!(cell.party_lv, 31);
        assert_eq!(cell.money, 4321);

        // Only block 3 may have moved - the card header, the directory and
        // every other block are the player's bytes and must be untouched.
        let b3 = card::BLOCK_SIZE * 3;
        let changed_outside: Vec<usize> = original
            .iter()
            .zip(exported.iter())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i)
            .filter(|i| !(b3..b3 + card::BLOCK_SIZE).contains(i))
            .collect();
        assert!(
            changed_outside.is_empty(),
            "writes escaped block 3: {changed_outside:?}"
        );
    }

    #[test]
    fn save_into_free_block_claims_its_directory_frame() {
        // A free block has no directory frame, so the save must stamp one or
        // the emulator's card browser will not see the save at all.
        let mut rt = rt_with_card(0, blank_card());
        rt.world_mut().party.money = 77;
        rt.world_mut().load_party(legaia_save::Party {
            members: vec![{
                let mut r = legaia_save::CharacterRecord::zeroed();
                r.set_name("Vahn");
                r
            }],
        });
        rt.write_session_into_card(0, 5).unwrap();
        let exported = rt.export_card(0);

        // The generic card walker (what an emulator uses) must find it.
        let saves = card::parse_card(&exported).expect("card still parses");
        assert_eq!(saves.len(), 1, "exactly the save we just wrote");
        assert_eq!(saves[0].block, 5);
        assert!(saves[0].product_code.starts_with("BASCUS-94254"));
        // And the frame's XOR checksum must be right.
        let f = card::DIR_FRAME_SIZE * 5;
        let expect = exported[f..f + 0x7F].iter().fold(0u8, |a, &b| a ^ b);
        assert_eq!(exported[f + 0x7F], expect, "directory-frame XOR checksum");

        let rt2 = rt_with_card(0, exported);
        assert!(rt2.card_block_snapshots(0)[4].present, "cell 4 = block 5");
    }

    /// A save the page writes with the disc loaded must load back. With the
    /// disc in hand the writer stamps the slot portrait, which sits inside
    /// the block's summed range; stamping it after the checksum left every
    /// browser save captioned "Damaged data." on the next Load.
    #[test]
    fn a_save_written_with_the_disc_loads_back_undamaged() {
        let Some(bytes) = std::env::var("LEGAIA_DISC_BIN")
            .ok()
            .and_then(|p| std::fs::read(p).ok())
        else {
            eprintln!("LEGAIA_DISC_BIN unset - skipping");
            return;
        };
        let mut rt = LegaiaRuntime::new();
        rt.load_disc(bytes, String::new()).expect("disc loads");
        rt.enter_field("town01").expect("enter town01");
        rt.insert_card_core(0, blank_card(), "browser".into())
            .expect("blank card mounts");
        for block in [1u8, 1, 2] {
            rt.write_session_into_card(0, block).unwrap();
            let cell = &rt.card_block_snapshots(0)[block as usize - 1];
            assert!(cell.present, "block {block} holds the save");
            assert!(!cell.damaged, "block {block} must verify on Load");
        }
        eprintln!("[ran] disc-backed save verifies on load");
    }

    /// Every filename on a memory card must be unique - the BIOS
    /// directory is keyed by it. A card written by retail numbers its
    /// saves by the save-select list position, so a save filed as `-03`
    /// can sit in any block; deriving the number from the block alone
    /// then hands a second block the same filename.
    #[test]
    fn saving_never_duplicates_a_filename_already_on_the_card() {
        // Retail card: block 1 holds save number 3.
        let mut rt = rt_with_card(0, card_with_numbered_save(1, 3, "Vahn", 100));
        rt.world_mut().party.money = 5;
        rt.world_mut().load_party(legaia_save::Party {
            members: vec![{
                let mut r = legaia_save::CharacterRecord::zeroed();
                r.set_name("Noa");
                r
            }],
        });
        // `block - 1` would be 3 here, colliding with block 1's file.
        rt.write_session_into_card(0, 4).unwrap();
        let exported = rt.export_card(0);

        let names: Vec<String> = (1..=CARD_BLOCKS)
            .filter_map(|b| {
                let f = card::DIR_FRAME_SIZE * b as usize;
                let n: String = exported[f + 10..f + 30]
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as char)
                    .collect();
                (!n.is_empty()).then_some(n)
            })
            .collect();
        let mut uniq = names.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), names.len(), "duplicate filenames: {names:?}");
        // The generic card walker must still see both saves.
        assert_eq!(card::parse_card(&exported).expect("parses").len(), 2);
    }

    /// Overwriting a block keeps its **existing** save number, because the
    /// directory filename is not rewritten on an overwrite: deriving a
    /// different number would leave the block's own title digits and its
    /// filename disagreeing about which save it is.
    #[test]
    fn overwriting_a_block_keeps_the_number_its_filename_carries() {
        let mut rt = rt_with_card(0, card_with_numbered_save(2, 7, "Vahn", 100));
        rt.world_mut().load_party(legaia_save::Party {
            members: vec![legaia_save::CharacterRecord::zeroed()],
        });
        rt.write_session_into_card(0, 2).unwrap();
        let exported = rt.export_card(0);

        let f = card::DIR_FRAME_SIZE * 2;
        let name: String = exported[f + 10..f + 30]
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as char)
            .collect();
        assert_eq!(name, card::legaia_save_filename(7), "filename untouched");
        let b = card::BLOCK_SIZE * 2 + card::RETAIL_TITLE_OFFSET;
        let title = &exported[b..b + 0x5C];
        assert_eq!(
            [
                title[card::RETAIL_TITLE_DIGIT_OFFSETS[0]],
                title[card::RETAIL_TITLE_DIGIT_OFFSETS[1]]
            ],
            card::save_title_digits(7),
            "the block title must agree with the filename"
        );
    }

    #[test]
    fn untouched_card_exports_byte_identical() {
        let original = card_with_save(1, "Vahn", 100);
        let mut rt = rt_with_card(0, original.clone());
        assert!(!rt.card_slot_dirty(0));
        assert_eq!(rt.export_card(0), original, "no writes = no changes");
    }

    #[test]
    fn load_from_card_lifts_the_block_into_the_world() {
        let mut rt = rt_with_card(1, card_with_save(2, "Gala", 555));
        rt.load_session_from_card(1, 2).expect("load");
        // The Load parks; the page's resume call lands it.
        rt.resume_parked_save().expect("a parked save");
        assert_eq!(rt.world_mut().party.money, 555);
        assert_eq!(rt.world_mut().party.roster.members.len(), 1);
        assert_eq!(rt.world_mut().party.roster.members[0].name(), "Gala");
    }

    /// A card Load resumes the save's own story state. The page lifts the
    /// block, then lands it in the scene the save names; that entry used to stage
    /// the scene picker's free-roam baseline over the loaded flags, clearing
    /// system flags `0x141` / `0x147` (the Rim Elm south-gate beat) in every
    /// resumed save. Disc-gated: `enter_field` needs the disc.
    #[test]
    fn a_card_load_keeps_the_saves_story_flags_across_the_scene_entry() {
        let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
            return;
        };
        let Ok(bytes) = std::fs::read(&disc) else {
            eprintln!("[skip] disc unreadable (disc-gated)");
            return;
        };
        let mut buf = card_with_save(1, "Vahn", 321);
        let b = card::BLOCK_SIZE;
        {
            let sc = &mut buf[b..b + card::BLOCK_SIZE];
            // System flags live at story-flag window `+0x158`, MSB-first.
            let mut bits = vec![0u8; card::RETAIL_STORY_FLAGS_SIZE];
            for f in [0x141u16, 0x147] {
                bits[0x158 + usize::from(f >> 3)] |= 0x80 >> (f & 7);
            }
            card::write_retail_story_flags(sc, &bits).unwrap();
            card::write_retail_resume(sc, "town01", "Rim Elm").unwrap();
        }
        let mut rt = LegaiaRuntime::new();
        rt.load_disc(bytes, String::new()).expect("load_disc");
        rt.insert_card_core(0, buf, "test card".into()).unwrap();
        let scene = rt.load_session_from_card(0, 1).expect("load");
        assert_eq!(scene, "town01");
        // The page's one resume call (`play_resume_save`).
        assert_eq!(
            rt.resume_parked_save(),
            Some(legaia_engine_core::resume::ResumeLanding::SavedScene(
                "town01".into()
            ))
        );
        let w = rt.world_mut();
        assert!(w.system_flag_test(0x141), "0x141 survived the resume");
        assert!(w.system_flag_test(0x147), "0x147 survived the resume");
        assert_eq!(w.party.money, 321);
        assert!(
            !w.field_vm.free_roam_staging,
            "a resume is not a picker visit"
        );
        eprintln!("[ok] card resume kept 0x141 / 0x147");
    }

    #[test]
    fn load_and_save_reject_an_empty_port() {
        let mut rt = LegaiaRuntime::new();
        assert!(rt.load_session_from_card(0, 1).is_err());
        assert!(rt.write_session_into_card(0, 1).is_err());
    }

    #[test]
    fn slots_json_describes_both_ports() {
        let rt = rt_with_card(0, card_with_save(1, "Vahn", 900));
        let v: serde_json::Value = serde_json::from_str(&rt.card_slots_json()).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), CARD_SLOTS);
        assert_eq!(arr[0]["inserted"], true);
        assert_eq!(arr[0]["format"], "mcr");
        assert_eq!(
            arr[0]["blocks"].as_array().unwrap().len(),
            CARD_BLOCKS as usize
        );
        assert_eq!(arr[0]["blocks"][0]["present"], true);
        assert_eq!(arr[0]["blocks"][0]["name"], "Vahn");
        assert_eq!(arr[1]["inserted"], false);
    }
}
