//! Engine-side global save state that doesn't fit in per-character records.
//!
//! The retail PSX save format encodes story flags, inventory, and other globals
//! in a block format whose exact layout hasn't been reverse-engineered from the
//! save-screen overlay yet. This module defines a self-describing binary format
//! (`LGSF v1`) for the engine's own slot files that bundles the party records
//! with the global state. Once the retail layout is traced it can be added as a
//! separate round-trip path alongside this one.
//!
//! ## Binary layout (`LGSF v1`)
//!
//! ```text
//! Offset  Bytes  Field
//! 0x00    4      Magic: b"LGSF"
//! 0x04    1      Version: 1
//! 0x05    4      story_flags (u32 LE)
//! 0x09    4      money       (i32 LE)
//! 0x0D    1      inventory_count (N)
//! 0x0E    2*N    inventory pairs: (item_id u8, count u8)
//! 0x0E+2N 1      party_count (M)
//! 0x0F+2N M*0x414  party records (M × CHARACTER_RECORD_SIZE bytes)
//! ```
//!
//! ## Binary layout (`LGSF v2` - extends v1)
//!
//! v2 is backward-compatible: the writer (when emitting v2 or higher) emits
//! a v1-shaped prelude and appends a per-character extension block plus a
//! game-level extension block after the v1 party records:
//!
//! ```text
//! ... v1 fields above ...
//!
//! After party records:
//! 4      ext_magic: b"LGX2"  (sentinel - old v1 readers stop here)
//! 4      ext_total_size (u32 LE) - bytes of the v2 extension block
//! 4      play_time_seconds (u32 LE) - total game time
//! 1      active_party_size (P)
//! P      active_party (P × char_slot bytes; 0..=2 main, 3+ guests)
//! 1      ext_char_count (X)
//! X*Y    per-character extension records (Y bytes each):
//!        - 4    learned_arts_mask (u32 LE)
//!        - 1    spell_count (S)
//!        - S    spell ids (u8 each)
//!        - 1    seru_capture_count (T)
//!        - T*4  per-seru: (seru_id u16 LE, points u16 LE)
//!        - 16   tactical_arts_chain bytes (4 chains × 4 directions max,
//!               packed Command bytes; 0 = empty)
//! 1      saved_chain_count (C)
//! C*M    per saved chain:
//!        - 1    char_slot
//!        - 1    name_len
//!        - N    name bytes (UTF-8)
//!        - 1    sequence_len
//!        - K    Command bytes
//! ```
//!
//! ## Binary layout (`LGSF v3` - extends v2)
//!
//! v3 appends a retail-faithful story-flag bitmap (mirroring the
//! [`crate::card::RETAIL_STORY_FLAGS_SIZE`]-byte region at retail SC offset
//! `0x14C0` / live RAM `0x80085600..0x80085958`; the `u16` length carries any
//! size)
//! after the v2 ext block. Writers always emit v3 once any byte of
//! [`SaveExt::story_flag_bits`] is non-empty; readers accept v1, v2, and v3.
//!
//! ```text
//! ... v2 fields above ...
//! 4      ext3_magic: b"LGX3" (sentinel - v2 readers stop here)
//! 4      ext3_total_size (u32 LE)
//! 2      story_flag_bits_len (u16 LE)
//! N      story_flag_bits (bytes, mirrors retail SC `+0x14C0`)
//! ```
//!
//! ## Binary layout (`LGSF v4` - extends v3)
//!
//! v4 appends a per-character "shiny Seru" list - spell ids learned from a
//! shiny-captured Seru (the rare 2%-per-battle variant; see the `shiny_seru`
//! randomizer feature + `legaia_engine_battle::seru_learning`). Such a spell deals
//! +35% damage for that character. The retail disc encodes this in the high
//! bit of the spell-level byte (`record+0x161`); the engine save carries it
//! out-of-band so the spell-id list stays clean.
//!
//! ```text
//! ... v3 fields above ...
//! 4      ext4_magic: b"LGX4" (sentinel - v3 readers stop here)
//! 4      ext4_total_size (u32 LE)
//! 1      shiny_char_count (X)
//! per char (only chars with at least one shiny spell):
//!   1    char_slot
//!   1    shiny_spell_count (S)
//!   S    shiny spell ids (u8 each)
//! ```
//!
//! ## Optional trailer: the resume point (`LGX5`)
//!
//! A save also names where it was written - the CDNAME label of the scene
//! (what retail's loader resumes into) and the scene's banner name (what the
//! save-select info panel prints as the location row). Neither is `World`
//! state, so they ride as a sibling [`SaveResume`] rather than a `SaveFile`
//! field, and the writer appends them **only when populated** so a file
//! without a resume point is byte-identical to a v4 file (the scenario suite
//! hashes those bytes). The version byte stays 4: a v4 reader stops at the
//! end of `LGX4` and never sees the trailer; [`SaveFile::parse_with_resume`]
//! looks past it for the magic.
//!
//! ```text
//! ... v4 fields above ...
//! 4      ext5_magic: b"LGX5" (absent when the resume point is empty)
//! 4      ext5_total_size (u32 LE)
//! 1      scene_len (N, <= 15)
//! N      scene CDNAME label (ASCII)
//! 1      location_len (M, <= 35)
//! M      location display name (ASCII)
//! ```
//!
//! ## The engine-ext blob inside a retail SC block (`LGXE`)
//!
//! Retail's composer copies `0x1A18` bytes of live game state to the front
//! of the `0x2000`-byte block and memsets the rest, and its loader copies the
//! same `0x1A18` bytes back - the bytes from
//! [`crate::card::RETAIL_LIVE_STATE_SIZE`] up to the checksum word at
//! [`crate::card::RETAIL_BLOCK_CHECKSUM_OFFSET`] are never read by the game.
//! That `0x5E4`-byte tail is where a card written by this engine keeps the
//! `LGX2` / `LGX4` payload retail has no slot for (the play clock, the party
//! composition, the chain library), magic-guarded so a real retail block -
//! all zeros there - reads as [`SaveExtV2::default`] exactly as before:
//!
//! ```text
//! +0x1A18  4      b"LGXE"
//! +0x1A1C  2      v2_len (u16 LE)   the LGX2 body, byte-identical to the file's
//! +0x1A1E  N      v2 body
//!          2      v4_len (u16 LE)   the LGX4 body
//!          M      v4 body
//! ```
//!
//! The block checksum covers the tail, so the writer restamps it like every
//! other `write_retail_*` helper. See `docs/subsystems/save-screen.md`.

use anyhow::{Context, Result, bail};

use crate::character::{CHARACTER_RECORD_SIZE, Party};

mod lgsf_parse;
use lgsf_parse::{apply_ext_v4, parse_ext_v2};
mod lgsf_write;
mod retail_block;
#[cfg(test)]
mod tests;

/// Four-byte magic at the start of every `LGSF` save file.
pub const SAVE_FILE_MAGIC: [u8; 4] = *b"LGSF";
/// Current format version stored in byte 4.
pub const SAVE_FILE_VERSION: u8 = 4;
/// V1 sentinel kept for legacy save reads.
pub const SAVE_FILE_VERSION_V1: u8 = 1;
/// V2 sentinel - first version with the LGX2 ext block but no LGX3 story-flag bitmap.
pub const SAVE_FILE_VERSION_V2: u8 = 2;
/// V3 sentinel - has the LGX3 story-flag bitmap but no LGX4 shiny block.
pub const SAVE_FILE_VERSION_V3: u8 = 3;
/// Magic at the start of the v2 extension block.
pub const SAVE_FILE_EXT_MAGIC: [u8; 4] = *b"LGX2";
/// Magic at the start of the v3 extension block (full story-flag bitmap).
pub const SAVE_FILE_EXT3_MAGIC: [u8; 4] = *b"LGX3";
/// Magic at the start of the v4 extension block (per-character shiny Seru list).
pub const SAVE_FILE_EXT4_MAGIC: [u8; 4] = *b"LGX4";
/// Magic of the optional resume-point trailer ([`SaveResume`]).
pub const SAVE_FILE_EXT5_MAGIC: [u8; 4] = *b"LGX5";
/// Magic of the optional **item-slot** block ([`SaveExt::item_slots`]).
///
/// Optional like the resume trailer rather than versioned like `LGX2`..`LGX4`:
/// a save whose bag carries no slot-level data emits nothing, so a file
/// written before the block existed and one written without it are the same
/// bytes.
pub const SAVE_FILE_EXT6_MAGIC: [u8; 4] = *b"LGX6";
/// Magic of the engine-ext blob a retail SC block carries in its unread tail
/// ([`SaveFile::write_engine_ext_into_retail_sc_block`]).
pub const RETAIL_ENGINE_EXT_MAGIC: [u8; 4] = *b"LGXE";

/// Optional minigame-purse block ([`crate::MinigameSave`]): the casino coin
/// bank, the Point Card bank and the fishing point record. Emitted only when
/// one of them is non-zero, like `LGX6`.
pub const SAVE_FILE_EXT7_MAGIC: [u8; 4] = *b"LGX7";
/// Optional field-position block ([`SaveExtV2::field_position`]): `i16 x`,
/// `i16 z`. Emitted only when the save carries a position, like `LGX6`.
pub const SAVE_FILE_EXT8_MAGIC: [u8; 4] = *b"LGX8";
/// Optional audio-level block ([`SaveExtV2::audio_levels`]): `i32
/// configured_level`, `i32 voice_volume`. Emitted only when the save carries
/// the pair, like `LGX8`.
pub const SAVE_FILE_EXT9_MAGIC: [u8; 4] = *b"LGX9";
/// Optional options block ([`SaveExtV2::options`]): the nine option-screen
/// config words, one byte each in [`crate::card::RetailOptions::to_bytes`]
/// order. Emitted only when the save carries them, like `LGX9`.
pub const SAVE_FILE_EXTA_MAGIC: [u8; 4] = *b"LGXA";
/// Byte offset of the engine-ext blob inside a retail SC block: the first
/// byte past the `0x1A18`-byte live-state copy retail composes and loads.
pub const RETAIL_ENGINE_EXT_OFFSET: usize = crate::card::RETAIL_LIVE_STATE_SIZE;
/// Bytes available to the engine-ext blob: up to the block checksum word.
pub const RETAIL_ENGINE_EXT_CAPACITY: usize =
    crate::card::RETAIL_BLOCK_CHECKSUM_OFFSET - RETAIL_ENGINE_EXT_OFFSET;
/// End of the engine-ext region = the block checksum word's offset.
const RETAIL_BLOCK_CHECKSUM_END: usize = crate::card::RETAIL_BLOCK_CHECKSUM_OFFSET;
/// Longest CDNAME label the resume trailer stores - one byte short of the
/// retail `0x10`-byte NUL-terminated scene-label field.
pub const RESUME_SCENE_MAX_LEN: usize = crate::card::RETAIL_SCENE_LABEL_LEN - 1;
/// Longest location name the resume trailer stores - one byte short of the
/// retail `0x24`-byte field.
pub const RESUME_LOCATION_MAX_LEN: usize = crate::card::RETAIL_LOCATION_NAME_LEN - 1;

/// Engine-wide global state that is not part of any per-character record.
///
/// Retail SC blocks store these alongside the character records at fixed
/// offsets:
/// - `story_flag_bits` mirrors the story-flag window at `0x14C0` (RAM
///   `0x80085600..0x80085958`, the system-flag bank at its `+0x158`).
/// - `inventory` mirrors the 144-byte 72-slot pair array at `0x1818`
///   (RAM `0x80085958..0x800859E8`).
///
/// [`legaia_engine_system::story_flag_state::StoryFlagState::story_flags`] is a separate 32-bit flag word at
/// `_DAT_1F800394` (PSX scratchpad) that the field VM operates on via
/// opcodes `0x2E` / `0x2F` / `0x30`. It is **not** a cache of the wide
/// retail bitmap: retail seeds it from the game-mode descriptor table
/// (`mode_table[mode_idx].param`, stride `0x18`, byte offset `0x14`) on
/// mode init in `FUN_8001DCF8`, and the SC save/load path never copies
/// between the bitmap and scratchpad. The two fields therefore
/// round-trip independently.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveExt {
    /// Field-VM scratchpad flag word (`_DAT_1F800394` in retail). Read
    /// by op `0x30` (test bit); set by op `0x2E`; cleared by op `0x2F`.
    /// Seeded on mode init from `mode_table[mode_idx].param` low 16
    /// bits; not synced to or from [`legaia_engine_system::story_flag_state::StoryFlagState::story_flag_bits`].
    pub story_flags: u32,
    /// Full story-flag window from retail SC offset `0x14C0`, mirroring live
    /// RAM `0x80085600..0x80085958` ([`crate::card::RETAIL_STORY_FLAGS_SIZE`]
    /// bytes; the system-flag bank is its `+0x158..`). Empty (`vec![]`) means
    /// "no retail-shaped data captured"; engines that don't track the wide
    /// bitmap can leave this default. Reads/writes of the bitmap go through
    /// [`crate::card::read_retail_story_flags`] /
    /// [`crate::card::write_retail_story_flags`].
    pub story_flag_bits: Vec<u8>,
    /// Running gold total mirroring `World::party.money`. Field-VM op `0x3A`
    /// mutates this; clamped to `[0, 9_999_999]` at runtime.
    pub money: i32,
    /// Per-item-ID inventory counts. Pairs are sorted by `item_id`.
    ///
    /// This is the **compact** view - one entry per held id, no slot
    /// coordinate - and it is what the v1 prelude carries, so a file written
    /// by any version reads back here. [`Self::item_slots`] is the physical
    /// array beside it.
    pub inventory: Vec<(u8, u8)>,
    /// The bag's **physical slot array** (SC `+0x1818`): 256 `(id, count)`
    /// pairs in slot order, holes included.
    ///
    /// Empty means "this file carries no slot-level data" - a pre-`LGX6`
    /// engine save, or an importer that only had the compact list - and a
    /// consumer then seeds slots densely from [`Self::inventory`], which is
    /// what every save did before the block existed. Slot order matters
    /// because one retail consumer indexes the bag by slot (PROT 0941's
    /// Steal); every other one addresses it by id and cannot tell.
    pub item_slots: Vec<(u8, u8)>,
    /// The minigame purses and records (casino coins, Point Card, fishing
    /// point record). Retail keeps all nine words in its live-state window,
    /// so a retail block carries them at their own offsets
    /// ([`crate::minigame_save`]); an `LGSF` file carries them in the optional
    /// `LGX7` block.
    pub minigames: crate::MinigameSave,
}

/// Per-character v2 extension data. Engines populate this from
/// in-memory state at save time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CharSaveExt {
    /// Bitmask of learned Tactical Arts (1 << art_id). The retail engine
    /// stores this in the character record at `+0x13C`; we mirror it here
    /// as a u32 word for convenience.
    pub learned_arts_mask: u32,
    /// Per-character learned spell list (spell ids).
    pub spells: Vec<u8>,
    /// Per-Seru capture totals: `(seru_id, points)` pairs.
    pub seru_captures: Vec<(u16, u16)>,
    /// Up to four "active" tactical-arts chains the player has bound to
    /// quick-call slots. Each chain is up to four packed `Command` bytes
    /// (`0` terminator). Mirrors the retail in-RAM chain table.
    pub active_chains: [[u8; 4]; 4],
    /// Spell ids learned from a **shiny** Seru (the rare 2%-per-battle
    /// variant). Each deals +35% damage for this character. Persisted in the
    /// LGSF v4 `LGX4` block, out-of-band from [`Self::spells`] so the spell-id
    /// list stays clean. Mirrors the retail `+0x161` spell-level high bit.
    pub shiny_spells: Vec<u8>,
}

/// One named saved chain in the v2 ext block.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedChainRecord {
    /// Character index this chain belongs to.
    pub char_slot: u8,
    /// Display name.
    pub name: String,
    /// Packed Command bytes (`0` = empty / terminator).
    pub sequence: Vec<u8>,
}

/// V2-only top-level extension block (engines fill at save time).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveExtV2 {
    /// Total game time in seconds (engine-side counter).
    pub play_time_seconds: u32,
    /// `char_slot` indices of the active battle party. First three are
    /// main characters; later entries are story guests.
    pub active_party: Vec<u8>,
    /// Per-character extension records keyed by `char_slot`.
    pub per_char: Vec<(u8, CharSaveExt)>,
    /// Cross-character saved chain library.
    pub saved_chains: Vec<SavedChainRecord>,
    /// Where the player stood in the field when the save was written - the
    /// `(x, z)` a resume seats the party at. Retail's carrier is the SC
    /// block's position snapshot ([`crate::card::RETAIL_FIELD_POS_X_OFFSET`]);
    /// an `LGSF` file carries it in the optional `LGX8` block. `None` = the
    /// save names no position, and a resume enters at the scene's own seat.
    pub field_position: Option<(i16, i16)>,
    /// The two audio-level words retail keeps in its live-state window - the
    /// configured level `0x8008457C` and the voice / SFX volume `0x80084580`
    /// ([`crate::card::RetailAudioLevels`]). A retail block carries them at
    /// their own offsets; an `LGSF` file carries them in the optional `LGX9`
    /// block. `None` = the save names none, and a load keeps the cold-reset
    /// pair.
    pub audio_levels: Option<crate::card::RetailAudioLevels>,
    /// The options screen's config words retail keeps in its live-state
    /// window - Battle Camera, Select Attack, Battle Command, Field Move,
    /// Field HP Display, Sound and the three vibration rows
    /// ([`crate::card::RetailOptions`]). A retail block carries them at their
    /// own offsets, so a card load restores them; an `LGSF` file carries them
    /// in the optional `LGXA` block. `None` = the save names none, and a load
    /// keeps the session's options.
    pub options: Option<crate::card::RetailOptions>,
}

/// Where a save was written: the scene to resume into and the name the
/// save-select info panel prints for it.
///
/// Retail keeps both in the live-state window the SC block mirrors - the
/// CDNAME label at [`crate::card::RETAIL_SCENE_LABEL_OFFSET`] (the scene its
/// loader re-enters) and the scene MAN's section-2 banner name at
/// [`crate::card::RETAIL_LOCATION_NAME_OFFSET`] (the info panel's location
/// row). Neither lives on the engine's `World`, so this rides beside
/// [`SaveFile`] rather than inside it: the LGSF writer appends it as the
/// optional `LGX5` trailer ([`SaveFile::write_with_resume`]) and the
/// retail-block bridge writes it into those two fields
/// ([`SaveResume::write_into_retail_sc_block`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveResume {
    /// CDNAME scene label (`town01`, ...). Empty = unknown.
    pub scene: String,
    /// Location display name (`Rim Elm`, ...). Empty = unknown.
    pub location: String,
}

impl SaveResume {
    /// `true` when neither field is set - the trailer is then not written.
    pub fn is_empty(&self) -> bool {
        self.scene.is_empty() && self.location.is_empty()
    }

    /// Read the resume point off a retail SC block: the CDNAME label at
    /// `+0x408` and the location name at `+0x200`, each NUL-terminated.
    /// Non-printable bytes end the string, so a never-written (free) block
    /// reads as empty rather than as garbage.
    pub fn from_retail_sc_block(sc_block: &[u8]) -> Self {
        Self {
            scene: crate::card::read_retail_scene_label(sc_block).unwrap_or_default(),
            location: crate::card::read_retail_location_name(sc_block).unwrap_or_default(),
        }
    }

    /// Write both fields into a retail SC block in place (NUL-padded to the
    /// retail field widths, restamping the block checksum). Retail's own
    /// composer copies the live scene label + banner name into exactly these
    /// bytes, so a block written here is one retail resumes correctly.
    pub fn write_into_retail_sc_block(&self, sc_block: &mut [u8]) -> Result<()> {
        crate::card::write_retail_resume(sc_block, &self.scene, &self.location)
    }
}

/// What the save-select info panel shows for a save's lead character - one
/// derivation shared by every host so the native window and the browser
/// page print the same slot the same way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LeaderSummary {
    /// Display name off the record's `+0x2A7` field.
    pub name: String,
    /// Level as the panel prints it - see [`displayed_level`].
    pub level: u8,
    /// `(current, max)` HP.
    pub hp: (u16, u16),
    /// `(current, max)` MP.
    pub mp: (u16, u16),
    /// Roster index of the lead (`0` = Vahn on every retail save: the lead
    /// is record 0 of the SC character-record array).
    pub char_id: u8,
}

/// The level the save-select panel prints for a record: retail's displayed
/// level byte (`+0x130`, [`crate::CharacterRecord::magic_rank`]) when it is
/// in `1..=99`, else the level the base XP curve implies for the record's
/// cumulative XP ([`crate::level_for_cumulative_xp`]). The fallback covers
/// records retail never displayed - a zeroed synthetic record, or one a tool
/// seeded by XP alone - so a save never prints as level 0.
pub fn displayed_level(record: &crate::CharacterRecord) -> u8 {
    match record.magic_rank() {
        l @ 1..=99 => l,
        _ => crate::level_for_cumulative_xp(record.cumulative_xp()),
    }
}

/// A complete engine save file: party records plus global state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveFile {
    /// Per-character roster (N records of [`CHARACTER_RECORD_SIZE`] bytes).
    pub party: Party,
    /// Global engine state bundled with the party.
    pub ext: SaveExt,
    /// V2 extension block. Empty / default when reading v1 saves.
    pub ext_v2: SaveExtV2,
}

impl Default for SaveFile {
    fn default() -> Self {
        Self {
            party: Party { members: vec![] },
            ext: SaveExt::default(),
            ext_v2: SaveExtV2::default(),
        }
    }
}

impl SaveFile {
    /// The lead character's info-panel summary, or `None` for a save with no
    /// party records. See [`LeaderSummary`] / [`displayed_level`].
    pub fn leader_summary(&self) -> Option<LeaderSummary> {
        let leader = self.party.members.first()?;
        let v = leader.hp_mp_sp();
        Some(LeaderSummary {
            name: leader.name(),
            level: displayed_level(leader),
            hp: (v.hp_cur, v.hp_max),
            mp: (v.mp_cur, v.mp_max),
            char_id: 0,
        })
    }
}
