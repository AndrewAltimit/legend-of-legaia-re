//! `LGSF` parsing: the versioned prelude walk and each `LGX*` block reader.

use super::*;

impl SaveFile {
    /// Parse `LGSF` bytes (v1 through v4), or fall back to the old
    /// party-only format for save files written before this module existed.
    /// A resume trailer, if present, is skipped - see
    /// [`Self::parse_with_resume`].
    pub fn parse(buf: &[u8]) -> Result<Self> {
        Self::parse_with_resume(buf).map(|(sf, _)| sf)
    }

    /// [`Self::parse`] that also returns the `LGX5` resume trailer - an empty
    /// [`SaveResume`] for a file written without one (every pre-trailer
    /// file, and every file whose resume point was empty).
    pub fn parse_with_resume(buf: &[u8]) -> Result<(Self, SaveResume)> {
        if buf.starts_with(&SAVE_FILE_MAGIC) {
            Self::parse_versioned(buf)
        } else {
            // Old format: raw party bytes, no ext data.
            let party = Party::parse(buf).context("parse legacy party-only save")?;
            Ok((
                Self {
                    party,
                    ext: SaveExt::default(),
                    ext_v2: SaveExtV2::default(),
                },
                SaveResume::default(),
            ))
        }
    }

    fn parse_versioned(buf: &[u8]) -> Result<(Self, SaveResume)> {
        if buf.len() < 15 {
            bail!("LGSF: buffer too short ({} bytes)", buf.len());
        }
        let version = buf[4];
        match version {
            SAVE_FILE_VERSION_V1 | SAVE_FILE_VERSION_V2 | SAVE_FILE_VERSION_V3
            | SAVE_FILE_VERSION => {}
            other => bail!("LGSF: unsupported version {other}"),
        }
        let story_flags = u32::from_le_bytes(buf[5..9].try_into().unwrap());
        let money = i32::from_le_bytes(buf[9..13].try_into().unwrap());
        let inv_count = buf[13] as usize;
        let inv_end = 14 + inv_count * 2;
        if buf.len() < inv_end + 1 {
            bail!("LGSF: truncated inventory (need {} bytes)", inv_end + 1);
        }
        let mut inventory = Vec::with_capacity(inv_count);
        for i in 0..inv_count {
            let off = 14 + i * 2;
            inventory.push((buf[off], buf[off + 1]));
        }
        let party_count = buf[inv_end] as usize;
        let party_start = inv_end + 1;
        let party_end = party_start + party_count * CHARACTER_RECORD_SIZE;
        if buf.len() < party_end {
            bail!(
                "LGSF: truncated party ({party_count} records need {}, got {})",
                party_count * CHARACTER_RECORD_SIZE,
                buf.len().saturating_sub(party_start)
            );
        }
        let party = if party_count == 0 {
            Party { members: vec![] }
        } else {
            Party::parse(&buf[party_start..party_end]).context("LGSF: parse party records")?
        };

        let mut ext = SaveExt {
            story_flags,
            money,
            inventory,
            story_flag_bits: Vec::new(),
            ..Default::default()
        };

        // V1 reads stop at party_end. V2/V3 may have one or two extension blocks.
        if version == SAVE_FILE_VERSION_V1 {
            return Ok((
                Self {
                    party,
                    ext,
                    ext_v2: SaveExtV2::default(),
                },
                SaveResume::default(),
            ));
        }
        let mut cursor = party_end;
        if cursor + 8 > buf.len() {
            // V2/V3 declared but no ext block - treat as empty.
            return Ok((
                Self {
                    party,
                    ext,
                    ext_v2: SaveExtV2::default(),
                },
                SaveResume::default(),
            ));
        }
        let magic = &buf[cursor..cursor + 4];
        if magic != SAVE_FILE_EXT_MAGIC {
            bail!("LGSF v2: missing ext magic at {cursor:#x}");
        }
        cursor += 4;
        let ext_total_size =
            u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        // `ext_total_size` is a raw u32 from the file; on a 32-bit target
        // (wasm) `cursor + ext_total_size` could wrap and slip past the
        // length check into a reversed slice range. Add checked.
        let ext_end = cursor
            .checked_add(ext_total_size)
            .filter(|&e| e <= buf.len())
            .ok_or_else(|| anyhow::anyhow!("LGSF v2: ext block truncated"))?;
        let ext_buf = &buf[cursor..ext_end];
        let mut ext_v2 = parse_ext_v2(ext_buf).context("parse LGSF v2 ext block")?;
        cursor = ext_end;

        // V3 LGX3 ext block carries the retail story-flag window.
        if version >= SAVE_FILE_VERSION_V3 {
            if cursor + 8 > buf.len() {
                bail!("LGSF v3: LGX3 ext header missing (cursor {cursor:#x})");
            }
            let magic3 = &buf[cursor..cursor + 4];
            if magic3 != SAVE_FILE_EXT3_MAGIC {
                bail!("LGSF v3: missing LGX3 magic at {cursor:#x}");
            }
            cursor += 4;
            let ext3_total_size =
                u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            let ext3_end = cursor
                .checked_add(ext3_total_size)
                .filter(|&e| e <= buf.len())
                .ok_or_else(|| anyhow::anyhow!("LGSF v3: LGX3 ext block truncated"))?;
            ext.story_flag_bits =
                parse_ext_v3(&buf[cursor..ext3_end]).context("parse LGSF v3 ext block")?;
            cursor = ext3_end;
        }

        // V4 LGX4 ext block carries the per-character shiny-Seru spell lists.
        if version >= SAVE_FILE_VERSION {
            if cursor + 8 > buf.len() {
                bail!("LGSF v4: LGX4 ext header missing (cursor {cursor:#x})");
            }
            let magic4 = &buf[cursor..cursor + 4];
            if magic4 != SAVE_FILE_EXT4_MAGIC {
                bail!("LGSF v4: missing LGX4 magic at {cursor:#x}");
            }
            cursor += 4;
            let ext4_total_size =
                u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            let ext4_end = cursor
                .checked_add(ext4_total_size)
                .filter(|&e| e <= buf.len())
                .ok_or_else(|| anyhow::anyhow!("LGSF v4: LGX4 ext block truncated"))?;
            apply_ext_v4(&buf[cursor..ext4_end], &mut ext_v2).context("parse LGSF v4 ext block")?;
            cursor = ext4_end;
        }

        // Optional LGX6 item-slot block: the bag's physical slot array.
        if cursor + 8 <= buf.len() && buf[cursor..cursor + 4] == SAVE_FILE_EXT6_MAGIC {
            cursor += 4;
            let ext6_total_size =
                u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            let ext6_end = cursor
                .checked_add(ext6_total_size)
                .filter(|&e| e <= buf.len())
                .ok_or_else(|| anyhow::anyhow!("LGSF: LGX6 item-slot block truncated"))?;
            ext.item_slots =
                parse_ext_v6(&buf[cursor..ext6_end]).context("parse LGSF LGX6 item-slot block")?;
            cursor = ext6_end;
        }

        // Optional LGX7 minigame-purse block.
        if cursor + 8 <= buf.len() && buf[cursor..cursor + 4] == SAVE_FILE_EXT7_MAGIC {
            cursor += 4;
            let ext7_total_size =
                u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            let ext7_end = cursor
                .checked_add(ext7_total_size)
                .filter(|&e| e <= buf.len())
                .ok_or_else(|| anyhow::anyhow!("LGSF: LGX7 minigame block truncated"))?;
            ext.minigames = crate::MinigameSave::parse_body(&buf[cursor..ext7_end])
                .context("parse LGSF LGX7 minigame block")?;
            cursor = ext7_end;
        }

        // Optional LGX8 field-position block.
        if cursor + 8 <= buf.len() && buf[cursor..cursor + 4] == SAVE_FILE_EXT8_MAGIC {
            cursor += 4;
            let ext8_total_size =
                u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            let ext8_end = cursor
                .checked_add(ext8_total_size)
                .filter(|&e| e <= buf.len() && ext8_total_size >= 4)
                .ok_or_else(|| anyhow::anyhow!("LGSF: LGX8 field-position block truncated"))?;
            let b = &buf[cursor..ext8_end];
            ext_v2.field_position = Some((
                i16::from_le_bytes([b[0], b[1]]),
                i16::from_le_bytes([b[2], b[3]]),
            ));
            cursor = ext8_end;
        }

        // Optional LGX9 audio-level block.
        if cursor + 8 <= buf.len() && buf[cursor..cursor + 4] == SAVE_FILE_EXT9_MAGIC {
            cursor += 4;
            let ext9_total_size =
                u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            let ext9_end = cursor
                .checked_add(ext9_total_size)
                .filter(|&e| e <= buf.len() && ext9_total_size >= 8)
                .ok_or_else(|| anyhow::anyhow!("LGSF: LGX9 audio-level block truncated"))?;
            let b = &buf[cursor..ext9_end];
            ext_v2.audio_levels = Some(crate::card::RetailAudioLevels {
                configured_level: i32::from_le_bytes(b[0..4].try_into().unwrap()),
                voice_volume: i32::from_le_bytes(b[4..8].try_into().unwrap()),
            });
            cursor = ext9_end;
        }

        // Optional LGXA options block. A body whose values are past their
        // rows' choices is not this block's; it reads as absent.
        if cursor + 8 <= buf.len() && buf[cursor..cursor + 4] == SAVE_FILE_EXTA_MAGIC {
            cursor += 4;
            let exta_total_size =
                u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            let exta_end = cursor
                .checked_add(exta_total_size)
                .filter(|&e| e <= buf.len() && exta_total_size >= 9)
                .ok_or_else(|| anyhow::anyhow!("LGSF: LGXA options block truncated"))?;
            let b: [u8; 9] = buf[cursor..cursor + 9].try_into().unwrap();
            ext_v2.options = crate::card::RetailOptions::from_bytes(b);
            cursor = exta_end;
        }

        // Optional LGX5 resume trailer: present only when the writer had a
        // scene / location to record. Absent = empty, never an error, so a
        // pre-trailer v4 file and a trailer-less v4 file read the same.
        let mut resume = SaveResume::default();
        if cursor + 8 <= buf.len() && buf[cursor..cursor + 4] == SAVE_FILE_EXT5_MAGIC {
            cursor += 4;
            let ext5_total_size =
                u32::from_le_bytes(buf[cursor..cursor + 4].try_into().unwrap()) as usize;
            cursor += 4;
            let ext5_end = cursor
                .checked_add(ext5_total_size)
                .filter(|&e| e <= buf.len())
                .ok_or_else(|| anyhow::anyhow!("LGSF: LGX5 resume trailer truncated"))?;
            resume = parse_resume_block(&buf[cursor..ext5_end])
                .context("parse LGSF LGX5 resume trailer")?;
        }

        Ok((Self { party, ext, ext_v2 }, resume))
    }
}

fn parse_resume_block(buf: &[u8]) -> Result<SaveResume> {
    let mut p = 0usize;
    let scene_len = *buf
        .get(p)
        .ok_or_else(|| anyhow::anyhow!("LGX5: missing scene len"))? as usize;
    p += 1;
    let scene = buf
        .get(p..p + scene_len)
        .ok_or_else(|| anyhow::anyhow!("LGX5: scene truncated"))?;
    p += scene_len;
    let loc_len = *buf
        .get(p)
        .ok_or_else(|| anyhow::anyhow!("LGX5: missing location len"))? as usize;
    p += 1;
    let location = buf
        .get(p..p + loc_len)
        .ok_or_else(|| anyhow::anyhow!("LGX5: location truncated"))?;
    Ok(SaveResume {
        scene: scene.iter().map(|&b| b as char).collect(),
        location: location.iter().map(|&b| b as char).collect(),
    })
}

/// Parse the LGX4 shiny block and fold each char's shiny spell list into the
/// matching [`SaveExtV2::per_char`] entry. Characters not present in `per_char`
/// (which always comes from the LGX2 block) are skipped. Empty blocks are a
/// no-op.
pub(super) fn apply_ext_v4(buf: &[u8], ext_v2: &mut SaveExtV2) -> Result<()> {
    if buf.is_empty() {
        return Ok(());
    }
    let mut p = 0usize;
    let char_count = buf[p] as usize;
    p += 1;
    for _ in 0..char_count {
        if p + 2 > buf.len() {
            bail!("LGX4: char prelude truncated");
        }
        let cs = buf[p];
        p += 1;
        let s_len = buf[p] as usize;
        p += 1;
        if p + s_len > buf.len() {
            bail!("LGX4: shiny spell list truncated");
        }
        let shiny = buf[p..p + s_len].to_vec();
        p += s_len;
        if let Some((_, ce)) = ext_v2.per_char.iter_mut().find(|(slot, _)| *slot == cs) {
            ce.shiny_spells = shiny;
        }
    }
    Ok(())
}

/// Decode the `LGX6` item-slot block: `u16 slot_count` then that many
/// `(id, count)` pairs, in slot order with holes intact.
fn parse_ext_v6(buf: &[u8]) -> Result<Vec<(u8, u8)>> {
    if buf.len() < 2 {
        bail!("LGX6: block too short ({} bytes)", buf.len());
    }
    let count = u16::from_le_bytes(buf[0..2].try_into().unwrap()) as usize;
    let end = 2 + count * 2;
    if buf.len() < end {
        bail!(
            "LGX6: truncated slot array (need {end} bytes, got {})",
            buf.len()
        );
    }
    Ok(buf[2..end]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| (c[0], c[1]))
        .collect())
}

fn parse_ext_v3(buf: &[u8]) -> Result<Vec<u8>> {
    if buf.len() < 2 {
        bail!("LGX3 ext block too short");
    }
    let bits_len = u16::from_le_bytes(buf[..2].try_into().unwrap()) as usize;
    if buf.len() < 2 + bits_len {
        bail!("LGX3 story_flag_bits truncated");
    }
    Ok(buf[2..2 + bits_len].to_vec())
}

pub(super) fn parse_ext_v2(buf: &[u8]) -> Result<SaveExtV2> {
    if buf.len() < 4 {
        bail!("ext block too short");
    }
    let mut p = 0usize;
    let play_time_seconds = u32::from_le_bytes(buf[p..p + 4].try_into().unwrap());
    p += 4;
    if p >= buf.len() {
        bail!("ext: missing active party len");
    }
    let active_len = buf[p] as usize;
    p += 1;
    if p + active_len > buf.len() {
        bail!("ext: truncated active party");
    }
    let active_party = buf[p..p + active_len].to_vec();
    p += active_len;
    if p >= buf.len() {
        bail!("ext: missing per-char count");
    }
    let pc_count = buf[p] as usize;
    p += 1;
    let mut per_char = Vec::with_capacity(pc_count);
    for _ in 0..pc_count {
        if p + 6 > buf.len() {
            bail!("ext: per-char prelude truncated");
        }
        let cs = buf[p];
        p += 1;
        let learned_arts_mask = u32::from_le_bytes(buf[p..p + 4].try_into().unwrap());
        p += 4;
        let s_len = buf[p] as usize;
        p += 1;
        if p + s_len > buf.len() {
            bail!("ext: spell list truncated");
        }
        let spells = buf[p..p + s_len].to_vec();
        p += s_len;
        if p >= buf.len() {
            bail!("ext: missing seru count");
        }
        let t_len = buf[p] as usize;
        p += 1;
        if p + t_len * 4 > buf.len() {
            bail!("ext: seru list truncated");
        }
        let mut seru_captures = Vec::with_capacity(t_len);
        for _ in 0..t_len {
            let sid = u16::from_le_bytes(buf[p..p + 2].try_into().unwrap());
            let pts = u16::from_le_bytes(buf[p + 2..p + 4].try_into().unwrap());
            seru_captures.push((sid, pts));
            p += 4;
        }
        if p + 16 > buf.len() {
            bail!("ext: chain table truncated");
        }
        let mut active_chains = [[0u8; 4]; 4];
        for chain in active_chains.iter_mut() {
            chain.copy_from_slice(&buf[p..p + 4]);
            p += 4;
        }
        per_char.push((
            cs,
            CharSaveExt {
                learned_arts_mask,
                spells,
                seru_captures,
                active_chains,
                // LGX2 carries no shiny data; the LGX4 block (parsed later)
                // folds it into the matching entry by char_slot.
                shiny_spells: Vec::new(),
            },
        ));
    }
    let ch_count = if p < buf.len() { buf[p] as usize } else { 0 };
    if p < buf.len() {
        p += 1;
    }
    let mut saved_chains = Vec::with_capacity(ch_count);
    for _ in 0..ch_count {
        // Need two bytes here: char_slot followed by the name-length byte.
        // A prior version only checked for one byte before reading both,
        // so a saved-chain record whose `char_slot` was the final buffer
        // byte panicked on the `nlen = buf[p]` read. Check both up front.
        if p + 2 > buf.len() {
            bail!("saved_chain: char_slot / name_len missing");
        }
        let cs = buf[p];
        p += 1;
        let nlen = buf[p] as usize;
        p += 1;
        if p + nlen > buf.len() {
            bail!("saved_chain: name truncated");
        }
        let name = std::str::from_utf8(&buf[p..p + nlen])
            .context("saved_chain: name not UTF-8")?
            .to_string();
        p += nlen;
        if p >= buf.len() {
            bail!("saved_chain: seq_len missing");
        }
        let slen = buf[p] as usize;
        p += 1;
        if p + slen > buf.len() {
            bail!("saved_chain: sequence truncated");
        }
        let sequence = buf[p..p + slen].to_vec();
        p += slen;
        saved_chains.push(SavedChainRecord {
            char_slot: cs,
            name,
            sequence,
        });
    }
    Ok(SaveExtV2 {
        play_time_seconds,
        field_position: None,
        audio_levels: None,
        options: None,
        active_party,
        per_char,
        saved_chains,
    })
}
