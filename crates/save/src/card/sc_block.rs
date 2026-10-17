//! The game-data layout inside one retail SC block: the offsets of the
//! character records, party roster, story flags, inventory, gold, coins,
//! play counter, audio levels and resume labels, with a reader and a
//! restamping writer for each.

use super::*;
use anyhow::{Result, bail};

/// Byte offset from the start of an SC save block to where game data begins.
///
/// Verified by locating the "Vahn", "Noa", "Gala" names in an actual Legaia
/// retail mednafen `.mcr` save at `~/.mednafen/sav/Legend of Legaia (USA).*.0.mcr`.
/// Block layout: SC magic at +0, icon palette at +0x60, icon pixels at +0x80;
/// game-data region begins at +0x200.
pub const RETAIL_GAME_DATA_OFFSET: usize = 0x200;

/// Byte offset from the game data start to the first character record's base.
///
/// The SC block is a verbatim dump of the resident save-state region (the
/// linear map `block_offset = RETAIL_GAME_DATA_OFFSET + (ram_addr -
/// SAVE_GAME_DATA_RAM_BASE)`, so `game_data` mirrors live RAM from
/// `0x80084340`). Character record `n` lives at live RAM `0x80084708 +
/// n*0x414`, i.e. `game_data + 0x3C8` - confirmed across six in-game RAM
/// captures (mid-game stats at `record+0x104`/`+0x11C` read back the
/// expected per-character HP/MP for all four roster slots).
///
/// The display / global header therefore occupies `game_data 0x000..0x3C7`:
/// location name at `+0x000`, primary character display name at `+0x054`,
/// most-recently-visited CDNAME label at `+0x208`, previous scene CDNAME at
/// `+0x218`, party gold at `+0x25C`.
///
/// NB: a record's **display name** is at internal offset `+0x2A7`
/// ([`crate::character::NAME_OFFSET`]), so the visible "Vahn"/"Noa"/"Gala"/
/// "Terra" strings land at `game_data + 0x66F + n*0x414` (SC `+0x86F` for
/// slot 0). Anchoring the record region at the *name* (`0x66F`) rather than
/// the true base (`0x3C8`) was an earlier off-by-`0x2A7` that made
/// [`crate::character::CharacterRecord`]'s stat offsets read into the wrong
/// fields on a populated save.
pub const RETAIL_CHAR_RECORD_HEADER_SIZE: usize = 0x3C8;

/// Stride between character records in the retail save format (matches
/// `CHARACTER_RECORD_SIZE` = 0x414 used in `crates/save/src/character.rs`).
///
/// Confirmed by the four roster slots at live RAM `0x80084708 + n*0x414`
/// (Vahn / Noa / Gala / Terra) - the names land at `+0x2A7` within each,
/// i.e. `game+0x66F / 0xA83 / 0xE97 / 0x12AB`.
pub const RETAIL_CHAR_RECORD_STRIDE: usize = 0x414;

/// Byte offset (within the SC block) of the roster display-name list the
/// save-select info panel reads: NUL-padded ASCII slots of
/// [`RETAIL_PARTY_NAME_STRIDE`] bytes each, one per active party member
/// (`game_data + 0x054 + k*0xC`; the "primary character display name" the
/// [`RETAIL_CHAR_RECORD_HEADER_SIZE`] doc lists is slot 0 of this table).
/// A byte scan across a real 9-save card puts every `Vahn`/`Noa`/`Gala`
/// occurrence in an SC block at exactly two sites: this table and each
/// character record's own name at [`crate::character::NAME_OFFSET`] - an
/// early solo-party save populates only slot 0 here, which is what pins
/// the table as the *party roster* rather than a fixed 3-name array.
pub const RETAIL_PARTY_NAME_TABLE_OFFSET: usize = 0x254;
/// Stride between roster display-name slots ([`RETAIL_PARTY_NAME_TABLE_OFFSET`]).
pub const RETAIL_PARTY_NAME_STRIDE: usize = 0xC;
/// Roster display-name slots (battle party maximum).
pub const RETAIL_PARTY_NAME_SLOTS: usize = 3;

/// Rename a character in a retail SC save block: every name field whose
/// current NUL-terminated value equals `from` - across the roster name
/// table and all four character records' display names - is rewritten to
/// `to` (NUL-padded), and the block's additive checksum is restamped when
/// anything changed. Returns the number of fields rewritten (0 when the
/// name does not appear, e.g. a save from before that character joined).
///
/// `Err` if `to` does not fit the narrower site (the record name field is
/// [`crate::character::NAME_LEN`] bytes including its NUL terminator) or
/// is not plain ASCII, or if the block is shorter than [`BLOCK_SIZE`].
pub fn rename_retail_character(sc_block: &mut [u8], from: &str, to: &str) -> Result<usize> {
    use crate::character::{NAME_LEN, NAME_OFFSET};
    if sc_block.len() < BLOCK_SIZE {
        bail!("SC block too small: {} bytes", sc_block.len());
    }
    if to.len() >= NAME_LEN || !to.bytes().all(|b| b.is_ascii() && b != 0) {
        bail!(
            "replacement name {to:?} must be ASCII and at most {} bytes",
            NAME_LEN - 1
        );
    }
    let mut sites: Vec<(usize, usize)> = (0..RETAIL_PARTY_NAME_SLOTS)
        .map(|k| {
            (
                RETAIL_PARTY_NAME_TABLE_OFFSET + k * RETAIL_PARTY_NAME_STRIDE,
                RETAIL_PARTY_NAME_STRIDE,
            )
        })
        .collect();
    // All four roster slots: slot 3 (Terra) overlaps the story-flag
    // bitmap from internal +0x2BC, but its name field (+0x2A7..+0x2B0)
    // ends before the overlap begins, so writing it is safe.
    sites.extend((0..4).map(|n| {
        (
            RETAIL_GAME_DATA_OFFSET
                + RETAIL_CHAR_RECORD_HEADER_SIZE
                + n * RETAIL_CHAR_RECORD_STRIDE
                + NAME_OFFSET,
            NAME_LEN,
        )
    }));
    let mut changed = 0usize;
    for (off, len) in sites {
        let field = &mut sc_block[off..off + len];
        let cur_len = field.iter().position(|&b| b == 0).unwrap_or(len);
        if &field[..cur_len] == from.as_bytes() {
            field.fill(0);
            field[..to.len()].copy_from_slice(to.as_bytes());
            changed += 1;
        }
    }
    if changed > 0 {
        restamp_sc_block_checksum(sc_block);
    }
    Ok(changed)
}

/// Maximum number of fully non-overlapping character record slots in the
/// retail SC layout.
///
/// The four-slot record array is immediately followed by the global game
/// data, so slot 3 (Terra)'s `0x414`-byte footprint runs from `game+0x1004`
/// to `game+0x1418` and its **tail** (from internal offset `+0x2BC`,
/// i.e. `game+0x12C0`) overlaps the story-flag window at
/// [`RETAIL_STORY_FLAGS_OFFSET`] and the inventory at
/// [`RETAIL_INVENTORY_OFFSET`]. Terra's meaningful fields all sit *before*
/// that boundary - her name (`+0x2A7`), live HP/MP (`+0x104`) and
/// RecordStats (`+0x11C`) are in exclusive space - and she is the New Game
/// template's fourth roster entry (HP 400) but never a savable battle-party
/// member, so the tail aliasing is benign: there is no special-case code
/// path; the global region simply begins partway through the fourth slot.
/// Callers that want a strictly non-overlapping record walk cap at 3.
pub const RETAIL_MAX_CHAR_RECORDS: usize = 3;

/// Party records a retail SC block actually carries - the four contiguous
/// `0x80084708 + n * 0x414` records of `docs/formats/save-record.md`, and so
/// the `max_records` cap a block lift reads under.
///
/// This is the number every caller in the workspace passes, and it is **not**
/// [`RETAIL_MAX_CHAR_RECORDS`]: that one is the strictly non-overlapping walk
/// (3), capped one short because the fourth record's tail aliases the global
/// region. A lift wants all four - Terra's meaningful fields are all in
/// exclusive space - so passing the non-overlapping cap silently drops her.
pub const RETAIL_SC_PARTY_RECORDS: usize = 4;

/// Byte offset from the SC block start to the story-flag bitmap.
///
/// The window runs [`RETAIL_STORY_FLAGS_SIZE`] bytes up to the item array and
/// mirrors the live-RAM region `0x80085600..0x80085958`.  Derived via the linear
/// address formula `block_offset = RETAIL_GAME_DATA_OFFSET +
/// (ram_addr - SAVE_GAME_DATA_RAM_BASE)`:
/// `0x200 + (0x80085600 - 0x80084340) = 0x14C0`.
///
/// Confirmed by byte-match against mednafen save states cross-referenced
/// against retail Drake and Sebucus MCR save blocks.
pub const RETAIL_STORY_FLAGS_OFFSET: usize = 0x14C0;

/// Size of the story-flag window in the save block: live RAM
/// `0x80085600..0x80085958`, i.e. everything from the window's start up to
/// the item array at [`RETAIL_INVENTORY_OFFSET`].
///
/// The window's `+0x158` is the **system-flag bank** `0x80085758` (the
/// bitfield `FUN_8003CE08` / `_CE34` / `_CE64` set, clear and test), which is
/// `0x200` bytes long - flags `0x000..=0xFFF` - and so ends exactly at the
/// item array. Retail's card load restores all of it: the load arm of
/// `FUN_801DD35C` copies the whole `0x1A18`-byte live-state window back with
/// `FUN_8001A8B0(0x80084140, 0x801E5120, 0x1A18)` (`0x801DFA98..0x801DFAAC`).
/// A `0x200`-byte window here stopped at `0x80085800` and reached only the
/// bank's first `0xA8` bytes, so every flag from `0x540` up was dropped on a
/// lift.
pub const RETAIL_STORY_FLAGS_SIZE: usize = RETAIL_INVENTORY_OFFSET - RETAIL_STORY_FLAGS_OFFSET;

/// Byte offset from the SC block start to the global inventory array.
///
/// The inventory holds 72 slots of `(item_id: u8, count: u8)` pairs
/// (144 bytes total, `0x90`) and mirrors the live-RAM region
/// `0x80085958..0x800859E8`.  Derived via the linear address formula:
/// `0x200 + (0x80085958 - 0x80084340) = 0x1818`.
///
/// Confirmed by 100% byte-match against mednafen save states cross-referenced
/// against retail Drake and Sebucus MCR save blocks.
pub const RETAIL_INVENTORY_OFFSET: usize = 0x1818;

/// Slots in the general-consumable **display page** - what a cheat device's
/// *Have 99 Items* code covers, and not an engine bound.
///
/// The array is [`crate::retail_inventory::ITEM_SLOTS_TOTAL`] slots long and
/// the accessors bound on `gp[+0x2D4]`, which is only ever 128 or 256
/// (`docs/subsystems/inventory.md`). Reading or writing only this page drops
/// every item a played-through save holds above slot 71, which is why the
/// block lift and the block composer both walk
/// [`read_retail_item_window`]'s full span.
pub const RETAIL_INVENTORY_SLOTS: usize = 72;

/// Size in bytes of the consumable display page (`RETAIL_INVENTORY_SLOTS × 2`).
pub const RETAIL_INVENTORY_SIZE: usize = RETAIL_INVENTORY_SLOTS * 2; // 0x90

/// Size in bytes of the whole item array at `RETAIL_INVENTORY_OFFSET`
/// (`0x200` - 256 slots of `(id, count)`).
pub const RETAIL_ITEM_WINDOW_SIZE: usize = crate::retail_inventory::ITEM_SLOTS_TOTAL * 2;

/// Byte offset from the SC block start to the party **gold** (i32 LE).
///
/// Mirrors live RAM `0x8008459C` (`_DAT_8008459C`, the word the battle-victory
/// reward writer `FUN_8004F0E8` credits - see `docs/subsystems/boot.md`) via
/// the linear map: `0x200 + (0x8008459C - 0x80084340) = 0x45C`
/// (= `game_data + 0x25C`, the offset `docs/subsystems/save-screen.md` pins).
pub const RETAIL_GOLD_OFFSET: usize = 0x45C;

/// Byte offset from the SC block start to the casino **coin bank** (u32 LE).
///
/// Mirrors live RAM `0x800845A4` - the global the slot-machine overlay reads
/// at session entry and assigns on cash-out (`docs/subsystems/minigame-slot-machine.md`),
/// and the address the third-party cheat database classifies as `coins_u32`
/// (`crates/cheats/src/classify.rs`). Linear map:
/// `0x200 + (0x800845A4 - 0x80084340) = 0x464`.
///
/// An in-place write here is **not** the end of the edit: the block carries
/// its own additive checksum at [`RETAIL_BLOCK_CHECKSUM_OFFSET`] and every
/// writer in this module restamps it - see [`sc_block_checksum`].
pub const RETAIL_COINS_OFFSET: usize = 0x464;

/// Read the party gold (i32 LE at [`RETAIL_GOLD_OFFSET`]) from a retail SC
/// save block. `None` if the block is too small.
pub fn read_retail_gold(sc_block: &[u8]) -> Option<i32> {
    let b = sc_block.get(RETAIL_GOLD_OFFSET..RETAIL_GOLD_OFFSET + 4)?;
    Some(i32::from_le_bytes(b.try_into().unwrap()))
}

/// Write the party gold in place, restamping the block checksum
/// ([`restamp_sc_block_checksum`]). `Err` if the block is too small.
pub fn write_retail_gold(sc_block: &mut [u8], gold: i32) -> Result<()> {
    let end = RETAIL_GOLD_OFFSET + 4;
    if sc_block.len() < end {
        bail!("sc_block too small for retail gold field (need >= {end})");
    }
    sc_block[RETAIL_GOLD_OFFSET..end].copy_from_slice(&gold.to_le_bytes());
    restamp_sc_block_checksum(sc_block);
    Ok(())
}

/// Byte offset of the **field position snapshot** X (i32 LE), live RAM
/// `0x80084568`; the Z word follows at [`RETAIL_FIELD_POS_Z_OFFSET`].
///
/// The mode-change prologue `FUN_80016230` writes the pair when the field run
/// (mode `3`) hands over to the menu / battle modes (`0x08` / `0x14` / `0x18`
/// / `0x1A`): `lh` of the player actor's `+0x14` / `+0x18`, stored as a
/// sign-extended word (`0x80016400..0x8001641C`). So a save made from the
/// pause menu carries where the player stood.
///
/// Retail reads it back on the scene load that follows a **card load**: the
/// load arm of `FUN_801DD35C` raises `_DAT_8007B8C0 = 1` after its
/// `0x1A18`-byte copy (`0x801DFB04`), and the MAN loader `FUN_8003AEB0`, on
/// that flag, zeroes `_DAT_80073EFC` and copies this pair into the
/// destination-entry operand `_DAT_80073EF4` / `_DAT_80073EF8`
/// (`0x8003B764..0x8003B798`) - the words every field entry seats the player
/// from. No facing is stored: the arm zeroes `_DAT_80073EFC`.
pub const RETAIL_FIELD_POS_X_OFFSET: usize = 0x428;
/// Z word of the field position snapshot (live RAM `0x8008456C`).
pub const RETAIL_FIELD_POS_Z_OFFSET: usize = 0x42C;

/// Byte offset of the present-party **member count** (u8), live RAM
/// `0x80084594`: what the field VM's `PARTY_ADD` (op `0x3C`) bumps and every
/// roster walk (the pause menu, the item-window selector `FUN_8004313C`)
/// reads with `lbu`.
pub const RETAIL_PARTY_COUNT_OFFSET: usize = 0x454;
/// Byte offset of the party **leader** id (u8), live RAM `0x80084597`. The MAN
/// loader copies it into `_DAT_8007B8F8` (`0x8003B724..0x8003B72C`); the
/// leader swap `FUN_801D27E0` writes the same id here and at the head of the
/// member list.
pub const RETAIL_PARTY_LEADER_OFFSET: usize = 0x457;
/// Byte offset of the present-party **member list** (u8 roster ids), live
/// RAM `0x80084598`, [`RETAIL_PARTY_COUNT_OFFSET`] entries long.
pub const RETAIL_PARTY_MEMBERS_OFFSET: usize = 0x458;
/// Longest member list the block holds: `PARTY_ADD` caps at four, and the
/// party gold word follows at [`RETAIL_GOLD_OFFSET`].
pub const RETAIL_PARTY_MEMBERS_MAX: usize = RETAIL_GOLD_OFFSET - RETAIL_PARTY_MEMBERS_OFFSET;

/// The field position snapshot `(x, z)`, or `None` if the block is too small
/// or the snapshot was never taken. The words are sign-extended halfwords, so
/// the `i16` narrowing is exact.
///
/// `(0, 0)` reads as "never taken": it is the pair a window holds before the
/// first mode-change snapshot (a live state captured without a menu visit
/// carries it), and it is the grid's outer corner, not a place any scene
/// seats the party. Retail would seat a card load there regardless; the
/// engine lets that entry take the scene's own seat instead.
pub fn read_retail_field_position(sc_block: &[u8]) -> Option<(i16, i16)> {
    let word = |o: usize| -> Option<i16> {
        let b = sc_block.get(o..o + 4)?;
        Some(i32::from_le_bytes(b.try_into().unwrap()) as i16)
    };
    let pos = (
        word(RETAIL_FIELD_POS_X_OFFSET)?,
        word(RETAIL_FIELD_POS_Z_OFFSET)?,
    );
    (pos != (0, 0)).then_some(pos)
}

/// Write the field position snapshot as retail's `FUN_80016230` does (two
/// sign-extended words), restamping the block checksum.
pub fn write_retail_field_position(sc_block: &mut [u8], pos: (i16, i16)) -> Result<()> {
    let end = RETAIL_FIELD_POS_Z_OFFSET + 4;
    if sc_block.len() < end {
        bail!("sc_block too small for the field position snapshot (need >= {end})");
    }
    sc_block[RETAIL_FIELD_POS_X_OFFSET..RETAIL_FIELD_POS_X_OFFSET + 4]
        .copy_from_slice(&i32::from(pos.0).to_le_bytes());
    sc_block[RETAIL_FIELD_POS_Z_OFFSET..end].copy_from_slice(&i32::from(pos.1).to_le_bytes());
    restamp_sc_block_checksum(sc_block);
    Ok(())
}

/// Byte offset of the **configured audio level** (i32 LE), live RAM
/// `0x8008457C`: the reference the live level `_DAT_8007B910` is reset to.
///
/// The cold reset `FUN_8001FFA4` seeds it `0xD7`; the MAN loader
/// `FUN_8003AEB0` copies it into the live level on every non-skip scene load
/// (`_DAT_8007B910 = _DAT_8008457C`), so a card load re-applies the saved
/// value at the first scene it enters; and the battle duck targets a
/// percentage of it. Linear map: `0x200 + (0x8008457C - 0x80084340) = 0x43C`.
pub const RETAIL_AUDIO_LEVEL_OFFSET: usize = 0x43C;
/// Byte offset of the **voice / SFX volume** word (i32 LE), live RAM
/// `0x80084580`, cold-reset `200`. Every voice-attr key-on halves it into its
/// `vol_l` / `vol_r` pair (`(word << 15) >> 16`) - the SCUS cue drainer
/// `FUN_80016B6C` and the minigame overlays alike.
pub const RETAIL_VOICE_VOLUME_OFFSET: usize = 0x440;

/// The two audio-level words a retail save block carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailAudioLevels {
    /// `_DAT_8008457C` - the configured (reference) audio level.
    pub configured_level: i32,
    /// `_DAT_80084580` - the voice / SFX volume word.
    pub voice_volume: i32,
}

impl RetailAudioLevels {
    /// The values the cold reset `FUN_8001FFA4` stores.
    pub const COLD_RESET: Self = Self {
        configured_level: 0xD7,
        voice_volume: 200,
    };
}

/// Read both audio-level words. `None` when the block is too small, or when
/// both words are zero - a free block, or a live-state window the cold reset
/// never ran over; no retail save loads that pair, so reading it as a real
/// setting would import silence.
pub fn read_retail_audio_levels(sc_block: &[u8]) -> Option<RetailAudioLevels> {
    let word = |o: usize| -> Option<i32> {
        Some(i32::from_le_bytes(sc_block.get(o..o + 4)?.try_into().ok()?))
    };
    let levels = RetailAudioLevels {
        configured_level: word(RETAIL_AUDIO_LEVEL_OFFSET)?,
        voice_volume: word(RETAIL_VOICE_VOLUME_OFFSET)?,
    };
    (levels.configured_level != 0 || levels.voice_volume != 0).then_some(levels)
}

/// Write both audio-level words, restamping the block checksum.
pub fn write_retail_audio_levels(sc_block: &mut [u8], levels: RetailAudioLevels) -> Result<()> {
    let end = RETAIL_VOICE_VOLUME_OFFSET + 4;
    if sc_block.len() < end {
        bail!("sc_block too small for the audio-level words (need >= {end})");
    }
    sc_block[RETAIL_AUDIO_LEVEL_OFFSET..RETAIL_AUDIO_LEVEL_OFFSET + 4]
        .copy_from_slice(&levels.configured_level.to_le_bytes());
    sc_block[RETAIL_VOICE_VOLUME_OFFSET..end].copy_from_slice(&levels.voice_volume.to_le_bytes());
    restamp_sc_block_checksum(sc_block);
    Ok(())
}

/// The options screen's config words a retail save block carries, as each
/// row's value index (the word retail's picker writes, `FUN_801DA9F8`).
///
/// All nine sit in the live-state window the block mirrors, at `VA -
/// 0x80084140` like the audio level ([`RETAIL_OPTION_WORD_OFFSETS`]), so
/// retail's card load restores them with the rest of the window: a save made
/// with Run on resumes running. Vibration rows read `0` = Vibration On,
/// `1` = Off, as the picker stores them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetailOptions {
    /// `0x800846C0` Battle Camera: Close / Normal / Far.
    pub battle_camera: u8,
    /// `0x800846C4` Battle Select Attack: Select / Automatic / Command.
    pub select_attack: u8,
    /// `0x800846C8` Battle Command: directional buttons / cross button.
    pub battle_command: u8,
    /// `0x800846CC` Field Move: Walk / Run.
    pub field_move: u8,
    /// `0x800845C4` Field HP Display: Immediate / Gradual / Display Off.
    pub hp_display: u8,
    /// `0x800846BC` Sound: Stereo / Monaural.
    pub sound: u8,
    /// `0x800845C8` Dual Shock "Battles".
    pub vibration_battles: u8,
    /// `0x800845A8` Dual Shock "Events".
    pub vibration_events: u8,
    /// `0x800845CC` Dual Shock "Encounters".
    pub vibration_encounters: u8,
}

/// Block offset and choice count of each [`RetailOptions`] word, in
/// [`RetailOptions::to_bytes`] order.
pub const RETAIL_OPTION_WORD_OFFSETS: [(usize, u8); 9] = [
    (0x580, 3),
    (0x584, 3),
    (0x588, 2),
    (0x58C, 2),
    (0x484, 3),
    (0x57C, 2),
    (0x488, 2),
    (0x468, 2),
    (0x48C, 2),
];

impl RetailOptions {
    /// The nine values in [`RETAIL_OPTION_WORD_OFFSETS`] order.
    pub fn to_bytes(self) -> [u8; 9] {
        [
            self.battle_camera,
            self.select_attack,
            self.battle_command,
            self.field_move,
            self.hp_display,
            self.sound,
            self.vibration_battles,
            self.vibration_events,
            self.vibration_encounters,
        ]
    }

    /// The inverse of [`Self::to_bytes`]. `None` when a value is past its
    /// row's choices.
    pub fn from_bytes(b: [u8; 9]) -> Option<Self> {
        if b.iter()
            .zip(RETAIL_OPTION_WORD_OFFSETS)
            .any(|(&v, (_, n))| v >= n)
        {
            return None;
        }
        Some(Self {
            battle_camera: b[0],
            select_attack: b[1],
            battle_command: b[2],
            field_move: b[3],
            hp_display: b[4],
            sound: b[5],
            vibration_battles: b[6],
            vibration_events: b[7],
            vibration_encounters: b[8],
        })
    }
}

/// Read the nine option words. `None` when the block is too small or a word
/// is past its row's choices - not an options window (a continuation block,
/// or bytes retail never wrote there).
pub fn read_retail_options(sc_block: &[u8]) -> Option<RetailOptions> {
    let mut b = [0u8; 9];
    for (dst, (o, _)) in b.iter_mut().zip(RETAIL_OPTION_WORD_OFFSETS) {
        let w = i32::from_le_bytes(sc_block.get(o..o + 4)?.try_into().ok()?);
        *dst = u8::try_from(w).ok()?;
    }
    RetailOptions::from_bytes(b)
}

/// Write the nine option words, restamping the block checksum.
pub fn write_retail_options(sc_block: &mut [u8], options: RetailOptions) -> Result<()> {
    let end = RETAIL_OPTION_WORD_OFFSETS
        .iter()
        .map(|&(o, _)| o + 4)
        .max()
        .unwrap_or(0);
    if sc_block.len() < end {
        bail!("sc_block too small for the option words (need >= {end})");
    }
    for (v, (o, _)) in options
        .to_bytes()
        .into_iter()
        .zip(RETAIL_OPTION_WORD_OFFSETS)
    {
        sc_block[o..o + 4].copy_from_slice(&i32::from(v).to_le_bytes());
    }
    restamp_sc_block_checksum(sc_block);
    Ok(())
}

/// The present-party member list (roster ids, count taken from
/// [`RETAIL_PARTY_COUNT_OFFSET`]). `None` when the block is too small or the
/// count is `0` or past [`RETAIL_PARTY_MEMBERS_MAX`] - a free block, not a
/// party.
pub fn read_retail_present_party(sc_block: &[u8]) -> Option<Vec<u8>> {
    let n = usize::from(*sc_block.get(RETAIL_PARTY_COUNT_OFFSET)?);
    if !(1..=RETAIL_PARTY_MEMBERS_MAX).contains(&n) {
        return None;
    }
    Some(
        sc_block
            .get(RETAIL_PARTY_MEMBERS_OFFSET..RETAIL_PARTY_MEMBERS_OFFSET + n)?
            .to_vec(),
    )
}

/// Write the present party: the count, the leader (the list's head, as the
/// leader swap keeps it) and the member list zero-padded to
/// [`RETAIL_PARTY_MEMBERS_MAX`], restamping the block checksum. `Err` on an
/// empty or over-long list, or a block too small.
pub fn write_retail_present_party(sc_block: &mut [u8], members: &[u8]) -> Result<()> {
    if !(1..=RETAIL_PARTY_MEMBERS_MAX).contains(&members.len()) {
        bail!(
            "present party must hold 1..={RETAIL_PARTY_MEMBERS_MAX} members, got {}",
            members.len()
        );
    }
    let end = RETAIL_PARTY_MEMBERS_OFFSET + RETAIL_PARTY_MEMBERS_MAX;
    if sc_block.len() < end {
        bail!("sc_block too small for the present-party list (need >= {end})");
    }
    sc_block[RETAIL_PARTY_COUNT_OFFSET] = members.len() as u8;
    sc_block[RETAIL_PARTY_LEADER_OFFSET] = members[0];
    let dst = &mut sc_block[RETAIL_PARTY_MEMBERS_OFFSET..end];
    dst.fill(0);
    dst[..members.len()].copy_from_slice(members);
    restamp_sc_block_checksum(sc_block);
    Ok(())
}

/// Byte offset of the **play-time counter** (u32 LE), live RAM `0x80084570`
/// (`0x200 + (0x80084570 - 0x80084340) = 0x430`).
///
/// The counter ticks [`RETAIL_PLAY_COUNTER_HZ`] times a second: the save
/// screen's time line divides it by `216000` for hours (`(c >> 6) *
/// 0x026D60DD`, high word `>> 5`) and by `3600` for total minutes (`c *
/// 0x91A2B3C5`, high word `>> 11`), then clamps the display at `99:59`
/// (menu overlay, `0x801DD5C8..0x801DD618`). The New Game slate zeroes it
/// (`FUN_8001DCF8`, `sw zero` at `0x8001DD58`).
pub const RETAIL_PLAY_COUNTER_OFFSET: usize = 0x430;

/// Ticks per second of the play-time counter at [`RETAIL_PLAY_COUNTER_OFFSET`].
pub const RETAIL_PLAY_COUNTER_HZ: u32 = 60;

/// Read the play-time counter (u32 LE at [`RETAIL_PLAY_COUNTER_OFFSET`], in
/// `1/60` s ticks) from a retail SC save block. `None` if the block is too
/// small.
pub fn read_retail_play_counter(sc_block: &[u8]) -> Option<u32> {
    let b = sc_block.get(RETAIL_PLAY_COUNTER_OFFSET..RETAIL_PLAY_COUNTER_OFFSET + 4)?;
    Some(u32::from_le_bytes(b.try_into().unwrap()))
}

/// Write the play-time counter in place, restamping the block checksum.
/// `Err` if the block is too small.
pub fn write_retail_play_counter(sc_block: &mut [u8], ticks: u32) -> Result<()> {
    let end = RETAIL_PLAY_COUNTER_OFFSET + 4;
    if sc_block.len() < end {
        bail!("sc_block too small for the retail play counter (need >= {end})");
    }
    sc_block[RETAIL_PLAY_COUNTER_OFFSET..end].copy_from_slice(&ticks.to_le_bytes());
    restamp_sc_block_checksum(sc_block);
    Ok(())
}

/// Read the casino coin bank (u32 LE at [`RETAIL_COINS_OFFSET`]) from a
/// retail SC save block. `None` if the block is too small.
pub fn read_retail_coins(sc_block: &[u8]) -> Option<u32> {
    let b = sc_block.get(RETAIL_COINS_OFFSET..RETAIL_COINS_OFFSET + 4)?;
    Some(u32::from_le_bytes(b.try_into().unwrap()))
}

/// Write the casino coin bank in place - the targeted patch the site's
/// minigames use to bank won coins into a player's own retail save.
///
/// Two fields change: the coin slot and the block checksum word
/// ([`restamp_sc_block_checksum`]), which retail's loader compares before it
/// will accept the block. `Err` if the block is too small.
pub fn write_retail_coins(sc_block: &mut [u8], coins: u32) -> Result<()> {
    let end = RETAIL_COINS_OFFSET + 4;
    if sc_block.len() < end {
        bail!("sc_block too small for retail coin field (need >= {end})");
    }
    sc_block[RETAIL_COINS_OFFSET..end].copy_from_slice(&coins.to_le_bytes());
    restamp_sc_block_checksum(sc_block);
    Ok(())
}

/// Byte offset from the SC block start to the most-recently-visited CDNAME
/// scene label (NUL-terminated ASCII) - `game_data + 0x208`, the field the
/// save-select screen reads for its location line's scene join and the
/// scene retail's loader resumes into. The previous scene's label follows at
/// `+0x418` (same width).
pub const RETAIL_SCENE_LABEL_OFFSET: usize = 0x408;

/// Width of the scene-label field: `0x10` bytes, NUL-terminated (the
/// previous-scene label starts at `RETAIL_SCENE_LABEL_OFFSET + 0x10`).
pub const RETAIL_SCENE_LABEL_LEN: usize = 0x10;

/// Byte offset from the SC block start to the location display name
/// (NUL-terminated ASCII at `game_data + 0x000`).
pub const RETAIL_LOCATION_NAME_OFFSET: usize = RETAIL_GAME_DATA_OFFSET;

/// Width of the location-name field. Retail installs the scene MAN's
/// section-2 banner name here with a fixed `0x24`-byte copy
/// (`FUN_8001A8B0(0x80084340, name, 0x24)` at `0x801E1A28`, see
/// `docs/formats/place-names.md`), so whatever followed the name's NUL in the
/// MAN rides along on a real card; a writer here NUL-pads the whole field.
pub const RETAIL_LOCATION_NAME_LEN: usize = 0x24;

/// Bytes of live game state a retail save block carries: the composer
/// (`FUN_801E1934`) memsets its `0x2000`-byte buffer, copies `0x1A18` bytes
/// from `0x80084140` over the front (`_li a2,0x1a18` at `0x801e1bd8`), and
/// the loader (`FUN_801DD35C`) copies the same `0x1A18` bytes back
/// (`_li a2,0x1a18` at `0x801dfaac`). Everything from here to the checksum
/// word at [`RETAIL_BLOCK_CHECKSUM_OFFSET`] is zero on a retail card and
/// never read by the game - the region the engine-ext blob
/// (`crate::ext::RETAIL_ENGINE_EXT_OFFSET`) occupies.
pub const RETAIL_LIVE_STATE_SIZE: usize = 0x1A18;

/// A NUL-terminated printable-ASCII string at `at`, at most `width` bytes.
/// Stops at the first NUL **or** non-printable byte - a free block's
/// leftover bytes never come back as a name. `None` if `at` is out of range.
fn retail_ascii_field(sc_block: &[u8], at: usize, width: usize) -> Option<String> {
    let field = sc_block.get(at..at + width)?;
    Some(
        field
            .iter()
            .take_while(|&&b| (0x20..=0x7E).contains(&b))
            .map(|&b| b as char)
            .collect(),
    )
}

/// The CDNAME scene label at [`RETAIL_SCENE_LABEL_OFFSET`], or `None` if the
/// block is too short. Empty for a block nothing composed there.
pub fn read_retail_scene_label(sc_block: &[u8]) -> Option<String> {
    retail_ascii_field(sc_block, RETAIL_SCENE_LABEL_OFFSET, RETAIL_SCENE_LABEL_LEN)
}

/// The location display name at [`RETAIL_LOCATION_NAME_OFFSET`], or `None`
/// if the block is too short. Empty for a block nothing composed there.
pub fn read_retail_location_name(sc_block: &[u8]) -> Option<String> {
    retail_ascii_field(
        sc_block,
        RETAIL_LOCATION_NAME_OFFSET,
        RETAIL_LOCATION_NAME_LEN,
    )
}

/// Write the resume point - the CDNAME `scene` label and the `location`
/// display name - into their retail fields in place, NUL-padding each to its
/// full width (truncating to `width - 1` so the terminator survives), and
/// restamp the block checksum. Non-ASCII characters are written as `?`.
/// `Err` if the block is too small to hold both fields.
pub fn write_retail_resume(sc_block: &mut [u8], scene: &str, location: &str) -> Result<()> {
    let need = RETAIL_SCENE_LABEL_OFFSET + RETAIL_SCENE_LABEL_LEN;
    if sc_block.len() < need {
        bail!("sc_block too small for the resume fields (need {need} bytes)");
    }
    let put = |sc_block: &mut [u8], at: usize, width: usize, text: &str| {
        let field = &mut sc_block[at..at + width];
        field.fill(0);
        for (dst, c) in field.iter_mut().zip(text.chars()).take(width - 1) {
            *dst = if c.is_ascii() && !c.is_ascii_control() {
                c as u8
            } else {
                b'?'
            };
        }
    };
    put(
        sc_block,
        RETAIL_SCENE_LABEL_OFFSET,
        RETAIL_SCENE_LABEL_LEN,
        scene,
    );
    put(
        sc_block,
        RETAIL_LOCATION_NAME_OFFSET,
        RETAIL_LOCATION_NAME_LEN,
        location,
    );
    restamp_sc_block_checksum(sc_block);
    Ok(())
}

/// RAM base address for the save game data block (`game_data[0]` in the SC block).
///
/// The save block's `game_data` region (starting at `RETAIL_GAME_DATA_OFFSET`)
/// is a structured dump whose global fields (gold, story flags, inventory, …)
/// map linearly to live RAM via:
///
/// ```text
/// sc_block_offset = RETAIL_GAME_DATA_OFFSET + (ram_addr - SAVE_GAME_DATA_RAM_BASE)
/// ```
///
/// Derived by anchoring the confirmed inventory address:
/// `0x80085958 - game_data_offset(0x1618) = 0x80084340`.
/// Cross-validated against party gold (`0x8008459C` → `game_data+0x025C`),
/// story-flag bitmap (`0x80085600` → `game_data+0x12C0`), and the active
/// scene slot (`0x80084540` → `game_data+0x0200`).
pub const SAVE_GAME_DATA_RAM_BASE: u32 = 0x80084340;

/// Extract raw character record bytes from a retail SC save block.
///
/// `sc_block` is the full 8192-byte save block starting with the `SC` magic.
/// Returns at most `max_records` records. Stops early at the first all-zero
/// record (unused / empty slot). Returns `None` if the block is too small to
/// hold even the header region.
///
/// Each returned `Vec<u8>` is exactly `RETAIL_CHAR_RECORD_STRIDE` (0x414) bytes
/// and can be parsed by `legaia_save::CharacterRecord::parse`.
///
/// # Example
///
/// ```
/// # use legaia_save::card::{read_retail_char_records, RETAIL_GAME_DATA_OFFSET};
/// let sc_block = vec![0u8; 8192];
/// // An all-zero block yields zero records (first slot is empty).
/// assert!(read_retail_char_records(&sc_block, 4).map_or(true, |v| v.is_empty()));
/// ```
pub fn read_retail_char_records(sc_block: &[u8], max_records: usize) -> Option<Vec<Vec<u8>>> {
    let game_data = sc_block.get(RETAIL_GAME_DATA_OFFSET..)?;
    let records_start = game_data.get(RETAIL_CHAR_RECORD_HEADER_SIZE..)?;
    let mut out = Vec::new();
    for i in 0..max_records {
        // Use checked arithmetic: `max_records` is caller-supplied, so a huge
        // value must stop the walk (the `?` short-circuit), never overflow the
        // `offset` / `offset + STRIDE` computation into a slice-index panic.
        let offset = i.checked_mul(RETAIL_CHAR_RECORD_STRIDE)?;
        let end = offset.checked_add(RETAIL_CHAR_RECORD_STRIDE)?;
        let record = records_start.get(offset..end)?;
        if record.iter().all(|&b| b == 0) {
            break; // stop at first empty slot
        }
        out.push(record.to_vec());
    }
    Some(out)
}

/// Extract the story-flag window from a retail SC save block.
///
/// Returns the [`RETAIL_STORY_FLAGS_SIZE`]-byte slice at
/// `RETAIL_STORY_FLAGS_OFFSET`, or `None` if the block is too small.  Each bit
/// corresponds to one game-progress flag (visited towns, event triggers, Door
/// of Wind flags, etc.); the system-flag bank is the slice's `+0x158..`.
///
/// The slice mirrors the live-RAM region `0x80085600..0x80085958`.
pub fn read_retail_story_flags(sc_block: &[u8]) -> Option<&[u8]> {
    sc_block.get(RETAIL_STORY_FLAGS_OFFSET..RETAIL_STORY_FLAGS_OFFSET + RETAIL_STORY_FLAGS_SIZE)
}

/// Extract the 72-slot inventory from a retail SC save block.
///
/// Returns the 144-byte raw inventory array at `RETAIL_INVENTORY_OFFSET`, or
/// `None` if the block is too small.  Each slot is a 2-byte `(item_id: u8,
/// count: u8)` pair in little-endian order.  An `item_id` of `0` with
/// `count` of `0` denotes an empty slot.
///
/// The slice mirrors the live-RAM region `0x80085958..0x800859E8`.
pub fn read_retail_inventory(sc_block: &[u8]) -> Option<&[u8]> {
    sc_block.get(RETAIL_INVENTORY_OFFSET..RETAIL_INVENTORY_OFFSET + RETAIL_INVENTORY_SIZE)
}

/// Extract the full 256-slot **accessor item window** from a retail SC block.
///
/// [`read_retail_inventory`] returns the 72-slot general consumable page.
/// The retail accessor family (`FUN_8004313C` window select and the
/// find / consume / add entries, `crate::retail_inventory`) walks a window of
/// up to [`crate::retail_inventory::ITEM_SLOTS_TOTAL`] slots over the same
/// base - `SC+0x1818..SC+0x1A18`, exactly the tail of the `0x1A18`-byte live
/// game-state snapshot the save block carries. This returns that whole span
/// (`0x200` bytes, `(id, count)` pairs), or `None` if the block is too small.
pub fn read_retail_item_window(sc_block: &[u8]) -> Option<&[u8]> {
    let len = crate::retail_inventory::ITEM_SLOTS_TOTAL * 2;
    sc_block.get(RETAIL_INVENTORY_OFFSET..RETAIL_INVENTORY_OFFSET + len)
}

/// Write character records into a retail SC save block in place.
///
/// `records` are written sequentially starting at the retail record region
/// (`RETAIL_GAME_DATA_OFFSET + RETAIL_CHAR_RECORD_HEADER_SIZE`). Each record
/// occupies exactly `RETAIL_CHAR_RECORD_STRIDE` (0x414) bytes; shorter slices
/// are zero-padded, longer ones are truncated.
///
/// Returns the number of slots written. Returns `Err` if the SC block is too
/// small to hold even one record. The function does **not** zero trailing
/// slots: the retail SC layout places the story-flag bitmap (`+0x14C0`) and
/// inventory (`+0x1818`) regions in addresses that overlap what would
/// otherwise be the 4th and 5th record slots, so blindly zero-padding
/// trailing slots would clobber later writes. Callers that want a clean
/// block should zero-fill the buffer before writing.
pub fn write_retail_char_records(sc_block: &mut [u8], records: &[Vec<u8>]) -> Result<usize> {
    let records_start = RETAIL_GAME_DATA_OFFSET + RETAIL_CHAR_RECORD_HEADER_SIZE;
    if sc_block.len() < records_start + RETAIL_CHAR_RECORD_STRIDE {
        bail!(
            "sc_block too small for retail char record region (need >= {}, got {})",
            records_start + RETAIL_CHAR_RECORD_STRIDE,
            sc_block.len()
        );
    }
    let cap = (sc_block.len() - records_start) / RETAIL_CHAR_RECORD_STRIDE;
    let n = records.len().min(cap);
    for (i, rec) in records.iter().take(n).enumerate() {
        let off = records_start + i * RETAIL_CHAR_RECORD_STRIDE;
        let dst = &mut sc_block[off..off + RETAIL_CHAR_RECORD_STRIDE];
        dst.fill(0);
        let take = rec.len().min(RETAIL_CHAR_RECORD_STRIDE);
        dst[..take].copy_from_slice(&rec[..take]);
    }
    restamp_sc_block_checksum(sc_block);
    Ok(n)
}

/// Write the story-flag window into a retail SC save block in place.
///
/// `bits` shorter than [`RETAIL_STORY_FLAGS_SIZE`] is zero-padded on the right;
/// longer slices are truncated. Returns the number of bytes written.
/// Returns `Err` if the SC block is too small to hold the bitmap region.
pub fn write_retail_story_flags(sc_block: &mut [u8], bits: &[u8]) -> Result<usize> {
    let end = RETAIL_STORY_FLAGS_OFFSET + RETAIL_STORY_FLAGS_SIZE;
    if sc_block.len() < end {
        bail!(
            "sc_block too small for retail story-flag region (need >= {}, got {})",
            end,
            sc_block.len()
        );
    }
    let dst = &mut sc_block[RETAIL_STORY_FLAGS_OFFSET..end];
    dst.fill(0);
    let take = bits.len().min(RETAIL_STORY_FLAGS_SIZE);
    dst[..take].copy_from_slice(&bits[..take]);
    restamp_sc_block_checksum(sc_block);
    Ok(take)
}

/// Write the 72-slot inventory into a retail SC save block in place.
///
/// `pairs` is an engine-side `(item_id, count)` list, **not** a pre-laid slot
/// array: it is composed into the window through retail's own add + normalize
/// accessors ([`crate::retail_inventory::compose_window`]), so the block that
/// lands is one retail's item code could itself have produced - stacks clamped
/// at 99, one stack per id, no `id == 0` phantoms. Copying the list in
/// positionally instead is how a bag with 137 Healing Leaves, or two stacks of
/// the same id, would reach a real memory card.
///
/// Trailing slots are zeroed. Returns the number of **occupied** slots (which
/// is below `pairs.len()` when duplicate ids merged). Grants past the window's
/// capacity are still dropped - unchanged behaviour, and this signature has no
/// channel to report it; a caller that needs to know calls
/// [`crate::retail_inventory::compose_window`] directly and reads its
/// [`RefusedGrant`](crate::retail_inventory::RefusedGrant) list. Returns `Err`
/// if the SC block is too small to hold the inventory region.
pub fn write_retail_inventory(sc_block: &mut [u8], pairs: &[(u8, u8)]) -> Result<usize> {
    let end = RETAIL_INVENTORY_OFFSET + RETAIL_ITEM_WINDOW_SIZE;
    if sc_block.len() < end {
        bail!(
            "sc_block too small for retail inventory region (need >= {}, got {})",
            end,
            sc_block.len()
        );
    }
    let (inv, _refused) = crate::retail_inventory::compose_window(
        crate::retail_inventory::ITEM_WINDOW_BASE,
        crate::retail_inventory::ITEM_SLOTS_TOTAL,
        pairs,
    );
    let dst = &mut sc_block[RETAIL_INVENTORY_OFFSET..end];
    dst.fill(0);
    let mut occupied = 0usize;
    for (i, &(id, count)) in inv.slots().iter().enumerate() {
        dst[i * 2] = id;
        dst[i * 2 + 1] = count;
        if id != 0 {
            occupied = i + 1;
        }
    }
    restamp_sc_block_checksum(sc_block);
    Ok(occupied)
}

/// Write a **pre-laid slot array** into the SC block's item region, verbatim.
///
/// The sibling of [`write_retail_inventory`], for a caller that already holds
/// retail's own array - slot order, holes, duplicate stacks and all - and must
/// not have it re-composed. `slots` shorter than the array zero-fills the
/// tail; longer is truncated. Returns the highest occupied slot index plus
/// one, and restamps the block checksum.
///
/// Keeping both is the point: a caller with a *list* has no slot coordinate
/// and must go through retail's add path to produce a representable block,
/// while a caller with the array would have its holes squeezed out by that
/// same path - and a hole is exactly what the one slot-addressed consumer
/// (PROT 0941's Steal) draws over.
pub fn write_retail_item_window(sc_block: &mut [u8], slots: &[(u8, u8)]) -> Result<usize> {
    let end = RETAIL_INVENTORY_OFFSET + RETAIL_ITEM_WINDOW_SIZE;
    if sc_block.len() < end {
        bail!(
            "sc_block too small for retail inventory region (need >= {}, got {})",
            end,
            sc_block.len()
        );
    }
    let dst = &mut sc_block[RETAIL_INVENTORY_OFFSET..end];
    dst.fill(0);
    let mut occupied = 0usize;
    for (i, &(id, count)) in slots
        .iter()
        .take(crate::retail_inventory::ITEM_SLOTS_TOTAL)
        .enumerate()
    {
        dst[i * 2] = id;
        dst[i * 2 + 1] = count;
        if id != 0 {
            occupied = i + 1;
        }
    }
    restamp_sc_block_checksum(sc_block);
    Ok(occupied)
}
