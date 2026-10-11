//! The shared injection map every code mod plans against: the same-size
//! [`Edit`] a plan is made of, the verified-dead regions of `SCUS_942.54` and
//! the menu overlay that hold injected routines and data, the live-table
//! ranges a region must stay out of, and the few retail addresses several
//! mods share (the slot-A overlay base, the battle-setup hook site, the BIOS
//! `rand` thunk).
//!
//! A feature mod names this module to find free space; it never names
//! another feature. Which flag owns which region is recorded once, in
//! `legaia_disc_patch::space_ledger` (the same addresses, with their owners
//! and evidence); `legaia-patcher`'s ledger test holds the two in step.

/// One same-size write into a target file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// `None` = `SCUS_942.54`; `Some(idx)` = PROT entry `idx` (raw).
    pub prot_index: Option<usize>,
    /// File offset within that target.
    pub file_off: usize,
    /// Little-endian bytes to write.
    pub bytes: Vec<u8>,
}

// --- Overlay images ---------------------------------------------------------

/// PROT entry index of the battle-action overlay (0898).
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize =
    legaia_game_tables::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;
/// PROT entry index of the menu overlay (0899) hosting the spell-list renderer.
pub const MENU_OVERLAY_PROT_INDEX: usize = 899;
/// Load base VA shared by the slot-A overlays (0897 / 0898 / 0899). A VA inside
/// one maps to PROT-entry file offset `va - OVERLAY_BASE_VA`.
pub const OVERLAY_BASE_VA: u32 = legaia_game_tables::move_power::BATTLE_OVERLAY_BASE;

// --- Shared retail sites ----------------------------------------------------

/// Battle-setup hook site (SCUS, after the monster-setup loop in
/// `FUN_800513F0`): the per-battle roll of `--shiny-seru` and of
/// `--oscillating-ap` both detour here, so the two are mutually exclusive.
pub const HOOK_SETUP_VA: u32 = 0x8005_1A20;
pub const HOOK_SETUP_W0: u32 = 0x3C02_8008; // lui v0,0x8008
/// BIOS `rand` thunk (returns `v0`).
pub const RAND_FUNC_VA: u32 = 0x8005_6798;

// --- Code-cave layout ------------------------------------------------------
//
// IMPORTANT: a region being all-zero in the clean SCUS / overlay is NOT proof
// it is dead. Several static tables (the victory mouth-override table at
// `0x80077E80`, the move-power table at `0x801F4F5C`) have zero-padded
// rows/records that the game still INDEXES at runtime. An earlier layout put
// routines in those zero tails: the victory-pose face animator read the routine
// bytes as facial keyframes (corrupted mouth) and six move ids read them as
// move-power records (garbage damage / trail texpage). Every region below is
// chosen to be (a) all-zero in the clean image, (b) constant-zero across diverse
// in-battle save states (so it is not a runtime buffer like a name scratch), and
// (c) outside every known live table - the last enforced by [`SCUS_TABLE_RANGES`]
// / [`OVERLAY_TABLE_RANGES`] at plan time.

/// Known live data tables in `SCUS_942.54` (VA start, end-exclusive). A routine
/// or data region must not overlap any of these even if the overlapped bytes are
/// zero - they are indexed at runtime. Pinned from `docs/reference/memory-map.md`,
/// the `legaia_actor_formats::face_anim` / `item_names` table addresses, and the SsAPI
/// sound/effect tables found by read-watching a live battle (their zero padding
/// is what the old arena3/4/5 wrongly squatted in - the Healing-Leaf freeze).
pub const SCUS_TABLE_RANGES: &[(u32, u32)] = &[
    // Dialog-font + text-render live block, up to the item-name table. Covers the
    // 256-byte glyph advance/width table (`0x80073F1C`, `legaia_font`), the 38-entry
    // `0xCE`-escape table (`0x80074050`), the per-glyph advance-padding var
    // `DAT_800740E8` (read every glyph by the font renderer `FUN_80036888`), the
    // text-render globals + handler table (`0x800742EC`/`0x800742F0`), and the 4xu32
    // ability bitmask (`0x80074358`, memory-map.md). A read-watch sweep found 11
    // live refs inside this window - a plain zero-run here is NOT dead space.
    (0x8007_3F1C, 0x8007_436C),
    (0x8007_436C, 0x8007_625C), // item / equipment / spell name + stat tables
    (0x8007_625C, 0x8007_6900), // accessory table + face source/geo/delta tables
    (0x8007_0700, 0x8007_078C), // pad before the 28-mode game-mode table (old arena3)
    (0x8007_7828, 0x8007_7A28), // per-monster steal table (256 * 2 bytes)
    (0x8007_7E80, 0x8007_8800), // victory mouth-override table (rows 0x800781B0..) + party-size
    (0x8007_8870, 0x8007_88C0), // party-member size table (`0x80078878`)
    (0x8007_8C4C, 0x8007_8CC0), // new-game starting-party template
    // SsAPI sound/effect tables (read-watch-pinned: FUN_8005c0c8/8005d0b8 index
    // 0x800794f0 etc; FUN_8005a210/8005a358 read 0x80078d54/0x80078e48; SPU DMA
    // + FUN_8006xxxx read the 0x80079800.. buffer and the 0x8007af00 I/O table).
    (0x8007_8D00, 0x8007_9800), // sound tables incl. 0x800794f0 (old arena4/arena5)
    (0x8007_97D0, 0x8007_A900), // SsAPI value/DMA buffer cluster
    (0x8007_AF00, 0x8007_AFF8), // SsAPI I/O register table (between arena1 and arena2)
    (0x8007_B040, 0x8007_B800), // trailing SsAPI value tables + SPU-transfer scratch
];
/// Known live tables in the battle-action overlay (0898), same VA space. The
/// move-id index map + the move-power table window (which absorbed the old cave).
pub const OVERLAY_TABLE_RANGES: &[(u32, u32)] = &[(0x801F_4E63, 0x801F_69D8)];

/// SCUS rodata gap (padding before the steal table). Hosts the scratch word +
/// the setup (B) and capture-copy (C1) routines. Reference-free; not in any
/// table.
pub const SCUS_GAP_VA: u32 = 0x8007_7728;
/// First VA used by the steal table; gap-1 routines must end at or below this.
pub const SCUS_GAP_END_VA: u32 = 0x8007_7828;

// "Zero is not dead", part 3: the earlier arena3/4/5 (`0x8007075C`,
// `0x80079340`, `0x80079509`) were picked as plain zero-runs but turned out to
// be the **zero padding inside the live SsAPI sound/effect tables** in the
// `0x80079xxx` cluster - the item-use sound engine indexes a table at
// `0x800794F0` (read by `FUN_8005d0b8`) straight into the old arena5 bitmap, so
// using a Healing Leaf read our bytes as garbage table entries and the
// sound-synced item banner never dismissed (the Tetsu-tutorial Healing-Leaf
// freeze). A zero run is dead ONLY if no code references it; every region below
// is now **read-watch-verified unreferenced** on a live PCSX-Redux battle (item
// use, victory pose, AND a summon cast) - not merely all-zero.
//
/// Verified-dead arena 1: the high tail of the shared `0x8007AB38` rodata gap
/// (the same gap the bonus-drop / charm / flee-EXP code hooks use), above all of
/// them (flee-EXP ends `0x8007AE00`). The SsAPI sound I/O table begins exactly at
/// `0x8007AF00` (read every frame by `FUN_8006a7d0`/`8006b880`); read-watching
/// confirms `0x8007AE00..0x8007AF00` is unread, so the whole 256 bytes are
/// usable. Hosts the damage (D), grant (C2), grant-shift (K2), in-battle-menu
/// flag (H) and field-menu colour (F) routines.
pub const ARENA1_VA: u32 = 0x8007_AE00;
pub const ARENA1_END_VA: u32 = 0x8007_AF00;
/// Verified-dead arena 2: a dead pocket between two SsAPI tables (`0x8007AF40`
/// I/O table .. `0x8007B040` value table), read-watch-verified unread. Hosts the
/// +35% cast-banner routine (J). **4-byte aligned**: a `j` detour drops the
/// target's low 2 bits, so an unaligned entry would jump into garbage (the
/// earlier alignment freeze). Raw zero-run `0x8007AFF6` -> rounded to `0x8007AFF8`.
pub const ARENA2_VA: u32 = 0x8007_AFF8;
pub const ARENA2_END_VA: u32 = 0x8007_B040;
/// Verified-dead slot 6: a 69-byte padding gap between two `0x80078xxx` tables
/// (party-size .. new-game), read-watch-verified unread on item use, victory and
/// a live summon cast (its neighbours `0x80078870`/`0x80078d54` are hammered by
/// the sound funcs; this gap is silent). Hosts the summon-fade routine (K).
/// 4-byte aligned (zero-run start `0x80078A87` -> `0x80078A88`).
pub const SLOT6_VA: u32 = 0x8007_8A88;
pub const SLOT6_END_VA: u32 = 0x8007_8ACC;

// --- Menu overlay (PROT 0899) ----------------------------------------------

/// Base of the 0899 run every `--seru-trade` piece lives in, and past its
/// highest blob the menu half of `--show-super-arts`.
///
/// It is the blank lower band of the save-menu atlas TIM the overlay carries at
/// `0x801E5120` (pixels `0x801E5340..0x801ED340`, 256x256 4bpp, uploaded to VRAM
/// `(960, 0)` when the card screen opens): atlas rows 162..223, all-zero in the
/// file. Nothing samples those rows (a card-screen capture's draw list reads rows
/// 0..160 of that page, and 224..239 come from the save-icon sheet uploaded over
/// them), no instruction in any image forms an address in them, both save-screen
/// card buffers (`0x801E5120..0x801E9120`) end below them, and they read zero in
/// every library capture with the overlay resident. See
/// `legaia_disc_patch::space_ledger`, which also refuses any region overlapping a runtime
/// buffer.
///
/// The run once sat at `0x801E74E0`, inside the save compose buffer - which a
/// save from the pause menu fills and then returns to the root menu with the
/// overlay still resident.
pub const MENU_RUN_C_VA: u32 = 0x801E_A440;
/// Length of the run.
pub const MENU_RUN_C_LEN: u32 = 0xF00;
/// One past the last byte of the run.
pub const MENU_RUN_C_END_VA: u32 = MENU_RUN_C_VA + MENU_RUN_C_LEN;
// Every cell and string is reached through one shared `lui` of its page, so the
// whole run must sit inside one `%hi` page.
const _: () =
    assert!(crate::mips::hi(MENU_RUN_C_VA) == crate::mips::hi(MENU_RUN_C_VA + MENU_RUN_C_LEN - 1));
