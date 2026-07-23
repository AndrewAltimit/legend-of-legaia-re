//! Static layout: PROT indices, hook sites, runtime offsets/globals, the
//! Seru-name allowlist inputs, and the code-cave arena map (with the live-table
//! guard ranges). All addresses are pinned from the recognized US build.

/// PROT entry index of the battle-action overlay (0898).
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize =
    legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;
/// PROT entry index of the menu overlay (0899) hosting the spell-list renderer.
pub const MENU_OVERLAY_PROT_INDEX: usize = 899;
/// Load base VA shared by the slot-A overlays (0897 / 0898 / 0899). A VA inside
/// one maps to PROT-entry file offset `va - OVERLAY_BASE_VA`.
pub const OVERLAY_BASE_VA: u32 = legaia_asset::move_power::BATTLE_OVERLAY_BASE;

/// Default per-battle probability (percent) of a shiny capturable enemy.
pub const DEFAULT_PCT: u8 = 2;
/// Damage / stat bonus a shiny Seru grants (percent). Mirrors
/// `legaia_engine_core::seru_learning::SHINY_DAMAGE_BONUS_PCT`.
pub const SHINY_BONUS_PCT: u32 = 35;

// --- Hook sites (VA, expected first word). Each is detoured with [j, nop]; the
//     routine replays the two displaced words and returns to hook+8. ----------

/// Setup hook (SCUS, after the monster-setup loop in `FUN_800513F0`).
pub const HOOK_SETUP_VA: u32 = 0x8005_1A20;
pub(crate) const HOOK_SETUP_W0: u32 = 0x3C02_8008; // lui v0,0x8008
/// Capture-success hook (overlay 0898, `FUN_801ec3e4`): the captured enemy actor
/// (`v1`) is live here, so its shiny marker can be stashed.
pub const HOOK_CAPTURE_VA: u32 = 0x801E_E2E8;
pub(crate) const HOOK_CAPTURE_W0: u32 = 0xA082_0269; // sb v0,0x269(a0)
/// Grant hook (overlay 0898, `FUN_801E92DC`): the spell-level byte is written
/// `=1`; the routine ORs `0x80` when the captured enemy was shiny.
pub const HOOK_GRANT_VA: u32 = 0x801E_93B4;
pub(crate) const HOOK_GRANT_W0: u32 = 0xA043_0729; // sb v1,0x729(v0)
/// Grant shift hook (overlay 0898, `FUN_801e92dc`): just before the insert-at-
/// front shift loop, `v0` = the caster's record base. The shift-hook (K2) mirrors
/// the level-array shift onto the parallel shiny-byte array.
pub const HOOK_GRANT_SHIFT_VA: u32 = 0x801E_9320;
pub(crate) const HOOK_GRANT_SHIFT_W0: u32 = 0x9046_0704; // lbu a2,0x704(v0)
/// Damage hook (overlay 0898, `FUN_801dd864`): the spell level is read into the
/// summon-damage scaler; `v0` = the matched spell's slot base.
pub const HOOK_DAMAGE_VA: u32 = 0x801D_DB08;
pub(crate) const HOOK_DAMAGE_W0: u32 = 0x9042_0729; // lbu v0,0x729(v0)
/// Menu spell-list level-digit read (overlay 0899, `FUN_801d2e74`).
pub const HOOK_MENU_VA: u32 = 0x801D_2FA0;
pub(crate) const HOOK_MENU_W0: u32 = 0x8C63_46B0; // lw v1,0x46b0(v1)

// --- Runtime offsets / globals ---------------------------------------------

/// Battle-actor pointer table slot 3 (frontmost enemy) VA.
pub(crate) const ACTOR_SLOT3_VA: u32 = 0x801C_937C;
/// Per-actor **fade / translucency level** (zero-init each battle). The draw
/// helper `FUN_8004A908` (`0x8004AD0C`), when this byte is nonzero, modulates the
/// actor's draw colour `× (128 - fade) / 128` and renders it with the
/// semi-transparent primitive (high byte `0x81`). The shiny feature sets it to
/// `1`, which makes the shiny enemy render **see-through** - the intended shiny
/// visual tell - AND serves as the capture-link marker (C1 reads it). (The
/// scout that picked this offset called it "free"; it is not - it is the fade
/// field, and the translucency is a deliberate, game-native effect, not a side
/// effect.)
pub(crate) const ACTOR_SHINY_OFF: u16 = 0x226;
/// Per-spell-slot **shiny byte** offset, parallel to the level array. The shiny
/// flag lives here (`0x80` = shiny) instead of in the level byte's high bit, so
/// no level reader (the spell-level-up + display fn `FUN_800402f4`, the Lv menus)
/// ever sees it - eliminating the blank-level-up-box / corrupted-mouth / "Lv 129"
/// leaks. `0x788 = LEVEL_OFF + (0x1C0 - 0x161)`: a 32-byte run at record `+0x1C0`,
/// verified all-zero / unused across 228 record samples and inside the saved
/// record footprint. The grant shift-hook (K2) keeps it in sync with the level
/// array on spell insert; reads are slot-indexed off the same base as the level.
pub(crate) const SHINY_BYTE_OFF: u16 = 0x788;
/// First boosted stat halfword (HP base) ...
pub(crate) const STAT_FIRST_OFF: u16 = 0x14C;
/// ... and one past the last (AGL current is `0x16A`, loop end is exclusive).
pub(crate) const STAT_END_OFF: u16 = 0x16C;
/// BIOS `rand` thunk (returns `v0`).
pub(crate) const RAND_FUNC_VA: u32 = 0x8005_6798;
/// Shiny high-bit flag in the level byte.
pub(crate) const SHINY_FLAG: u16 = 0x80;
/// First-monster id global (`DAT_8007BD0C`), set before the setup hook and
/// indexed into the capturable bitmap (1-based, matches the `monster-stats` id).
pub(crate) const FIRST_MONSTER_ID_VA: u32 = 0x8007_BD0C;

/// The 11 player Seru-magic names (spell ids `0x81..=0x8b`). A monster whose
/// name matches one of these (or a `"<name> $N"` / `"<name> ..."` variant) is a
/// capturable Seru - the population the shiny allowlist bitmap is built from.
pub const SERU_NAMES: [&str; 11] = [
    "Gimard", "Theeder", "Vera", "Gizam", "Nighto", "Zenoir", "Viguro", "Swordie", "Orb", "Freed",
    "Nova",
];
/// Capturable-allowlist bitmap size: 256 bits so any `u8` monster id indexes in
/// bounds without a runtime range check.
pub(crate) const BITMAP_BYTES: usize = 32;

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
/// the `legaia_asset::face_anim` / `item_names` table addresses, and the SsAPI
/// sound/effect tables found by read-watching a live battle (their zero padding
/// is what the old arena3/4/5 wrongly squatted in - the Healing-Leaf freeze).
pub(crate) const SCUS_TABLE_RANGES: &[(u32, u32)] = &[
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
pub(crate) const OVERLAY_TABLE_RANGES: &[(u32, u32)] = &[(0x801F_4E63, 0x801F_69D8)];

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

// Per-routine VAs carved from the arenas above (assigned in `plan`). The public
// consts the tests pin point at these arena addresses. Every ROUTINE VA must be
// 4-byte aligned (it is a `j` target); data VAs need not be.
/// Summon-fade routine (K) VA - the verified-dead slot 6 (its own pocket; K is
/// 56 bytes, slot 6 is 68).
pub const SUMMON_FADE_RUN_VA: u32 = SLOT6_VA;
/// Grant-shift routine (K2) VA (arena 1, after D + C2).
pub const SHIFT_RUN_VA: u32 = 0x8007_AE6C;
/// Field-menu colour routine (F) VA (arena 1, after K2 + H). SCUS-resident, so
/// the 0899 menu detour can jump to it.
pub const MENU_RUN_VA: u32 = 0x8007_AED0;
/// +35% cast-banner routine (J) VA (arena 2).
pub const BANNER_RUN_VA: u32 = ARENA2_VA;
/// The "+35% DMG!" display string - data in gap 1, after the bitmap + flag.
pub(crate) const BANNER_STR_VA: u32 = SHINY_CAST_FLAG_VA + 1;
/// One-byte "current cast is shiny" flag the in-battle menu stamper (H) writes
/// (bit 0x80 = shiny); the +35% banner (J) and summon-fade (K) read it. In gap 1,
/// after the 32-byte capturable bitmap.
pub const SHINY_CAST_FLAG_VA: u32 = SCUS_GAP_VA + 4 + 44 * 4 + 7 * 4 + BITMAP_BYTES as u32;
/// Text-colour global `_DAT_8007b454`: the menu writes a CLUT index here before
/// each glyph draw (`6` = the normal name/digit colour).
pub(crate) const TEXT_COLOR_GLOBAL_VA: u32 = 0x8007_B454;
/// CLUT colour index used for a shiny Seru's menu level digit (distinct from the
/// normal `6`). Picked from the documented in-game indices (`9` = red).
pub(crate) const SHINY_MENU_COLOR: u16 = 9;

// --- In-battle magic-menu level display (overlay 0898 `FUN_801d0748`) -------

/// The in-battle magic menu reads the selected spell's level byte here and
/// stores it into the menu struct (`sb v1,0x15(s1)`) for the "Lv NN" header,
/// WITHOUT masking - so the shiny bit leaks and shows as "Lv 129". (Distinct
/// from the field menu's `HOOK_MENU_VA` in 0899, which F already masks.)
pub const HOOK_BMENU_LVL_VA: u32 = 0x801D_1B00;
/// `lbu v1,0x729(v0)` - the displaced word the masker replays.
pub(crate) const HOOK_BMENU_LVL_W0: u32 = 0x9043_0729;
/// In-battle menu shiny-flag stamper (H) VA (arena 1, after K2).
pub const BMENU_RUN_VA: u32 = 0x8007_AEB0;

// --- Summon transparency (SCUS `FUN_8004a908`, draw-time fade modulator) ----

/// The sole draw-time reader of an actor's fade byte `+0x226`
/// (`lbu v0,0x226(s1)` inside `FUN_8004a908`; `s1` = the actor being drawn).
/// The summon creature's `+0x226` is rebuilt to 0 every frame (a per-frame
/// struct rebuild no write-watch can trap), so pre-writing the byte can never
/// fade it. Instead the detour overrides the value AT THE READ: if the cast is
/// shiny (`SHINY_CAST_FLAG`) and `s1` is the summon actor, force `v0` to
/// `SUMMON_FADE`. Live-validated on a real Gimard cast.
pub const HOOK_FADE_VA: u32 = 0x8004_AD0C;
/// `lbu v0,0x226(s1)` - the displaced word the routine replays.
pub(crate) const HOOK_FADE_W0: u32 = 0x9222_0226;
/// Where the function continues (the `beq v0,zero` after the fade read); the
/// routine returns here (= hook + 8).
pub(crate) const HOOK_FADE_RET_VA: u32 = 0x8004_AD14;
/// Battle actor-pointer table slot 7 (`0x801C9370 + 7*4`). The 8-slot battle
/// actor array (`0x800EC9E8 + i*0x2D4`) is a fixed layout - party 0..2, enemies
/// 3..6, summon = slot 7 (the dedicated summon slot, confirmed across battles).
pub(crate) const SUMMON_ACTOR_SLOT_VA: u32 = 0x801C_938C;
/// Fade strength for the summon. `FUN_8004a908` scales colour by
/// `(0x80-fade)/0x80` then STP-blends 50/50 with the background, so a *higher*
/// fade reads as *more* transparent: `0x40` -> creature contributes ~25% of its
/// colour, `0x60` -> ~12.5% (clearly translucent over the dark battle floor).
pub(crate) const SUMMON_FADE: u16 = 0x60;
// (The summon-fade routine K lives at `SUMMON_FADE_RUN_VA` = the verified-dead
// slot 6, declared with the other routine VAs above.)

// --- +35% cast text (SCUS `FUN_80031d00` battle text-widget renderer) --------

/// The cast spell-name banner is drawn by the battle HUD text-widget loop in
/// `FUN_80031d00`: `lw a0,0x18(s4)` loads the widget's string ptr just before
/// `jal FUN_80036888` (the glyph renderer). The detour replays the load + the
/// following `li v0,7`, then - if the cast is shiny (`SHINY_CAST_FLAG`) and this
/// is the move-name banner widget (`a1 == 0x801C`, the move-banner style; the
/// caster/target NAME widgets drawn by the same call use `a1 == 0`) - redirects
/// `a0` at a custom "+35% DMG!" string. (The earlier approach of overriding the
/// `0x80077344` banner globals at the SM spawn was a no-op; those globals don't
/// drive the visible banner.)
pub const HOOK_BANNER_VA: u32 = 0x8003_21D4;
pub(crate) const HOOK_BANNER_W0: u32 = 0x8E84_0018; // lw a0,0x18(s4)
/// Where the routine returns (the `sw v0,0x13c(gp)` after the a0 load = hook + 8).
pub(crate) const HOOK_BANNER_RET_VA: u32 = 0x8003_21DC;
/// The move-banner widget's style halfword (`s4+0x12`, passed as `a1`). Name
/// widgets use `0`, so this distinguishes the spell banner from the HUD names.
pub(crate) const BANNER_STYLE_TAG: u16 = 0x801C;
/// Y screen coordinate for the relocated "+35% DMG!" text: the renderer reads
/// the line's Y from the 5th arg at `0x10(sp)` (`FUN_80036888`'s `lw s6,0x50(sp)`;
/// the spell banner's native Y is ~150 = mid-screen). The native "Magic effect:"
/// announcement box occupies screen rows ~12..28 at the top, so the +35% text is
/// dropped to `0x1E` (= 30) - one glyph line **below** that box - to avoid the
/// overlap that occurred at the old `0x0A`. Live-validated on a real cast (the
/// effect box and the +35% line stack cleanly). The detour overwrites that stack
/// slot (it keeps the caller's sp).
pub(crate) const BANNER_TOP_Y: u16 = 0x1E;

/// Multiplier numerator for the +35% boost (135; the boost is `× BONUS / 100`).
pub(crate) const BONUS: u16 = (100 + SHINY_BONUS_PCT) as u16; // 135
