//! [`SaveFile`] to and from a retail SC block, and the `LGXE` engine
//! extension that rides in the unused tail of the block.

use super::*;

impl SaveFile {
    /// Build a [`SaveFile`] from a retail SC save block (8 KiB block whose
    /// first two bytes are [`crate::SAVE_BLOCK_MAGIC`]).
    ///
    /// Reads party records, the story-flag window, the **whole
    /// 256-slot** item array, and the party gold
    /// ([`crate::card::RETAIL_GOLD_OFFSET`], mirrors RAM `0x8008459C`) at
    /// their pinned offsets. [`SaveExt::item_slots`] keeps the array verbatim,
    /// slot order and holes intact; [`SaveExt::inventory`] is the compact view
    /// of the same bytes, with the `(0, 0)` slots dropped.
    ///
    /// The walk is the full `0x200`-byte span
    /// ([`crate::card::read_retail_item_window`]) and not the 72-slot
    /// consumable **page** - 72 is the size of a cheat device's display page,
    /// not an engine bound, and the accessors bound on `gp[+0x2D4]`, which is
    /// only ever 128 or 256 (`docs/subsystems/inventory.md`). Lifting only the
    /// page silently dropped every item a played-through save holds above slot
    /// 71.
    ///
    /// `max_records` caps the party walk - retail saves never hold more
    /// than 4 active records (Vahn / Noa / Gala / Terra).
    pub fn from_retail_sc_block(sc_block: &[u8], max_records: usize) -> Result<Self> {
        let party = Party::from_retail_sc_block(sc_block, max_records)
            .context("LGSF: parse party records from retail SC block")?;
        let story_flag_bits = crate::card::read_retail_story_flags(sc_block)
            .map(<[u8]>::to_vec)
            .unwrap_or_default();
        let item_slots: Vec<(u8, u8)> = crate::card::read_retail_item_window(sc_block)
            .map(|raw| {
                raw.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| (p[0], p[1]))
                    .collect()
            })
            .unwrap_or_default();
        let inventory: Vec<(u8, u8)> = item_slots
            .iter()
            .copied()
            .filter(|&(id, count)| !(id == 0 && count == 0))
            .collect();
        let story_flags = story_flag_bits
            .get(..4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .unwrap_or(0);
        let money = crate::card::read_retail_gold(sc_block).unwrap_or(0);
        let minigames = crate::MinigameSave::from_retail_sc_block(sc_block).unwrap_or_default();
        // The engine-ext blob in the block's unread tail. A retail block is
        // zero there (no magic) and reads as the default, exactly as before.
        let engine_ext = Self::read_engine_ext_from_retail_sc_block(sc_block);
        let authored_by_engine = engine_ext.is_some();
        let mut ext_v2 = engine_ext.unwrap_or_default();
        // The present party is the block's own count + member list
        // (`0x80084594` / `0x80084598`), never the number of populated
        // records: the New Game template seeds all four records, so a
        // record count makes a Vahn-alone save a four-member party. An
        // engine-authored block keeps the composition its `LGXE` blob names.
        if !authored_by_engine
            && let Some(members) = crate::card::read_retail_present_party(sc_block)
        {
            ext_v2.active_party = members;
        }
        // Where the player stood - what retail's MAN loader seats the party
        // at after a card load.
        ext_v2.field_position = crate::card::read_retail_field_position(sc_block);
        // The configured audio level and the voice volume - live-state words
        // retail's card load restores with the rest of the window, and which
        // the next scene load re-applies (`FUN_8003AEB0` copies the level
        // into the live `_DAT_8007B910`).
        ext_v2.audio_levels = crate::card::read_retail_audio_levels(sc_block);
        // The options screen's words, which retail's card load restores with
        // the window: a save made running resumes running. Read only beside a
        // real audio pair - a block with neither is a free or never-loaded
        // window, and its zero words are not a setting anyone chose.
        ext_v2.options = ext_v2
            .audio_levels
            .and_then(|_| crate::card::read_retail_options(sc_block));
        // The play clock: retail's `1/60` s counter, which the engine keeps in
        // whole seconds. An engine-authored block's `LGXE` blob carries the
        // exact seconds it wrote and keeps them.
        if !authored_by_engine && let Some(ticks) = crate::card::read_retail_play_counter(sc_block)
        {
            ext_v2.play_time_seconds = ticks / crate::card::RETAIL_PLAY_COUNTER_HZ;
        }
        Ok(Self {
            party,
            ext: SaveExt {
                story_flags,
                story_flag_bits,
                money,
                inventory,
                item_slots,
                minigames,
            },
            ext_v2,
        })
    }

    /// Read the `LGXE` engine-ext blob off a retail SC block's unread tail
    /// (see the module doc). `None` when the magic is absent - every retail
    /// block - or the blob does not parse; the caller falls back to
    /// [`SaveExtV2::default`], so a damaged blob costs the play clock, never
    /// the save.
    pub fn read_engine_ext_from_retail_sc_block(sc_block: &[u8]) -> Option<SaveExtV2> {
        let tail = sc_block.get(RETAIL_ENGINE_EXT_OFFSET..RETAIL_BLOCK_CHECKSUM_END)?;
        if tail.get(..4)? != RETAIL_ENGINE_EXT_MAGIC {
            return None;
        }
        let mut p = 4usize;
        let v2_len = u16::from_le_bytes(tail.get(p..p + 2)?.try_into().ok()?) as usize;
        p += 2;
        let v2 = tail.get(p..p + v2_len)?;
        p += v2_len;
        let v4_len = u16::from_le_bytes(tail.get(p..p + 2)?.try_into().ok()?) as usize;
        p += 2;
        let v4 = tail.get(p..p + v4_len)?;
        let mut ext_v2 = parse_ext_v2(v2).ok()?;
        apply_ext_v4(v4, &mut ext_v2).ok()?;
        Some(ext_v2)
    }

    /// Write this save's `LGX2` / `LGX4` payload into a retail SC block's
    /// unread tail (`0x1A18..0x1FFC`) and restamp the block checksum. Returns
    /// `Ok(false)` - with the tail zeroed - when the blob does not fit the
    /// `0x5E4`-byte capacity (a chain library of hundreds of named chains);
    /// the retail regions are untouched either way. Deliberately not part of
    /// [`Self::write_into_retail_sc_block`]: that composer's region list is
    /// pinned by its own test, and this region is the one retail never reads.
    pub fn write_engine_ext_into_retail_sc_block(&self, sc_block: &mut [u8]) -> Result<bool> {
        let Some(tail) = sc_block.get_mut(RETAIL_ENGINE_EXT_OFFSET..RETAIL_BLOCK_CHECKSUM_END)
        else {
            bail!(
                "sc_block too small for the engine-ext tail (need {RETAIL_BLOCK_CHECKSUM_END} bytes)"
            );
        };
        tail.fill(0);
        let v2 = self.ext_v2_body();
        let v4 = self.ext_v4_body();
        let need = 4 + 2 + v2.len() + 2 + v4.len();
        let fits =
            need <= tail.len() && v2.len() <= u16::MAX as usize && v4.len() <= u16::MAX as usize;
        if fits {
            tail[..4].copy_from_slice(&RETAIL_ENGINE_EXT_MAGIC);
            let mut p = 4;
            tail[p..p + 2].copy_from_slice(&(v2.len() as u16).to_le_bytes());
            p += 2;
            tail[p..p + v2.len()].copy_from_slice(&v2);
            p += v2.len();
            tail[p..p + 2].copy_from_slice(&(v4.len() as u16).to_le_bytes());
            p += 2;
            tail[p..p + v4.len()].copy_from_slice(&v4);
        }
        crate::card::restamp_sc_block_checksum(sc_block);
        Ok(fits)
    }

    /// Write this save into a retail SC save block in place.
    ///
    /// Stamps the SC magic at offset 0, then dispatches each region through
    /// the corresponding `write_retail_*` helper. The SC block must be at
    /// least the size of the largest region's end (handled by the underlying
    /// helpers). `ext.story_flag_bits` shorter than the window is right-padded
    /// with zeros; longer is truncated. Inventory slots past the 72-slot
    /// retail cap are dropped.
    ///
    /// `money` is written to the pinned retail gold slot
    /// ([`crate::card::RETAIL_GOLD_OFFSET`]), so gold round-trips through
    /// the retail block like the party / flags / inventory do.
    ///
    /// Each helper restamps the block's additive checksum
    /// ([`crate::card::restamp_sc_block_checksum`]), so a full-size block
    /// comes out of here in the state retail's loader accepts. On a block
    /// shorter than [`crate::card::BLOCK_SIZE`] there is no checksum word to
    /// restamp and the regions are simply written.
    pub fn write_into_retail_sc_block(&self, sc_block: &mut [u8]) -> Result<()> {
        if sc_block.len() < crate::card::SAVE_BLOCK_HEADER.len() {
            bail!("sc_block too small to hold the SC header");
        }
        // The whole four-byte header, not just the two magic bytes. `+2` is
        // the icon-frame descriptor and `+3` the block count; leaving them as
        // found means a previously-free block gets a correct payload behind a
        // header the BIOS reads as junk. The rest of the block identity (the
        // save-number digits and the slot's portrait) needs a slot and the
        // disc, so it lives in `card::write_retail_block_identity`.
        sc_block[..crate::card::SAVE_BLOCK_HEADER.len()]
            .copy_from_slice(&crate::card::SAVE_BLOCK_HEADER);
        let records: Vec<Vec<u8>> = self.party.members.iter().map(|m| m.raw.to_vec()).collect();
        crate::card::write_retail_char_records(sc_block, &records)?;
        crate::card::write_retail_story_flags(sc_block, &self.ext.story_flag_bits)?;
        // The array wins when this save carries one: it is retail's own slot
        // layout and must land verbatim, holes included. A save with only the
        // compact list goes through retail's add path instead, which is what
        // makes a list representable as a block at all.
        if self.ext.item_slots.is_empty() {
            crate::card::write_retail_inventory(sc_block, &self.ext.inventory)?;
        } else {
            crate::card::write_retail_item_window(sc_block, &self.ext.item_slots)?;
        }
        crate::card::write_retail_gold(sc_block, self.ext.money)?;
        // The minigame purses sit in the same live-state window as the gold.
        self.ext.minigames.write_into_retail_sc_block(sc_block)?;
        // The present party and the field position: the two words retail's
        // loader reads back that a lift fills from the block, so a block
        // written here resumes with the party and at the place it names.
        // A save without either leaves the block's bytes as found.
        if (1..=crate::card::RETAIL_PARTY_MEMBERS_MAX).contains(&self.ext_v2.active_party.len()) {
            crate::card::write_retail_present_party(sc_block, &self.ext_v2.active_party)?;
        }
        if let Some(pos) = self.ext_v2.field_position {
            crate::card::write_retail_field_position(sc_block, pos)?;
        }
        if let Some(levels) = self.ext_v2.audio_levels {
            crate::card::write_retail_audio_levels(sc_block, levels)?;
        }
        if let Some(options) = self.ext_v2.options {
            crate::card::write_retail_options(sc_block, options)?;
        }
        // The play clock, so retail's save screen prints the time this save
        // was played for and retail's own clock carries on from it.
        crate::card::write_retail_play_counter(
            sc_block,
            self.ext_v2
                .play_time_seconds
                .saturating_mul(crate::card::RETAIL_PLAY_COUNTER_HZ),
        )?;
        Ok(())
    }
}
