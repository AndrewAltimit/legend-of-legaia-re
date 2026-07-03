//! Capture observation: the per-STR FMV-trigger save corpus (fmv_id / game-mode / BGM globals).

/// PSX-virtual address of the FMV-id global written by the
/// field-VM op `0x4C 0xE2` handler at `0x801E30E4`. The runtime
/// FMV-state selector at `0x801CECA0` reads this as a `s16`.
pub const FMV_ID_ADDR: u32 = 0x8007_BA78;

/// PSX-virtual address of the next-game-mode global. Every
/// FMV-trigger writer pokes this to `0x1A` (StrInit).
pub const GAME_MODE_ADDR: u32 = 0x8007_B83C;

/// Expected game mode value when the corpus saves are loaded.
/// The main mode dispatcher transitions to mode `26 = StrInit`
/// on the next frame.
pub const EXPECTED_GAME_MODE: u8 = 0x1A;

/// PSX-virtual address of the BGM ID global written by the
/// field-VM op `0x35` sub-op `1` BGM selector. The trigger path
/// resets this to `2000` (global pool index `0`) before the
/// FMV plays.
pub const BGM_ID_ADDR: u32 = 0x8007_BAC8;

/// Expected BGM ID value across the corpus. `2000` resolves to
/// global pool entry `0` per the BGM resolver `FUN_800243F0`.
pub const EXPECTED_BGM_ID: u16 = 2000;

/// Expected scene name in the scene-bundle pool (slots 0 + 1).
/// All nine saves share this label; per-save corpus assertions
/// can use it as a fast residency check.
pub const EXPECTED_SCENE_LABEL: &str = "map01";

/// Expected `recover_base` return value for every save in the
/// corpus - the `map01` field-pack base. Pins `map01`'s
/// field-pack runtime residency for cross-referencing against
/// `FMV_TRIGGER_FIELD_SCENES` and the existing
/// `field_pack_load::TOWN01_FIELD_PACK_BASE` constant.
pub const MAP01_FIELD_PACK_BASE: u32 = 0x80139530;

/// One save in the per-STR FMV corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorpusEntry {
    /// Mednafen save-state slot suffix (`mc{N}`).
    pub slot: u32,
    /// FMV index the field-VM / debug menu wrote to
    /// [`FMV_ID_ADDR`] before this save was taken.
    pub expected_fmv_id: i16,
}

/// Nine corpus entries, one per `fmv_id ∈ 0..=8`. The user-side
/// slot numbering is `[2,3,4,5,6,7,8,9,0]` mapped to
/// `expected_fmv_id ∈ 0..=8`.
pub const CORPUS: [CorpusEntry; 9] = [
    CorpusEntry {
        slot: 2,
        expected_fmv_id: 0,
    },
    CorpusEntry {
        slot: 3,
        expected_fmv_id: 1,
    },
    CorpusEntry {
        slot: 4,
        expected_fmv_id: 2,
    },
    CorpusEntry {
        slot: 5,
        expected_fmv_id: 3,
    },
    CorpusEntry {
        slot: 6,
        expected_fmv_id: 4,
    },
    CorpusEntry {
        slot: 7,
        expected_fmv_id: 5,
    },
    CorpusEntry {
        slot: 8,
        expected_fmv_id: 6,
    },
    CorpusEntry {
        slot: 9,
        expected_fmv_id: 7,
    },
    CorpusEntry {
        slot: 0,
        expected_fmv_id: 8,
    },
];

/// Read the FMV-id global from main RAM (signed 16-bit LE).
pub fn read_fmv_id(main_ram: &[u8]) -> Option<i16> {
    let off = (FMV_ID_ADDR - 0x80000000) as usize;
    legaia_bytes::i16_le(main_ram, off)
}

/// Read the game-mode byte from main RAM.
pub fn read_game_mode(main_ram: &[u8]) -> Option<u8> {
    let off = (GAME_MODE_ADDR - 0x80000000) as usize;
    main_ram.get(off).copied()
}

/// Read the BGM-id global from main RAM (unsigned 16-bit LE).
pub fn read_bgm_id(main_ram: &[u8]) -> Option<u16> {
    let off = (BGM_ID_ADDR - 0x80000000) as usize;
    legaia_bytes::u16_le(main_ram, off)
}

/// Search the field-pack region following `field_pack_base` for
/// the field-VM FMV-trigger op `0x4C 0xE2 lo hi`. Returns each
/// match as `(absolute_addr, fmv_id_operand)`. Used to confirm
/// (or refute) that the captured save still has the trigger
/// bytecode resident - the corpus saves return zero matches, a
/// stable signature of the debug-menu-driven trigger path.
pub fn scan_field_pack_for_trigger_ops(
    main_ram: &[u8],
    field_pack_base: u32,
    scan_len: u32,
) -> Vec<(u32, i16)> {
    let lo = (field_pack_base - 0x80000000) as usize;
    let hi = (lo + scan_len as usize).min(main_ram.len());
    let bytes = &main_ram[lo..hi];
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 < bytes.len() {
        if bytes[i] == 0x4C && bytes[i + 1] == 0xE2 {
            let id = i16::from_le_bytes([bytes[i + 2], bytes[i + 3]]);
            out.push((field_pack_base + i as u32, id));
            i += 4;
        } else {
            i += 1;
        }
    }
    out
}
