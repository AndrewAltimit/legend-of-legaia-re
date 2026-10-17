//! `LGSF` serialisation: the v1-compatible prelude and the `LGX*` blocks.

use super::*;

impl SaveFile {
    /// Serialise to `LGSF` bytes (a v1-compatible prelude, then the `LGX2`
    /// / `LGX3` / `LGX4` blocks). No resume trailer: identical to
    /// [`Self::write_with_resume`] with an empty [`SaveResume`].
    pub fn write(&self) -> Vec<u8> {
        self.write_with_resume(&SaveResume::default())
    }

    /// [`Self::write`] plus the optional `LGX5` resume trailer, appended only
    /// when `resume` carries a scene or a location (see the module doc).
    pub fn write_with_resume(&self, resume: &SaveResume) -> Vec<u8> {
        let mut out = self.write_v4_body();
        if !resume.is_empty() {
            let block = resume_block(resume);
            out.extend_from_slice(&SAVE_FILE_EXT5_MAGIC);
            out.extend_from_slice(&(block.len() as u32).to_le_bytes());
            out.extend_from_slice(&block);
        }
        out
    }

    /// The `LGX2` block body (no magic / size header) - the play clock,
    /// party composition, per-character ext and chain library.
    pub(super) fn ext_v2_body(&self) -> Vec<u8> {
        let mut ext_block = Vec::new();
        ext_block.extend_from_slice(&self.ext_v2.play_time_seconds.to_le_bytes());
        let active_len = self.ext_v2.active_party.len().min(255) as u8;
        ext_block.push(active_len);
        for &cs in self.ext_v2.active_party.iter().take(active_len as usize) {
            ext_block.push(cs);
        }
        let pc_count = self.ext_v2.per_char.len().min(255) as u8;
        ext_block.push(pc_count);
        for (cs, ce) in self.ext_v2.per_char.iter().take(pc_count as usize) {
            ext_block.push(*cs);
            ext_block.extend_from_slice(&ce.learned_arts_mask.to_le_bytes());
            let s_len = ce.spells.len().min(255) as u8;
            ext_block.push(s_len);
            ext_block.extend_from_slice(&ce.spells[..s_len as usize]);
            let t_len = ce.seru_captures.len().min(255) as u8;
            ext_block.push(t_len);
            for &(sid, pts) in ce.seru_captures.iter().take(t_len as usize) {
                ext_block.extend_from_slice(&sid.to_le_bytes());
                ext_block.extend_from_slice(&pts.to_le_bytes());
            }
            for chain in ce.active_chains.iter() {
                ext_block.extend_from_slice(chain);
            }
        }
        let ch_count = self.ext_v2.saved_chains.len().min(255) as u8;
        ext_block.push(ch_count);
        for ch in self.ext_v2.saved_chains.iter().take(ch_count as usize) {
            ext_block.push(ch.char_slot);
            let nb = ch.name.as_bytes();
            let nlen = nb.len().min(63) as u8;
            ext_block.push(nlen);
            ext_block.extend_from_slice(&nb[..nlen as usize]);
            let slen = ch.sequence.len().min(63) as u8;
            ext_block.push(slen);
            ext_block.extend_from_slice(&ch.sequence[..slen as usize]);
        }
        ext_block
    }

    /// The `LGX4` block body (no header) - per-character shiny-Seru lists;
    /// only characters with at least one shiny spell are listed.
    pub(super) fn ext_v4_body(&self) -> Vec<u8> {
        let mut ext4_block = Vec::new();
        let shiny_chars: Vec<&(u8, CharSaveExt)> = self
            .ext_v2
            .per_char
            .iter()
            .filter(|(_, ce)| !ce.shiny_spells.is_empty())
            .collect();
        let shiny_count = shiny_chars.len().min(255) as u8;
        ext4_block.push(shiny_count);
        for (cs, ce) in shiny_chars.iter().take(shiny_count as usize) {
            ext4_block.push(*cs);
            let s_len = ce.shiny_spells.len().min(255) as u8;
            ext4_block.push(s_len);
            ext4_block.extend_from_slice(&ce.shiny_spells[..s_len as usize]);
        }
        ext4_block
    }

    /// Everything up to and including the `LGX4` block - the v4 file.
    fn write_v4_body(&self) -> Vec<u8> {
        let party_bytes = self.party.write();
        let inv = &self.ext.inventory;
        let mut out = Vec::with_capacity(15 + inv.len() * 2 + party_bytes.len() + 256);

        out.extend_from_slice(&SAVE_FILE_MAGIC);
        out.push(SAVE_FILE_VERSION);
        out.extend_from_slice(&self.ext.story_flags.to_le_bytes());
        out.extend_from_slice(&self.ext.money.to_le_bytes());
        // Clamp the count byte AND the emitted pairs identically: the count is
        // a single u8, so >255 entries would wrap (256 -> 0) while still writing
        // every pair, misaligning the whole downstream parse. Every other
        // length-prefixed field (party_count, spells, chains) clamps the same
        // way; the v1 inventory loop must too.
        let inv_count = inv.len().min(u8::MAX as usize);
        out.push(inv_count as u8);
        for &(id, count) in inv.iter().take(inv_count) {
            out.push(id);
            out.push(count);
        }
        let party_count = self.party.members.len().min(255) as u8;
        out.push(party_count);
        out.extend_from_slice(&party_bytes[..party_count as usize * CHARACTER_RECORD_SIZE]);

        // V2 extension block.
        let ext_block = self.ext_v2_body();
        out.extend_from_slice(&SAVE_FILE_EXT_MAGIC);
        let ext_total_size = ext_block.len() as u32;
        out.extend_from_slice(&ext_total_size.to_le_bytes());
        out.extend_from_slice(&ext_block);

        // V3 extension block: retail-faithful story-flag window.
        // Emit unconditionally so the magic byte is a stable parse marker -
        // an empty bitmap encodes as `len=0` and costs 10 bytes.
        let mut ext3_block = Vec::new();
        let bits_len = self.ext.story_flag_bits.len().min(u16::MAX as usize) as u16;
        ext3_block.extend_from_slice(&bits_len.to_le_bytes());
        ext3_block.extend_from_slice(&self.ext.story_flag_bits[..bits_len as usize]);
        out.extend_from_slice(&SAVE_FILE_EXT3_MAGIC);
        let ext3_total_size = ext3_block.len() as u32;
        out.extend_from_slice(&ext3_total_size.to_le_bytes());
        out.extend_from_slice(&ext3_block);

        // V4 extension block: per-character shiny-Seru spell lists. Emit
        // unconditionally so the LGX4 magic is a stable parse marker.
        let ext4_block = self.ext_v4_body();
        out.extend_from_slice(&SAVE_FILE_EXT4_MAGIC);
        let ext4_total_size = ext4_block.len() as u32;
        out.extend_from_slice(&ext4_total_size.to_le_bytes());
        out.extend_from_slice(&ext4_block);

        // Optional LGX6 block: the bag's physical slot array, `(id, count)`
        // in slot order. Emitted only when there is one, so a save without
        // slot-level data is byte-identical to a pre-block file.
        if !self.ext.item_slots.is_empty() {
            let mut ext6_block = Vec::with_capacity(self.ext.item_slots.len() * 2 + 2);
            let slot_count = self.ext.item_slots.len().min(u16::MAX as usize) as u16;
            ext6_block.extend_from_slice(&slot_count.to_le_bytes());
            for &(id, count) in self.ext.item_slots.iter().take(slot_count as usize) {
                ext6_block.push(id);
                ext6_block.push(count);
            }
            out.extend_from_slice(&SAVE_FILE_EXT6_MAGIC);
            out.extend_from_slice(&(ext6_block.len() as u32).to_le_bytes());
            out.extend_from_slice(&ext6_block);
        }

        // Optional LGX7 block: the minigame purses. Emitted only when one is
        // non-zero, so a save without minigame progress is byte-identical to
        // a pre-block file.
        if !self.ext.minigames.is_empty() {
            let body = self.ext.minigames.body();
            out.extend_from_slice(&SAVE_FILE_EXT7_MAGIC);
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&body);
        }

        // Optional LGX8 block: the field position the save was written at.
        // Emitted only when there is one, so a save without it is
        // byte-identical to a pre-block file.
        if let Some((x, z)) = self.ext_v2.field_position {
            out.extend_from_slice(&SAVE_FILE_EXT8_MAGIC);
            out.extend_from_slice(&4u32.to_le_bytes());
            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&z.to_le_bytes());
        }

        // Optional LGX9 block: the two audio-level words. Emitted only when
        // the save carries them.
        if let Some(levels) = self.ext_v2.audio_levels {
            out.extend_from_slice(&SAVE_FILE_EXT9_MAGIC);
            out.extend_from_slice(&8u32.to_le_bytes());
            out.extend_from_slice(&levels.configured_level.to_le_bytes());
            out.extend_from_slice(&levels.voice_volume.to_le_bytes());
        }

        // Optional LGXA block: the options screen's config words. Emitted
        // only when the save carries them.
        if let Some(options) = self.ext_v2.options {
            let body = options.to_bytes();
            out.extend_from_slice(&SAVE_FILE_EXTA_MAGIC);
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&body);
        }

        out
    }
}

/// The `LGX5` body: two length-prefixed ASCII strings, each clamped to its
/// retail field width so the same bytes fit the SC block's fields.
fn resume_block(resume: &SaveResume) -> Vec<u8> {
    let scene = ascii_clamped(&resume.scene, RESUME_SCENE_MAX_LEN);
    let location = ascii_clamped(&resume.location, RESUME_LOCATION_MAX_LEN);
    let mut out = Vec::with_capacity(2 + scene.len() + location.len());
    out.push(scene.len() as u8);
    out.extend_from_slice(&scene);
    out.push(location.len() as u8);
    out.extend_from_slice(&location);
    out
}

/// A string as printable-ASCII bytes, non-ASCII folded to `?`, cut to `max`.
fn ascii_clamped(s: &str, max: usize) -> Vec<u8> {
    s.chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                c as u8
            } else {
                b'?'
            }
        })
        .take(max)
        .collect()
}
