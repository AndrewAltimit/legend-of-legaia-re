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

use legaia_engine_core::save_select::{SlotContent, SlotSnapshot};
use legaia_save::emu::{self, CardView};
use legaia_save::{SaveFile, card};
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

/// The product code retail stamps into a Legaia save's directory frame
/// (USA, `SCUS-94254`). Written when a save claims a previously-free block
/// so the emulator's card browser labels it like a real Legaia save.
/// REF: FUN_801E1208 matches `BASCUS-94254PRO_` when enumerating slots.
const LEGAIA_PRODUCT_CODE: &str = "BASCUS-94254PRO_00";

/// One inserted card.
pub struct InsertedCard {
    /// The container bytes, exactly as imported plus any in-place SC-block
    /// writes. Never re-encoded.
    pub bytes: Vec<u8>,
    /// Display label the page gave it (its file / save name).
    pub label: String,
    /// `true` once an in-game save has written into this card and the page
    /// has not exported it since.
    pub dirty: bool,
}

impl InsertedCard {
    fn view(&self) -> Option<CardView> {
        emu::detect(&self.bytes).ok()
    }
}

/// Read a NUL-terminated ASCII field out of an SC block.
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

impl LegaiaRuntime {
    /// The card in rack slot `slot`, if one is inserted.
    pub(crate) fn card(&self, slot: usize) -> Option<&InsertedCard> {
        self.cards.get(slot).and_then(|c| c.as_ref())
    }

    /// Per-**card-slot** snapshots: the pill row of the retail save screen.
    /// `present` means "a card is inserted here", not "this holds a save" -
    /// in card-slots mode that is what the session gates its confirm on
    /// (see `SaveSelectSession::set_card_slots_mode`).
    ///
    /// The label carries the card's own name so the page can surface which
    /// image is in which port.
    pub(crate) fn card_slot_snapshots(&self) -> Vec<SlotSnapshot> {
        (0..CARD_SLOTS)
            .map(|i| match self.card(i) {
                Some(c) => SlotSnapshot {
                    slot: i as u8,
                    present: true,
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
    /// Each present block is lifted through [`SaveFile::from_retail_sc_block`]
    /// so the grid's portraits and the info panel's name / level / HP / MP /
    /// location rows come off the real save, exactly as retail reads them
    /// out of its per-slot buffer at `0x801EF1B8 + N * 0x100`.
    pub(crate) fn card_block_snapshots(&self, slot: usize) -> Vec<SlotSnapshot> {
        let Some(cardslot) = self.card(slot) else {
            return (0..CARD_BLOCKS).map(SlotSnapshot::empty).collect();
        };
        let Some(view) = cardslot.view() else {
            return (0..CARD_BLOCKS).map(SlotSnapshot::empty).collect();
        };
        (0..CARD_BLOCKS)
            .map(|cell| {
                let block = cell + 1;
                // A block nothing claims is free; the info panel says so.
                if !view.block_is_save_start(&cardslot.bytes, block) {
                    return SlotSnapshot::empty(cell);
                }
                // Past here the block IS claimed, so every way of failing to
                // read it is someone else's save rather than a free block -
                // a distinction retail captions differently.
                let Some(sc) = view.sc_block(&cardslot.bytes, block) else {
                    return SlotSnapshot::foreign(cell);
                };
                let Ok(sf) = SaveFile::from_retail_sc_block(sc, 4) else {
                    return SlotSnapshot::foreign(cell);
                };
                let Some(leader) = sf.party.members.first() else {
                    return SlotSnapshot::foreign(cell);
                };
                let hp = leader.hp_mp_sp();
                let name = leader.name();
                let location = sc_ascii(sc, card::RETAIL_LOCATION_NAME_OFFSET, 0x40);
                SlotSnapshot {
                    slot: cell,
                    present: true,
                    content: SlotContent::LegaiaSave,
                    label: if name.is_empty() {
                        format!("Block {block}")
                    } else {
                        name.clone()
                    },
                    // Retail's displayed level byte (record +0x130).
                    party_lv: leader.magic_rank(),
                    location,
                    money: sf.ext.money.max(0) as u32,
                    // The lead roster slot is Vahn on every retail save, and
                    // the portrait set is Vahn / Noa / Gala in char order.
                    leader_char_id: 0,
                    leader_name: name,
                    leader_hp: (hp.hp_cur, hp.hp_max),
                    leader_mp: (hp.mp_cur, hp.mp_max),
                    play_time_seconds: sf.ext_v2.play_time_seconds,
                }
            })
            .collect()
    }

    /// Write the live session into `block` of the card in rack slot `slot`.
    ///
    /// The SC payload is rebuilt from the world through
    /// [`SaveFile::write_into_retail_sc_block`] and stamped **in place** -
    /// every byte outside the block (other saves, the container header) is
    /// preserved, so the result is still the player's own card. A block that
    /// was free also gets its directory frame claimed.
    pub(crate) fn write_session_into_card(&mut self, slot: usize, block: u8) -> Result<(), String> {
        let sf = self.world_mut().save_full();
        let card_slot = self
            .cards
            .get_mut(slot)
            .and_then(|c| c.as_mut())
            .ok_or_else(|| format!("no memory card in slot {}", slot + 1))?;
        let view = emu::detect(&card_slot.bytes).map_err(|e| format!("{e}"))?;
        let was_active = view.block_is_save_start(&card_slot.bytes, block);
        let sc = view
            .sc_block_mut(&mut card_slot.bytes, block)
            .ok_or_else(|| format!("card has no block {block}"))?;
        sf.write_into_retail_sc_block(sc)
            .map_err(|e| format!("save: {e}"))?;
        if !was_active {
            view.claim_block(&mut card_slot.bytes, block, LEGAIA_PRODUCT_CODE)
                .map_err(|e| format!("{e}"))?;
        }
        card_slot.dirty = true;
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
        self.cards[slot] = Some(InsertedCard {
            bytes,
            label,
            dirty: false,
        });
        Ok(self.card_slot_json(slot))
    }

    /// Load `block` of the card in rack slot `slot` into the live session.
    pub(crate) fn load_session_from_card(
        &mut self,
        slot: usize,
        block: u8,
    ) -> Result<String, String> {
        let card_slot = self
            .card(slot)
            .ok_or_else(|| format!("no memory card in slot {}", slot + 1))?;
        let view = emu::detect(&card_slot.bytes).map_err(|e| format!("{e}"))?;
        let sc = view
            .sc_block(&card_slot.bytes, block)
            .ok_or_else(|| format!("card has no block {block}"))?;
        let sf = SaveFile::from_retail_sc_block(sc, 4)
            .map_err(|e| format!("not a valid retail save: {e}"))?;
        if sf.party.members.is_empty() {
            return Err("that block holds no character records".to_string());
        }
        let scene = sc_ascii(sc, card::RETAIL_SCENE_LABEL_OFFSET, 0x10);
        self.world_mut().load_full(sf);
        Ok(scene)
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
        let format = c.view().map(|v| v.format.label()).unwrap_or("?");
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

    /// A raw 128 KiB card with every block free.
    fn blank_card() -> Vec<u8> {
        let mut buf = vec![0u8; card::CARD_SIZE];
        buf[..2].copy_from_slice(&card::CARD_MAGIC);
        for i in 1..=card::DIR_FRAMES {
            let off = card::DIR_FRAME_SIZE * i;
            buf[off..off + 4].copy_from_slice(&card::state::FREE.to_le_bytes());
            let ck = buf[off..off + 0x7F].iter().fold(0u8, |a, &b| a ^ b);
            buf[off + 0x7F] = ck;
        }
        buf
    }

    /// A card carrying one Legaia save in `block`.
    fn card_with_save(block: u8, name: &str, gold: i32) -> Vec<u8> {
        let mut buf = blank_card();
        let f = card::DIR_FRAME_SIZE * block as usize;
        buf[f..f + 4].copy_from_slice(&card::state::FIRST_BLOCK.to_le_bytes());
        buf[f + 8..f + 10].copy_from_slice(&0xFFFFu16.to_le_bytes());
        buf[f + 10..f + 22].copy_from_slice(b"BASCUS-94254");
        let b = card::BLOCK_SIZE * block as usize;
        let sc = &mut buf[b..b + card::BLOCK_SIZE];
        sc[..2].copy_from_slice(&card::SAVE_BLOCK_MAGIC);
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
    }

    #[test]
    fn save_into_existing_block_round_trips_and_preserves_the_rest() {
        let original = card_with_save(3, "Vahn", 900);
        let mut rt = rt_with_card(0, original.clone());
        rt.world_mut().money = 4321;
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
        rt.world_mut().money = 77;
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
        assert_eq!(rt.world_mut().money, 555);
        assert_eq!(rt.world_mut().roster.members.len(), 1);
        assert_eq!(rt.world_mut().roster.members[0].name(), "Gala");
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
