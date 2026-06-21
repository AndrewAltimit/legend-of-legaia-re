//! Shiny Seru feature: a rare (default 2%/battle) **capturable** enemy that
//! spawns with +35% stats, and whose captured Seru deals +35% damage forever.
//!
//! Mirrors the clean-room engine implementation in
//! `legaia_engine_core::seru_learning` (the `shiny` set + `SHINY_DAMAGE_BONUS_PCT`)
//! as a retail disc patch, built from the same `enemy_ally`-style code injection.
//!
//! ## What "shiny" means on retail
//!
//! 1. **Battle (B).** At battle setup, with probability `pct`, **if the frontmost
//!    enemy is a capturable Seru**, its combat stats are multiplied by 135/100
//!    and its per-actor **fade level** `+0x226` is set to `1`. That byte is the
//!    game's own translucency field (the draw helper `FUN_8004A908` renders the
//!    actor semi-transparent when it's nonzero), so a shiny enemy renders
//!    **see-through** in battle - a free, game-native visual tell - and the same
//!    byte doubles as the capture-link marker. The capturable
//!    check indexes a 256-bit allowlist bitmap by the first-monster id global
//!    (`DAT_8007BD0C`, reliably set before this hook - the game's own `0xB5`
//!    check reads it). The bitmap is built at patch time from the disc's monster
//!    names (every monster whose name matches a player Seru-magic name; see
//!    [`capturable_monster_ids`]) - NOT the volatile `actor+0x3e` byte, which
//!    the earlier RE mis-identified (it reads non-Seru values like 0x55 for
//!    gobu). So gobu and other non-Seru enemies are never shiny.
//! 2. **Capture (C1 + C2).** When a Seru is captured, the spell is granted into
//!    the character record at level 1 (`record+0x161 = 1`). If the captured enemy
//!    was shiny (its `+0x226` marker, stashed at capture-success into a scratch
//!    word), the grant ORs the free high bit `0x80` into that level byte - a
//!    persistent "shiny" flag that rides along through the spell-list insertion
//!    shift and survives a memory-card save (max legit level is 9, so `0x80` is
//!    free).
//! 3. **Damage (D).** When the spell is cast, the summon-damage scaler
//!    (`FUN_801dd864`) reads that level byte; the hook multiplies the running
//!    damage by 135/100 when `0x80` is set, then strips the bit for the normal
//!    `(level-1)/8` math.
//! 4. **Level-up (G1/G2/G3) + menu (F).** Three readers of the level byte are
//!    masked so the `0x80` flag is transparent: the level-up gate / read
//!    (`FUN_801E70BC`) sees the real level (so a shiny Seru still levels up, and
//!    the increment re-preserves `0x80`), and the menu spell-list renderer
//!    (`FUN_801d2e74`, overlay 0899) displays the real digit.
//!
//! ## Where the code lives
//!
//! Every routine is reached by an `enemy_ally`-style two-word detour (replace the
//! hook instruction + its successor with `j routine` + `nop`; the routine replays
//! both displaced words and returns to `hook+8`). Routines live in two preserved
//! reference-free zero regions, both resident when their hooks fire:
//!
//! - **A new `SCUS_942.54` rodata gap** at `0x80077728` (the padding before the
//!   steal table `DAT_80077828`; reference-free, distinct from the
//!   `0x8007AB38` gap the other gap features fill, so this composes with all of
//!   them). Hosts the scratch word + the setup routine (B), the capture-copy (C1),
//!   the level-up gate mask (G1), and the menu mask (F).
//! - **The battle-action overlay 0898 move-power-table padding** at `0x801F4FC4`
//!   (128 bytes, reference-free). Hosts the damage routine (D) and grant-OR (C2).
//! - **A second `SCUS_942.54` rodata gap** at `0x800783C4` (reference-free
//!   padding). Hosts the capturable allowlist bitmap (32 bytes) + the two
//!   level-up read-mask routines (G2/G3) - the routines that didn't fit the
//!   first gap once the bitmap + capturable check were added.
//!
//! All edits are guarded: each hook's instruction word must match the recognized
//! US build, and each routine region must be all-zero dead space. A
//! differently-laid-out image is refused, not corrupted. No Sony bytes are
//! embedded; the routines are the randomizer's own code.

use anyhow::{Result, bail};

use legaia_asset::item_names;

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
const HOOK_SETUP_W0: u32 = 0x3C02_8008; // lui v0,0x8008
/// Capture-success hook (overlay 0898, `FUN_801ec3e4`): the captured enemy actor
/// (`v1`) is live here, so its shiny marker can be stashed.
pub const HOOK_CAPTURE_VA: u32 = 0x801E_E2E8;
const HOOK_CAPTURE_W0: u32 = 0xA082_0269; // sb v0,0x269(a0)
/// Grant hook (overlay 0898, `FUN_801E92DC`): the spell-level byte is written
/// `=1`; the routine ORs `0x80` when the captured enemy was shiny.
pub const HOOK_GRANT_VA: u32 = 0x801E_93B4;
const HOOK_GRANT_W0: u32 = 0xA043_0729; // sb v1,0x729(v0)
/// Grant shift hook (overlay 0898, `FUN_801e92dc`): just before the insert-at-
/// front shift loop, `v0` = the caster's record base. The shift-hook (K2) mirrors
/// the level-array shift onto the parallel shiny-byte array.
pub const HOOK_GRANT_SHIFT_VA: u32 = 0x801E_9320;
const HOOK_GRANT_SHIFT_W0: u32 = 0x9046_0704; // lbu a2,0x704(v0)
/// Damage hook (overlay 0898, `FUN_801dd864`): the spell level is read into the
/// summon-damage scaler; `v0` = the matched spell's slot base.
pub const HOOK_DAMAGE_VA: u32 = 0x801D_DB08;
const HOOK_DAMAGE_W0: u32 = 0x9042_0729; // lbu v0,0x729(v0)
/// Menu spell-list level-digit read (overlay 0899, `FUN_801d2e74`).
pub const HOOK_MENU_VA: u32 = 0x801D_2FA0;
const HOOK_MENU_W0: u32 = 0x8C63_46B0; // lw v1,0x46b0(v1)

// --- Runtime offsets / globals ---------------------------------------------

/// Battle-actor pointer table slot 3 (frontmost enemy) VA.
const ACTOR_SLOT3_VA: u32 = 0x801C_937C;
/// Per-actor **fade / translucency level** (zero-init each battle). The draw
/// helper `FUN_8004A908` (`0x8004AD0C`), when this byte is nonzero, modulates the
/// actor's draw colour `× (128 - fade) / 128` and renders it with the
/// semi-transparent primitive (high byte `0x81`). The shiny feature sets it to
/// `1`, which makes the shiny enemy render **see-through** - the intended shiny
/// visual tell - AND serves as the capture-link marker (C1 reads it). (The
/// scout that picked this offset called it "free"; it is not - it is the fade
/// field, and the translucency is a deliberate, game-native effect, not a side
/// effect.)
const ACTOR_SHINY_OFF: u16 = 0x226;
/// Per-spell-slot **shiny byte** offset, parallel to the level array. The shiny
/// flag lives here (`0x80` = shiny) instead of in the level byte's high bit, so
/// no level reader (the spell-level-up + display fn `FUN_800402f4`, the Lv menus)
/// ever sees it - eliminating the blank-level-up-box / corrupted-mouth / "Lv 129"
/// leaks. `0x788 = LEVEL_OFF + (0x1C0 - 0x161)`: a 32-byte run at record `+0x1C0`,
/// verified all-zero / unused across 228 record samples and inside the saved
/// record footprint. The grant shift-hook (K2) keeps it in sync with the level
/// array on spell insert; reads are slot-indexed off the same base as the level.
const SHINY_BYTE_OFF: u16 = 0x788;
/// First boosted stat halfword (HP base) ...
const STAT_FIRST_OFF: u16 = 0x14C;
/// ... and one past the last (AGL current is `0x16A`, loop end is exclusive).
const STAT_END_OFF: u16 = 0x16C;
/// BIOS `rand` thunk (returns `v0`).
const RAND_FUNC_VA: u32 = 0x8005_6798;
/// Shiny high-bit flag in the level byte.
const SHINY_FLAG: u16 = 0x80;
/// First-monster id global (`DAT_8007BD0C`), set before the setup hook and
/// indexed into the capturable bitmap (1-based, matches the `monster-stats` id).
const FIRST_MONSTER_ID_VA: u32 = 0x8007_BD0C;

/// The 11 player Seru-magic names (spell ids `0x81..=0x8b`). A monster whose
/// name matches one of these (or a `"<name> $N"` / `"<name> ..."` variant) is a
/// capturable Seru - the population the shiny allowlist bitmap is built from.
pub const SERU_NAMES: [&str; 11] = [
    "Gimard", "Theeder", "Vera", "Gizam", "Nighto", "Zenoir", "Viguro", "Swordie", "Orb", "Freed",
    "Nova",
];
/// Capturable-allowlist bitmap size: 256 bits so any `u8` monster id indexes in
/// bounds without a runtime range check.
const BITMAP_BYTES: usize = 32;

// --- Code-cave layout ------------------------------------------------------

/// New SCUS rodata gap base (word-aligned; padding before the steal table).
pub const SCUS_GAP_VA: u32 = 0x8007_7728;
/// First VA used by the steal table; SCUS routines must end at or below this.
pub const SCUS_GAP_END_VA: u32 = 0x8007_7828;
/// 0898 move-power-table padding cave base.
pub const CAVE_VA: u32 = 0x801F_4FC4;
/// First VA past the 0898 cave; cave routines must end at or below this.
pub const CAVE_END_VA: u32 = 0x801F_5044;
/// Second SCUS rodata gap (reference-free padding) hosting the capturable bitmap.
/// (Formerly also held the level-up read-mask routines G1/G2/G3, now removed -
/// the level byte is clean so no level masking is needed.)
pub const SCUS_GAP2_VA: u32 = 0x8007_83C4;
/// First VA past the second SCUS gap; its contents must end at or below this.
pub const SCUS_GAP2_END_VA: u32 = 0x8007_8420;
/// Reference-free 87-byte SCUS run hosting the grant shift routine (K2). (Same
/// proven-safe steal-table-adjacent class as the banner / summon-fade runs.)
pub const SHIFT_RUN_VA: u32 = 0x8007_82DC;
/// First VA past the shift run; K2 must end at or below this.
pub const SHIFT_RUN_END_VA: u32 = 0x8007_8330;
/// Small reference-free SCUS run (45-byte rodata gap, same proven-safe class as
/// gap 2) hosting the 11-word colour-aware menu routine (F). Sized to fit F
/// exactly so the three 87-byte runs stay free for the summon routine.
pub const MENU_RUN_VA: u32 = 0x8007_82A4;
/// First VA past the menu run; F must end at or below this.
pub const MENU_RUN_END_VA: u32 = 0x8007_82D0;
/// Reference-free 87-byte SCUS run hosting the +35% cast-banner routine (J) AND
/// its display string (the string sits right after the routine in the same run).
/// (Two more such runs at 0x8007821C / 0x800782DC stay reserved-dead - they held
/// a reverted summon-transparency attempt; a correct version needs a summon-spawn
/// hook that isn't located yet.)
pub const BANNER_RUN_VA: u32 = 0x8007_81BC;
pub const BANNER_RUN_END_VA: u32 = 0x8007_8210;
/// The "+35% DMG!" display string lives at routine-end within the banner run.
const BANNER_STR_VA: u32 = 0x8007_8200;
/// One-byte "current cast is shiny" flag the in-battle menu masker (H) stamps
/// with the selected spell's level byte (bit 0x80 = shiny); the +35% banner (J)
/// reads it. Lives at the tail of the menu-masker run.
pub const SHINY_CAST_FLAG_VA: u32 = 0x8007_8358;
/// Text-colour global `_DAT_8007b454`: the menu writes a CLUT index here before
/// each glyph draw (`6` = the normal name/digit colour).
const TEXT_COLOR_GLOBAL_VA: u32 = 0x8007_B454;
/// CLUT colour index used for a shiny Seru's menu level digit (distinct from the
/// normal `6`). Picked from the documented in-game indices (`9` = red).
const SHINY_MENU_COLOR: u16 = 9;

// --- In-battle magic-menu level display (overlay 0898 `FUN_801d0748`) -------

/// The in-battle magic menu reads the selected spell's level byte here and
/// stores it into the menu struct (`sb v1,0x15(s1)`) for the "Lv NN" header,
/// WITHOUT masking - so the shiny bit leaks and shows as "Lv 129". (Distinct
/// from the field menu's `HOOK_MENU_VA` in 0899, which F already masks.)
pub const HOOK_BMENU_LVL_VA: u32 = 0x801D_1B00;
/// `lbu v1,0x729(v0)` - the displaced word the masker replays.
const HOOK_BMENU_LVL_W0: u32 = 0x9043_0729;
/// Small reference-free SCUS run (36-byte rodata gap) hosting the 5-word masker.
pub const BMENU_RUN_VA: u32 = 0x8007_833C;
/// First VA past the battle-menu masker run.
pub const BMENU_RUN_END_VA: u32 = 0x8007_8360;

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
const HOOK_FADE_W0: u32 = 0x9222_0226;
/// Where the function continues (the `beq v0,zero` after the fade read); the
/// routine returns here (= hook + 8).
const HOOK_FADE_RET_VA: u32 = 0x8004_AD14;
/// Battle actor-pointer table slot 7 (`0x801C9370 + 7*4`). The 8-slot battle
/// actor array (`0x800EC9E8 + i*0x2D4`) is a fixed layout - party 0..2, enemies
/// 3..6, summon = slot 7 (the dedicated summon slot, confirmed across battles).
const SUMMON_ACTOR_SLOT_VA: u32 = 0x801C_938C;
/// Fade strength for the summon. `FUN_8004a908` scales colour by
/// `(0x80-fade)/0x80` then STP-blends 50/50 with the background, so a *higher*
/// fade reads as *more* transparent: `0x40` -> creature contributes ~25% of its
/// colour, `0x60` -> ~12.5% (clearly translucent over the dark battle floor).
const SUMMON_FADE: u16 = 0x60;
/// Reference-free SCUS run (steal-table-adjacent rodata padding) for the
/// summon-fade routine (K).
pub const SUMMON_FADE_RUN_VA: u32 = 0x8007_821C;
/// First VA past the summon-fade run.
pub const SUMMON_FADE_RUN_END_VA: u32 = 0x8007_8270;

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
const HOOK_BANNER_W0: u32 = 0x8E84_0018; // lw a0,0x18(s4)
/// Where the routine returns (the `sw v0,0x13c(gp)` after the a0 load = hook + 8).
const HOOK_BANNER_RET_VA: u32 = 0x8003_21DC;
/// The move-banner widget's style halfword (`s4+0x12`, passed as `a1`). Name
/// widgets use `0`, so this distinguishes the spell banner from the HUD names.
const BANNER_STYLE_TAG: u16 = 0x801C;
/// Y screen coordinate for the relocated "+35% DMG!" text: the renderer reads
/// the line's Y from the 5th arg at `0x10(sp)` (`FUN_80036888`'s `lw s6,0x50(sp)`;
/// the spell banner's native Y is ~150 = mid-screen). The empty top HUD box has a
/// blue interior spanning screen rows ~8..23 (measured from VRAM), so `0x0A`
/// centres the 12px glyph line inside it. The detour overwrites that stack slot
/// (it keeps the caller's sp).
const BANNER_TOP_Y: u16 = 0x0A;

// --- MIPS R3000 encoders (little-endian) -----------------------------------

const ZERO: u32 = 0;
const AT: u32 = 1; // assembler temp - safe to clobber (never held live)
const V0: u32 = 2;
const V1: u32 = 3;
const A0: u32 = 4;
const A1: u32 = 5;
const A2: u32 = 6;
const T0: u32 = 8;
const T1: u32 = 9;
const T2: u32 = 10;
const T3: u32 = 11;
const T4: u32 = 12;
const T5: u32 = 13;
const T6: u32 = 14;
const T7: u32 = 15;
const S1: u32 = 17; // live actor pointer in FUN_8004a908 (compared, never written)
const T8: u32 = 24;
const SP: u32 = 29; // stack pointer (the banner detour keeps the caller's sp)
const T9: u32 = 25;

const fn j(t: u32) -> u32 {
    (0x02 << 26) | ((t >> 2) & 0x03ff_ffff)
}
const fn jal(t: u32) -> u32 {
    (0x03 << 26) | ((t >> 2) & 0x03ff_ffff)
}
const fn nop() -> u32 {
    0
}
const fn lui(rt: u32, imm: u16) -> u32 {
    (0x0f << 26) | (rt << 16) | imm as u32
}
const fn ori(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0d << 26) | (rs << 21) | (rt << 16) | imm as u32
}
const fn andi(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0c << 26) | (rs << 21) | (rt << 16) | imm as u32
}
const fn addiu(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x09 << 26) | (rs << 21) | (rt << 16) | imm as u32
}
const fn lhu(rt: u32, rs: u32, off: u16) -> u32 {
    (0x25 << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn lw(rt: u32, rs: u32, off: u16) -> u32 {
    (0x23 << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn lbu(rt: u32, rs: u32, off: u16) -> u32 {
    (0x24 << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn sh(rt: u32, rs: u32, off: u16) -> u32 {
    (0x29 << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn sb(rt: u32, rs: u32, off: u16) -> u32 {
    (0x28 << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn sw(rt: u32, rs: u32, off: u16) -> u32 {
    (0x2b << 26) | (rs << 21) | (rt << 16) | off as u32
}
const fn sltiu(rt: u32, rs: u32, imm: u16) -> u32 {
    (0x0b << 26) | (rs << 21) | (rt << 16) | imm as u32
}
const fn beq(rs: u32, rt: u32, off: i16) -> u32 {
    (0x04 << 26) | (rs << 21) | (rt << 16) | (off as u16 as u32)
}
const fn bne(rs: u32, rt: u32, off: i16) -> u32 {
    (0x05 << 26) | (rs << 21) | (rt << 16) | (off as u16 as u32)
}
const fn addu(rd: u32, rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x21
}
const fn srl(rd: u32, rt: u32, sa: u32) -> u32 {
    (rt << 16) | (rd << 11) | (sa << 6) | 0x02
}
const fn srlv(rd: u32, rt: u32, rs: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x06
}
const fn multu(rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | 0x19
}
const fn divu(rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | 0x1b
}
const fn mflo(rd: u32) -> u32 {
    (rd << 11) | 0x12
}
const fn mfhi(rd: u32) -> u32 {
    (rd << 11) | 0x10
}
const fn lo(va: u32) -> u16 {
    (va & 0xffff) as u16
}
const fn hi(va: u32) -> u16 {
    (va.wrapping_add(0x8000) >> 16) as u16
}

const BONUS: u16 = (100 + SHINY_BONUS_PCT) as u16; // 135

// --- Routine assemblers. Each takes the two displaced words it must replay
//     (read from the image at plan time) plus its return VA. -----------------

/// (B) Setup: roll `pct`%; on a hit, **if the frontmost enemy is a capturable
/// Seru** (its monster id `DAT_8007BD0C` is set in the allowlist bitmap at
/// `bitmap_va`), boost its stats ×135/100 and mark it shiny (`actor+0x226`).
///
/// The capturable check uses the **first-monster id** global (reliably set
/// before this hook - the game's own `0xB5` check at `FUN_800513F0` `0x80051998`
/// reads it) indexed into a 256-bit allowlist bitmap, NOT the volatile
/// `actor+0x3e` byte (which the earlier RE mis-identified - it reads non-Seru
/// values like 0x55 for gobu and isn't a capturable flag). The bitmap is built
/// at patch time from the disc's monster names (see [`capturable_monster_ids`]).
/// Capturable scoping for the persistent damage bonus is *additionally* enforced
/// at capture time (C1/C2 only flag a Seru that is actually captured).
fn assemble_setup(pct: u8, bitmap_va: u32, disp: [u32; 2], ret: u32) -> Vec<u32> {
    const REPLAY: i32 = 40;
    const LOOP: i32 = 28;
    let w = vec![
        jal(RAND_FUNC_VA),                   // 0
        nop(),                               // 1
        addiu(T0, ZERO, 100),                // 2
        divu(V0, T0),                        // 3
        mfhi(T3),                            // 4  t3 = rand%100
        sltiu(T3, T3, pct as u16),           // 5
        beq(T3, ZERO, (REPLAY - 7) as i16),  // 6  miss -> replay
        nop(),                               // 7
        lui(V0, hi(ACTOR_SLOT3_VA)),         // 8
        lw(T2, V0, lo(ACTOR_SLOT3_VA)),      // 9  t2 = frontmost enemy actor
        nop(),                               // 10 load delay
        beq(T2, ZERO, (REPLAY - 12) as i16), // 11 no enemy -> replay
        nop(),                               // 12
        // --- capturable check: bitmap[first_monster_id] bit set? ---
        lui(V0, hi(FIRST_MONSTER_ID_VA)),     // 13
        lbu(T0, V0, lo(FIRST_MONSTER_ID_VA)), // 14 t0 = first monster id
        lui(T3, hi(bitmap_va)),               // 15 LOAD-DELAY SLOT (doesn't use t0)
        srl(T1, T0, 3),                       // 16 t1 = id >> 3 (byte index)
        addu(T3, T3, T1),                     // 17 t3 = hi(bitmap) + byte index
        lbu(T3, T3, lo(bitmap_va)),           // 18 t3 = bitmap[byte]
        andi(T0, T0, 7),                      // 19 LOAD-DELAY SLOT (uses t0, not t3)
        srlv(T3, T3, T0),                     // 20 t3 >>= (id & 7)
        andi(T3, T3, 1),                      // 21
        beq(T3, ZERO, (REPLAY - 23) as i16),  // 22 not capturable -> replay
        nop(),                                // 23
        // --- boost loop ---
        addiu(T4, ZERO, STAT_FIRST_OFF), // 24 off = 0x14C
        addiu(T5, ZERO, STAT_END_OFF),   // 25 end = 0x16C
        addiu(T6, ZERO, BONUS),          // 26 135
        addiu(T7, ZERO, 100),            // 27 100
        addu(T8, T2, T4),                // 28 LOOP: t8 = actor+off
        lhu(T9, T8, 0),                  // 29 t9 = stat
        addiu(T4, T4, 2),                // 30 LOAD-DELAY SLOT: advance offset
        multu(T9, T6),                   // 31 t9 valid now
        mflo(T9),                        // 32
        divu(T9, T7),                    // 33
        mflo(T9),                        // 34
        sh(T9, T8, 0),                   // 35
        bne(T4, T5, (LOOP - 37) as i16), // 36 -> LOOP
        nop(),                           // 37 branch-delay slot
        addiu(T9, ZERO, 1),              // 38
        sb(T9, T2, ACTOR_SHINY_OFF),     // 39 mark shiny
        disp[0],                         // 40 REPLAY
        disp[1],                         // 41
        j(ret),                          // 42
        nop(),                           // 43
    ];
    debug_assert_eq!(w.len() as i32, REPLAY + 4);
    w
}

/// Decode the `battle_data` monster archive and return the 1-based monster ids
/// (matching the runtime `DAT_8007BD0C` id) of every capturable Seru - a monster
/// whose name matches a player Seru-magic name in [`SERU_NAMES`] (including
/// `"<name> $N"` variants). Drives the shiny allowlist bitmap.
pub fn capturable_monster_ids(archive: &[u8]) -> Result<Vec<u16>> {
    let recs = legaia_asset::monster_archive::records(archive)
        .map_err(|e| anyhow::anyhow!("decode monster archive: {e}"))?;
    let mut ids = Vec::new();
    for (i, r) in recs.iter().enumerate() {
        let nm = r.name.trim();
        let is_seru = SERU_NAMES.iter().any(|s| {
            nm == *s || nm.starts_with(&format!("{s} ")) || nm.starts_with(&format!("{s}$"))
        });
        if is_seru {
            ids.push((i + 1) as u16);
        }
    }
    Ok(ids)
}

/// Build the 256-bit ([`BITMAP_BYTES`]) capturable allowlist: bit `id` set for
/// each capturable monster id. Ids `>= 256` are ignored (never valid).
fn build_bitmap(ids: &[u16]) -> Vec<u8> {
    let mut bm = vec![0u8; BITMAP_BYTES];
    for &id in ids {
        if (id as usize) < BITMAP_BYTES * 8 {
            bm[id as usize >> 3] |= 1 << (id & 7);
        }
    }
    bm
}

/// (C1) Capture-success: stash the captured enemy's shiny marker (`+0x226`,
/// enemy actor in `v1`) into `scratch`, then replay. `a0`=caster, `v1`=enemy.
fn assemble_capture_copy(scratch_va: u32, disp: [u32; 2], ret: u32) -> Vec<u32> {
    vec![
        disp[0],                      // sb v0,0x269(a0)  (replay)
        lbu(T8, V1, ACTOR_SHINY_OFF), // t8 = enemy +0x226
        lui(T9, hi(scratch_va)),      // t9 = &scratch
        sb(T8, T9, lo(scratch_va)),   // scratch = t8
        disp[1],                      // lw v1,0(s3)  (replay; overwrites v1 - read above first)
        j(ret),
        nop(),
    ]
}

/// (C2) Grant: write the just-granted spell's level byte **clean** (no shiny bit)
/// and set its slot-0 **shiny byte** (`+0x788`) to `0x80` iff the captured enemy
/// was shiny (`scratch != 0`), else `0`. The new spell is always inserted at
/// slot 0, so the shiny byte lands at `+0x788(v0)`. `v0`=record base (preserved),
/// `v1`=1 (the level). Replays `sb v1,0x729(v0)` + `sw zero,0x5d0(v0)` unchanged
/// (the level stays clean - shininess no longer rides the level byte).
fn assemble_grant_shiny(scratch_va: u32, disp: [u32; 2], ret: u32) -> Vec<u32> {
    const STORE: i32 = 8; // index of the shiny-byte store (the `beq` skip target)
    vec![
        disp[0],                           // 0 sb v1,0x729(v0)  (clean level=1)
        disp[1],                           // 1 sw zero,0x5d0(v0)
        lui(T9, hi(scratch_va)),           // 2
        lbu(T8, T9, lo(scratch_va)),       // 3 t8 = scratch (enemy shiny marker)
        ori(T7, ZERO, 0),                  // 4 default shiny byte = 0 (fills load delay)
        beq(T8, ZERO, (STORE - 6) as i16), // 5 not shiny -> store 0
        nop(),                             // 6
        ori(T7, ZERO, SHINY_FLAG),         // 7 shiny -> 0x80
        sb(T7, V0, SHINY_BYTE_OFF),        // 8 STORE: shiny[slot 0] = t7
        j(ret),                            // 9
        nop(),                             // 10
    ]
}

/// (K2) Grant shift: when a new spell is inserted at slot 0 the game shifts the
/// id / level / xp arrays down by one (`FUN_801e92dc`); mirror that for the
/// parallel shiny-byte array so each spell keeps its shiny flag. Hooked just
/// before the game's shift loop (`lbu a2,0x704(v0)` at `0x801e9320`, `v0`=record
/// base), it shifts `shiny[i] = shiny[i-1]` for `i = count..1`, then replays the
/// count load so the game's loop proceeds. `v0`/`a0`/`v1` preserved.
fn assemble_grant_shift(disp: [u32; 2], ret: u32) -> Vec<u32> {
    const LOOP: i32 = 6;
    const END: i32 = 13;
    vec![
        lbu(T8, V0, 0x704),                // 0 t8 = spell count (load)
        nop(),                             // 1 load delay
        beq(T8, ZERO, (END - 3) as i16),   // 2 count==0 -> nothing to shift
        nop(),                             // 3
        addu(T9, V0, T8),                  // 4 t9 = base + count
        addiu(T9, T9, SHINY_BYTE_OFF),     // 5 t9 = &shiny[count] (dst cursor)
        lbu(AT, T9, 0xFFFF),               // 6 LOOP: at = shiny[i-1]
        nop(),                             // 7 load delay
        sb(AT, T9, 0),                     // 8 shiny[i] = at
        addiu(T9, T9, 0xFFFF),             // 9 cursor--
        addiu(T8, T8, 0xFFFF),             // 10 count--
        bne(T8, ZERO, (LOOP - 12) as i16), // 11 -> LOOP
        nop(),                             // 12
        disp[0],                           // 13 END: lbu a2,0x704(v0) (replay; a2=count)
        disp[1],                           // 14 nop (replay)
        j(ret),                            // 15
        nop(),                             // 16
    ]
}

/// (D) Damage: read the matched spell's **shiny byte** (`+0x788(v0)`, where `v0`
/// is the matched slot base) and, if shiny, multiply the running summon damage
/// (`*a2`) ×135/100. The level byte is now clean, so it feeds the original
/// `(level-1)/8` power scaling unchanged - no masking. The shiny byte is read
/// *before* the replayed `lbu v0,0x729(v0)` overwrites `v0`. The boosted `*a2`
/// is reloaded by the replayed `lw v1,0(a2)` so the scaling sees it.
fn assemble_damage(disp: [u32; 2], ret: u32) -> Vec<u32> {
    const SKIP: i32 = 13; // index of the replayed `lw v1,0(a2)`
    vec![
        lbu(T8, V0, SHINY_BYTE_OFF), // 0  t8 = shiny[slot] (v0 = slot base here)
        disp[0],                     // 1  lbu v0,0x729(v0)  (v0 = clean level)
        andi(T8, T8, SHINY_FLAG),    // 2  shiny? (fills v0 load delay)
        beq(T8, ZERO, (SKIP - 4) as i16), // 3  not shiny -> skip boost
        nop(),                       // 4
        lw(T9, A2, 0),               // 5  t9 = *a2 (running damage)
        addiu(T0, ZERO, BONUS),      // 6  t0=135 (fills lw load delay)
        multu(T9, T0),               // 7  t9 * 135
        mflo(T9),                    // 8
        addiu(T0, ZERO, 100),        // 9
        divu(T9, T0),                // 10
        mflo(T9),                    // 11
        sw(T9, A2, 0),               // 12 *a2 = damage*135/100
        disp[1],                     // 13 SKIP: lw v1,0(a2)  (reload boosted; replay)
        j(ret),                      // 14
        nop(),                       // 15
    ]
}

// NOTE: the level-up mask routines G1/G2/G3 are GONE. With the shiny flag moved
// out of the level byte (now the parallel `+0x788` shiny array), the spell-level
// byte is always clean, so the level-up math, the `< 9` cap, and the spell-level
// display read correct levels natively - no masking needed. Removing them also
// frees the SCUS gap they occupied.

/// (F) Menu display: tint the level digit orange when the selected spell is
/// shiny. Reads the slot's **shiny byte** (`+0x788(v0)`, `v0`=slot base before
/// the replayed `lbu v0,0x729(v0)` overwrites it). The level byte is clean now,
/// so the digit value needs no masking - F only sets the colour global.
fn assemble_menu_color(disp: [u32; 2], ret: u32) -> Vec<u32> {
    const END: i32 = 9;
    vec![
        lbu(T8, V0, SHINY_BYTE_OFF),       // 0 t8 = shiny[slot] (v0 = slot base)
        disp[0],                           // 1 lw v1,0x46b0(v1) (doesn't touch v0/t8)
        disp[1],                           // 2 lbu v0,0x729(v0) (clean level digit)
        andi(T8, T8, SHINY_FLAG),          // 3 shiny? (fills v0 load delay)
        beq(T8, ZERO, (END - 4) as i16),   // 4 not shiny -> skip the colour set
        nop(),                             // 5
        lui(T9, hi(TEXT_COLOR_GLOBAL_VA)), // 6
        addiu(T0, ZERO, SHINY_MENU_COLOR), // 7
        sw(T0, T9, lo(TEXT_COLOR_GLOBAL_VA)), // 8 _DAT_8007b454 = shiny colour
        j(ret),                            // 9 END
        nop(),                             // 10
    ]
}

/// (H) In-battle magic-menu: stamp `SHINY_CAST_FLAG` (= `0x80` iff the selected
/// spell is shiny) for the summon-fade (K) + cast-text (J') hooks. Reads the
/// slot's **shiny byte** (`+0x788(v0)`); the level byte (`v1`) is clean, so the
/// "Lv N" header needs no masking. `v0`=slot base (survives the `lbu v1` load).
fn assemble_bmenu(disp: [u32; 2], ret: u32) -> Vec<u32> {
    vec![
        disp[0],                            // 0 lbu v1,0x729(v0) (clean level; v0 survives)
        lbu(T8, V0, SHINY_BYTE_OFF),        // 1 t8 = shiny[slot] (v0 still = base)
        disp[1],                            // 2 lbu v0,0x2(s1) (replay; fills loads)
        andi(T8, T8, SHINY_FLAG),           // 3 t8 = 0x80 if shiny else 0
        lui(AT, hi(SHINY_CAST_FLAG_VA)),    // 4
        sb(T8, AT, lo(SHINY_CAST_FLAG_VA)), // 5 SHINY_CAST_FLAG = shiny bit
        j(ret),                             // 6
        nop(),                              // 7
    ]
}

/// (K) Summon transparency: at the draw-time fade read in `FUN_8004a908`, if the
/// current cast is shiny (`SHINY_CAST_FLAG`) and the actor being drawn (`s1`) is
/// the summon creature (battle actor slot 7), override the fade `v0` with
/// `SUMMON_FADE` so the creature renders semi-transparent. The summon's own
/// `+0x226` is rebuilt to 0 every frame, so the fade must be injected at the
/// read. The hook fires for every drawn actor, but only the shiny-summon case
/// changes anything; non-matching cases return the real fade unchanged.
fn assemble_summon_fade(disp: [u32; 2], ret: u32) -> Vec<u32> {
    const RET: i32 = 12; // index of the closing `j(ret)`
    vec![
        disp[0],                              // 0  lbu v0,0x226(s1) (replay; v0 = real fade)
        disp[1],                              // 1  nop (replay; v0 load-delay slot)
        lui(AT, hi(SHINY_CAST_FLAG_VA)),      // 2
        lbu(A0, AT, lo(SHINY_CAST_FLAG_VA)),  // 3  a0 = shiny-cast flag (load)
        lui(AT, hi(SUMMON_ACTOR_SLOT_VA)),    // 4  (fills a0 load-delay)
        lw(A1, AT, lo(SUMMON_ACTOR_SLOT_VA)), // 5  a1 = actor_table[7] = summon ptr (load)
        andi(A0, A0, SHINY_FLAG),             // 6  a0 valid; isolate shiny bit
        beq(A0, ZERO, (RET - 8) as i16),      // 7  not shiny -> keep real fade
        nop(),                                // 8  (a1 valid after this)
        bne(A1, S1, (RET - 10) as i16),       // 9  not the summon -> keep real fade
        nop(),                                // 10
        addiu(V0, ZERO, SUMMON_FADE),         // 11 override fade for the summon
        j(ret),                               // 12 RET: back to the `beq v0,zero`
        nop(),                                // 13
    ]
}

/// (J) +35% cast text: at the battle text-widget draw, if the cast is shiny
/// (`SHINY_CAST_FLAG`) and the widget is the move-name banner (`a1 == 0x801C`),
/// redirect the string pointer `a0` at the custom "+35% DMG!" string so the
/// banner reads it instead of the spell name. Replays the displaced
/// `lw a0,..`/`li v0,7` and returns to the `sw v0,0x13c(gp)` (hook + 8).
/// `a1`/`s4`/`v0` are preserved; only `a0` (intentionally), `AT` and `v1` change.
fn assemble_banner_replace(str_va: u32, disp: [u32; 2], ret: u32) -> Vec<u32> {
    const RET: i32 = 14; // index of the closing `j(ret)`
    vec![
        disp[0],                             // 0  lw a0,0x18(s4) (replay; a0 = name ptr)
        disp[1],                             // 1  li v0,7 (replay; fills a0 delay, sets v0)
        lui(AT, hi(SHINY_CAST_FLAG_VA)),     // 2
        lbu(AT, AT, lo(SHINY_CAST_FLAG_VA)), // 3  AT = shiny-cast flag (load)
        ori(V1, ZERO, BANNER_STYLE_TAG),     // 4  v1 = move-banner style (fills AT delay)
        andi(AT, AT, SHINY_FLAG),            // 5  AT valid; isolate shiny bit
        beq(AT, ZERO, (RET - 7) as i16),     // 6  not shiny -> keep the spell name
        nop(),                               // 7
        bne(A1, V1, (RET - 9) as i16),       // 8  not the banner widget -> keep name
        nop(),                               // 9
        lui(A0, hi(str_va)),                 // 10
        addiu(A0, A0, lo(str_va)),           // 11 a0 = "+35% DMG!" string
        ori(AT, ZERO, BANNER_TOP_Y),         // 12 AT = top-box Y
        sw(AT, SP, 0x10),                    // 13 overwrite the 5th-arg Y slot (top box)
        j(ret),                              // 14 RET: back to `sw v0,0x13c(gp)`
        nop(),                               // 15
    ]
}

/// The "+35% DMG!" banner string: plain ASCII + `0x00` terminator (the banner
/// path inherits the colour from the widget style, so no escape is needed).
/// Our own text (no Sony bytes).
fn banner_string() -> Vec<u8> {
    let mut s = b"+35% DMG!".to_vec();
    s.push(0x00);
    s
}

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

/// A planned shiny-Seru injection: all the same-size writes + the chosen `pct`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShinySeruInjection {
    pub edits: Vec<Edit>,
    pub pct: u8,
}

fn read_word(buf: &[u8], off: usize) -> Result<u32> {
    let b = buf
        .get(off..off + 4)
        .ok_or_else(|| anyhow::anyhow!("buffer too short at {off:#x}"))?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()))
}

fn words_to_bytes(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Resolve a SCUS VA to a file offset and confirm the two hook words: the first
/// must equal `expect_w0` (build fingerprint); the pair is returned to replay.
fn scus_hook(scus: &[u8], va: u32, expect_w0: u32) -> Result<(usize, [u32; 2])> {
    let off = item_names::file_offset_for_va(scus, va)
        .ok_or_else(|| anyhow::anyhow!("can't resolve SCUS VA {va:#x}"))?;
    let w0 = read_word(scus, off)?;
    let w1 = read_word(scus, off + 4)?;
    if w0 != expect_w0 {
        bail!("SCUS hook {va:#x} = {w0:#010x}, expected {expect_w0:#010x} (unrecognized build)");
    }
    Ok((off, [w0, w1]))
}

/// Same for an overlay VA (`file_off = va - OVERLAY_BASE_VA`).
fn ov_hook(overlay: &[u8], va: u32, expect_w0: u32) -> Result<(usize, [u32; 2])> {
    let off = (va - OVERLAY_BASE_VA) as usize;
    let w0 = read_word(overlay, off)?;
    let w1 = read_word(overlay, off + 4)?;
    if w0 != expect_w0 {
        bail!("overlay hook {va:#x} = {w0:#010x}, expected {expect_w0:#010x} (unrecognized build)");
    }
    Ok((off, [w0, w1]))
}

/// Confirm `[va, va+len)` is all-zero dead space in `buf` (file offset = `off`).
fn assert_zero(buf: &[u8], off: usize, len: usize, va: u32) -> Result<()> {
    let region = buf
        .get(off..off + len)
        .ok_or_else(|| anyhow::anyhow!("region {va:#x}..+{len} past end of file"))?;
    if region.iter().any(|&b| b != 0) {
        bail!("region {va:#x}..+{len} is not all-zero dead space (build / collision) - refusing");
    }
    Ok(())
}

impl ShinySeruInjection {
    /// Plan all edits for `pct`% shiny capturable enemies. Needs the
    /// `SCUS_942.54` image, the battle-action overlay (0898) and the menu
    /// overlay (0899) raw PROT entries. Refuses (without touching anything) if
    /// the build isn't the recognized US layout or a routine region isn't dead.
    pub fn plan(
        scus: &[u8],
        ov0898: &[u8],
        ov0899: &[u8],
        pct: u8,
        capturable_ids: &[u16],
    ) -> Result<Self> {
        if pct == 0 || pct > 100 {
            bail!("shiny-seru percent {pct} out of range 1..=100");
        }

        // Resolve + fingerprint every hook (also captures the words to replay).
        let setup = scus_hook(scus, HOOK_SETUP_VA, HOOK_SETUP_W0)?;
        let capture = ov_hook(ov0898, HOOK_CAPTURE_VA, HOOK_CAPTURE_W0)?;
        let grant = ov_hook(ov0898, HOOK_GRANT_VA, HOOK_GRANT_W0)?;
        let gshift = ov_hook(ov0898, HOOK_GRANT_SHIFT_VA, HOOK_GRANT_SHIFT_W0)?;
        let damage = ov_hook(ov0898, HOOK_DAMAGE_VA, HOOK_DAMAGE_W0)?;
        let menu = ov_hook(ov0899, HOOK_MENU_VA, HOOK_MENU_W0)?;
        let bmenu = ov_hook(ov0898, HOOK_BMENU_LVL_VA, HOOK_BMENU_LVL_W0)?;
        let banner = scus_hook(scus, HOOK_BANNER_VA, HOOK_BANNER_W0)?;
        let fade = scus_hook(scus, HOOK_FADE_VA, HOOK_FADE_W0)?;

        // The capturable allowlist bitmap lives at the head of the second SCUS
        // gap; the setup routine indexes it by the first-monster id.
        let bitmap = build_bitmap(capturable_ids);
        let bitmap_va = SCUS_GAP2_VA;

        // --- region 1: SCUS gap 1 (scratch + setup / capture / gate / menu) ---
        let scratch_va = SCUS_GAP_VA;
        let mut scus_va = SCUS_GAP_VA + 4; // 4-byte scratch word reserved first
        let mut place_scus = |words: Vec<u32>| -> Result<(u32, Vec<u32>)> {
            let va = scus_va;
            scus_va += (words.len() * 4) as u32;
            if scus_va > SCUS_GAP_END_VA {
                bail!("shiny routines overrun the SCUS gap end {SCUS_GAP_END_VA:#x}");
            }
            Ok((va, words))
        };
        // --- region 3: SCUS gap 2 now holds only the bitmap (G2/G3 removed). ---
        let gap2_va = SCUS_GAP2_VA + bitmap.len() as u32;
        // Fixed-VA placement for the single-routine runs (menu F, battle-menu H,
        // grant-shift K2, summon-fade K): each routine has its own reference-free
        // run, asserted dead + length-checked below.
        let fit = |va: u32, end: u32, words: Vec<u32>, what: &str| -> Result<(u32, Vec<u32>)> {
            if va + (words.len() * 4) as u32 > end {
                bail!("shiny {what} routine overruns its run end {end:#x}");
            }
            Ok((va, words))
        };
        // --- region 2: 0898 cave (damage + grant) ---
        let mut cave_va = CAVE_VA;
        let mut place_cave = |words: Vec<u32>| -> Result<(u32, Vec<u32>)> {
            let va = cave_va;
            cave_va += (words.len() * 4) as u32;
            if cave_va > CAVE_END_VA {
                bail!("shiny routines overrun the 0898 cave end {CAVE_END_VA:#x}");
            }
            Ok((va, words))
        };

        let (b_va, b_words) =
            place_scus(assemble_setup(pct, bitmap_va, setup.1, HOOK_SETUP_VA + 8))?;
        let (c1_va, c1_words) = place_scus(assemble_capture_copy(
            scratch_va,
            capture.1,
            HOOK_CAPTURE_VA + 8,
        ))?;
        let (d_va, d_words) = place_cave(assemble_damage(damage.1, HOOK_DAMAGE_VA + 8))?;
        let (c2_va, c2_words) =
            place_cave(assemble_grant_shiny(scratch_va, grant.1, HOOK_GRANT_VA + 8))?;
        // Colour-aware menu routine (F) in its own small reference-free run.
        let (f_va, f_words) = fit(
            MENU_RUN_VA,
            MENU_RUN_END_VA,
            assemble_menu_color(menu.1, HOOK_MENU_VA + 8),
            "menu",
        )?;
        // In-battle magic-menu shiny-flag stamper (H).
        let (h_va, h_words) = fit(
            BMENU_RUN_VA,
            BMENU_RUN_END_VA,
            assemble_bmenu(bmenu.1, HOOK_BMENU_LVL_VA + 8),
            "battle-menu",
        )?;
        // Grant shift-hook (K2): mirrors the spell-list shift onto the shiny array.
        let (k2_va, k2_words) = fit(
            SHIFT_RUN_VA,
            SHIFT_RUN_END_VA,
            assemble_grant_shift(gshift.1, HOOK_GRANT_SHIFT_VA + 8),
            "grant-shift",
        )?;
        // Summon-transparency routine (K) in its own reference-free SCUS run.
        let (k_va, k_words) = fit(
            SUMMON_FADE_RUN_VA,
            SUMMON_FADE_RUN_END_VA,
            assemble_summon_fade(fade.1, HOOK_FADE_RET_VA),
            "summon-fade",
        )?;
        // +35% cast-text routine (J) + its display string in one run. The string
        // sits at BANNER_STR_VA after the routine; assert the routine fits before it.
        let banner_words = assemble_banner_replace(BANNER_STR_VA, banner.1, HOOK_BANNER_RET_VA);
        let banner_str = banner_string();
        if BANNER_RUN_VA + (banner_words.len() * 4) as u32 > BANNER_STR_VA {
            bail!("banner routine overruns its string at {BANNER_STR_VA:#x}");
        }
        if BANNER_STR_VA + banner_str.len() as u32 > BANNER_RUN_END_VA {
            bail!("banner routine+string overrun the run end {BANNER_RUN_END_VA:#x}");
        }

        // --- dead-space guards ---------------------------------------------
        // SCUS gap 1: scratch word + its routines (one contiguous span).
        let scus_gap_off = item_names::file_offset_for_va(scus, SCUS_GAP_VA)
            .ok_or_else(|| anyhow::anyhow!("can't resolve SCUS gap VA"))?;
        assert_zero(
            scus,
            scus_gap_off,
            (scus_va - SCUS_GAP_VA) as usize,
            SCUS_GAP_VA,
        )?;
        // SCUS gap 2: just the capturable bitmap now.
        let scus_gap2_off = item_names::file_offset_for_va(scus, SCUS_GAP2_VA)
            .ok_or_else(|| anyhow::anyhow!("can't resolve SCUS gap 2 VA"))?;
        assert_zero(
            scus,
            scus_gap2_off,
            (gap2_va - SCUS_GAP2_VA) as usize,
            SCUS_GAP2_VA,
        )?;
        // Single-routine SCUS runs (menu F, battle-menu H, grant-shift K2,
        // summon-fade K): each its own dead run.
        for (va, words) in [
            (MENU_RUN_VA, &f_words),
            (BMENU_RUN_VA, &h_words),
            (SHIFT_RUN_VA, &k2_words),
            (SUMMON_FADE_RUN_VA, &k_words),
        ] {
            let off = item_names::file_offset_for_va(scus, va)
                .ok_or_else(|| anyhow::anyhow!("can't resolve SCUS run {va:#x}"))?;
            assert_zero(scus, off, words.len() * 4, va)?;
        }
        // Banner run: routine + its string are one contiguous dead span.
        let banner_off = item_names::file_offset_for_va(scus, BANNER_RUN_VA)
            .ok_or_else(|| anyhow::anyhow!("can't resolve banner run VA"))?;
        let banner_span = (BANNER_STR_VA - BANNER_RUN_VA) as usize + banner_str.len();
        assert_zero(scus, banner_off, banner_span, BANNER_RUN_VA)?;
        // 0898 cave: all cave routines (one contiguous span).
        let cave_off = (CAVE_VA - OVERLAY_BASE_VA) as usize;
        assert_zero(ov0898, cave_off, (cave_va - CAVE_VA) as usize, CAVE_VA)?;

        // --- collect all edits ---------------------------------------------
        let detour = |target_va: u32| -> Vec<u8> { words_to_bytes(&[j(target_va), nop()]) };
        let scus_off = |va: u32| item_names::file_offset_for_va(scus, va).unwrap();
        let ov_off = |va: u32| (va - OVERLAY_BASE_VA) as usize;

        let mut edits = vec![
            // Detours (each [j, nop] over the two displaced words).
            Edit {
                prot_index: None,
                file_off: setup.0,
                bytes: detour(b_va),
            },
            Edit {
                prot_index: Some(BATTLE_ACTION_OVERLAY_PROT_INDEX),
                file_off: capture.0,
                bytes: detour(c1_va),
            },
            Edit {
                prot_index: Some(BATTLE_ACTION_OVERLAY_PROT_INDEX),
                file_off: grant.0,
                bytes: detour(c2_va),
            },
            Edit {
                prot_index: Some(BATTLE_ACTION_OVERLAY_PROT_INDEX),
                file_off: gshift.0,
                bytes: detour(SHIFT_RUN_VA),
            },
            Edit {
                prot_index: Some(BATTLE_ACTION_OVERLAY_PROT_INDEX),
                file_off: damage.0,
                bytes: detour(d_va),
            },
            Edit {
                prot_index: Some(MENU_OVERLAY_PROT_INDEX),
                file_off: menu.0,
                bytes: detour(f_va),
            },
            Edit {
                prot_index: Some(BATTLE_ACTION_OVERLAY_PROT_INDEX),
                file_off: bmenu.0,
                bytes: detour(h_va),
            },
            Edit {
                prot_index: None,
                file_off: banner.0,
                bytes: detour(BANNER_RUN_VA),
            },
            Edit {
                prot_index: None,
                file_off: fade.0,
                bytes: detour(SUMMON_FADE_RUN_VA),
            },
            // SCUS-hosted routines.
            Edit {
                prot_index: None,
                file_off: scus_off(b_va),
                bytes: words_to_bytes(&b_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(c1_va),
                bytes: words_to_bytes(&c1_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(f_va),
                bytes: words_to_bytes(&f_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(h_va),
                bytes: words_to_bytes(&h_words),
            },
            // Grant shift-hook routine (K2).
            Edit {
                prot_index: None,
                file_off: scus_off(k2_va),
                bytes: words_to_bytes(&k2_words),
            },
            // Summon-transparency routine (K).
            Edit {
                prot_index: None,
                file_off: scus_off(k_va),
                bytes: words_to_bytes(&k_words),
            },
            // +35% banner routine (J) + its display string, in the banner run.
            Edit {
                prot_index: None,
                file_off: scus_off(BANNER_RUN_VA),
                bytes: words_to_bytes(&banner_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(BANNER_STR_VA),
                bytes: banner_str.clone(),
            },
            // Capturable allowlist bitmap (SCUS gap 2, ahead of g2/g3).
            Edit {
                prot_index: None,
                file_off: scus_off(bitmap_va),
                bytes: bitmap.clone(),
            },
            // 0898-cave-hosted routines.
            Edit {
                prot_index: Some(BATTLE_ACTION_OVERLAY_PROT_INDEX),
                file_off: ov_off(d_va),
                bytes: words_to_bytes(&d_words),
            },
            Edit {
                prot_index: Some(BATTLE_ACTION_OVERLAY_PROT_INDEX),
                file_off: ov_off(c2_va),
                bytes: words_to_bytes(&c2_words),
            },
        ];
        edits.shrink_to_fit();

        Ok(Self { edits, pct })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(w: u32) -> u32 {
        w >> 26
    }

    #[test]
    fn setup_routine_shape() {
        let bm = 0x8007_83C4;
        let r = assemble_setup(2, bm, [HOOK_SETUP_W0, 0x3C03_8008], HOOK_SETUP_VA + 8);
        assert_eq!(r.len(), 44);
        assert_eq!(r[0], jal(RAND_FUNC_VA));
        assert_eq!(r[5], sltiu(T3, T3, 2));
        // Capturable check: read the first-monster id, index the bitmap, test bit.
        assert_eq!(r[14], lbu(T0, V0, lo(FIRST_MONSTER_ID_VA)));
        assert_eq!(r[16], srl(T1, T0, 3), "byte index");
        assert_eq!(r[18], lbu(T3, T3, lo(bm)), "load bitmap byte");
        assert_eq!(r[20], srlv(T3, T3, T0), "shift to the id's bit");
        // The boost loop multiplies by 135 and divides by 100; the increment
        // fills the load-delay slot right after the lhu.
        assert_eq!(r[29], lhu(T9, T8, 0));
        assert_eq!(
            r[30],
            addiu(T4, T4, 2),
            "increment in the lhu load-delay slot"
        );
        assert_eq!(r[31], multu(T9, T6));
        assert_eq!(r[33], divu(T9, T7));
        assert_eq!(r[26], addiu(T6, ZERO, 135));
        // Marks shiny on the enemy actor.
        assert_eq!(r[39], sb(T9, T2, ACTOR_SHINY_OFF));
        // Replays both displaced words then jumps back to hook+8.
        assert_eq!(r[40], HOOK_SETUP_W0);
        assert_eq!(op(r[42]), 0x02);
        assert_eq!(
            (r[42] & 0x03ff_ffff) << 2,
            (HOOK_SETUP_VA + 8) & 0x0fff_ffff
        );
    }

    #[test]
    fn setup_branches_target_replay() {
        let r = assemble_setup(2, 0x8007_83C4, [0, 0], 0);
        // The roll-miss, no-enemy, and not-capturable beqs skip to REPLAY (idx 40).
        for &i in &[6usize, 11, 22] {
            assert_eq!(op(r[i]), 0x04, "idx {i} is beq");
            let off = (r[i] & 0xffff) as i16 as i32;
            assert_eq!(i as i32 + 1 + off, 40, "beq at {i} targets REPLAY");
        }
        // bne idx 36 loops back to idx 28.
        assert_eq!(op(r[36]), 0x05);
        let off = (r[36] & 0xffff) as i16 as i32;
        assert_eq!(36 + 1 + off, 28);
    }

    #[test]
    fn build_bitmap_sets_the_right_bits() {
        let ids = [10u16, 25, 135];
        let bm = build_bitmap(&ids);
        assert_eq!(bm.len(), BITMAP_BYTES);
        for &id in &ids {
            assert_eq!((bm[id as usize >> 3] >> (id & 7)) & 1, 1, "id {id} set");
        }
        // A non-listed id (gobu = 4) is clear.
        assert_eq!((bm[4 >> 3] >> (4 & 7)) & 1, 0, "gobu (id 4) not capturable");
    }

    #[test]
    fn summon_fade_routine_shape() {
        // The hook fingerprint is the real `lbu v0,0x226(s1)`.
        assert_eq!(HOOK_FADE_W0, lbu(V0, S1, 0x226));
        let disp = [HOOK_FADE_W0, nop()];
        let r = assemble_summon_fade(disp, HOOK_FADE_RET_VA);
        assert_eq!(r.len(), 14);
        assert_eq!(r[0], HOOK_FADE_W0, "replays the fade read");
        // Gate on the shiny-cast flag, then on s1 == summon actor.
        assert_eq!(
            r[3],
            lbu(A0, AT, lo(SHINY_CAST_FLAG_VA)),
            "loads shiny flag"
        );
        assert_eq!(
            r[5],
            lw(A1, AT, lo(SUMMON_ACTOR_SLOT_VA)),
            "loads summon ptr"
        );
        assert_eq!(r[6], andi(A0, A0, SHINY_FLAG));
        // beq (idx7) and bne (idx9) both skip to the closing j(ret) at idx12.
        assert_eq!(op(r[7]), 0x04, "idx7 is beq");
        assert_eq!(
            7 + 1 + ((r[7] & 0xffff) as i16 as i32),
            12,
            "beq targets RET"
        );
        assert_eq!(op(r[9]), 0x05, "idx9 is bne");
        assert_eq!(
            9 + 1 + ((r[9] & 0xffff) as i16 as i32),
            12,
            "bne targets RET"
        );
        assert_eq!(r[11], addiu(V0, ZERO, SUMMON_FADE), "overrides the fade");
        // Returns to the `beq v0,zero` right after the read (hook + 8).
        assert_eq!(op(r[12]), 0x02);
        assert_eq!((r[12] & 0x03ff_ffff) << 2, HOOK_FADE_RET_VA & 0x0fff_ffff);
        assert_eq!(HOOK_FADE_RET_VA, HOOK_FADE_VA + 8);
    }

    #[test]
    fn damage_routine_boosts_via_shiny_byte() {
        let disp = [HOOK_DAMAGE_W0, 0x8CC3_0000]; // lbu v0,0x729(v0) ; lw v1,0(a2)
        let r = assemble_damage(disp, HOOK_DAMAGE_VA + 8);
        assert_eq!(r.len(), 16);
        // Reads the parallel shiny byte BEFORE the level load clobbers v0.
        assert_eq!(
            r[0],
            lbu(T8, V0, SHINY_BYTE_OFF),
            "reads the slot shiny byte"
        );
        assert_eq!(r[1], HOOK_DAMAGE_W0, "replays the (clean) level load");
        assert_eq!(r[2], andi(T8, T8, SHINY_FLAG), "tests the shiny bit");
        assert_eq!(r[7], multu(T9, T0));
        assert_eq!(r[13], 0x8CC3_0000, "replays the boosted-*a2 reload");
        // No masking of v0 - the level byte is clean now.
        assert!(!r.iter().any(|&w| w == andi(V0, V0, 0x7F)), "no level mask");
        // beq idx3 skips the boost to SKIP (idx13).
        assert_eq!(3 + 1 + ((r[3] & 0xffff) as i16 as i32), 13);
    }

    #[test]
    fn grant_writes_clean_level_and_shiny_byte() {
        let disp = [HOOK_GRANT_W0, 0xAC40_05D0]; // sb v1,0x729(v0) ; sw zero,0x5d0(v0)
        let r = assemble_grant_shiny(SCUS_GAP_VA, disp, HOOK_GRANT_VA + 8);
        // Level store is replayed UNCHANGED (no shiny OR) - the level stays clean.
        assert_eq!(r[0], HOOK_GRANT_W0, "replays the clean level store");
        assert!(
            !r.iter().any(|&w| w == ori(V1, V1, SHINY_FLAG)),
            "no level OR"
        );
        // Writes the slot-0 shiny byte (0x80 when shiny, else 0).
        assert_eq!(r[7], ori(T7, ZERO, SHINY_FLAG));
        assert_eq!(r[8], sb(T7, V0, SHINY_BYTE_OFF), "stores the shiny byte");
        // beq idx5 skips the 0x80 set to STORE (idx8).
        assert_eq!(5 + 1 + ((r[5] & 0xffff) as i16 as i32), 8);
    }

    #[test]
    fn grant_shift_routine_shape() {
        assert_eq!(HOOK_GRANT_SHIFT_W0, lbu(A2, V0, 0x704));
        let disp = [HOOK_GRANT_SHIFT_W0, nop()];
        let r = assemble_grant_shift(disp, HOOK_GRANT_SHIFT_VA + 8);
        assert_eq!(r[0], lbu(T8, V0, 0x704), "reads the spell count");
        // The loop shifts shiny[i] = shiny[i-1] (lbu -1, sb 0, decrement cursor + count).
        assert_eq!(r[6], lbu(AT, T9, 0xFFFF), "shiny[i-1]");
        assert_eq!(r[8], sb(AT, T9, 0), "shiny[i]");
        // bne idx11 loops back to LOOP (idx6); beq idx2 skips to END (idx13).
        assert_eq!(11 + 1 + ((r[11] & 0xffff) as i16 as i32), 6, "bne -> LOOP");
        assert_eq!(2 + 1 + ((r[2] & 0xffff) as i16 as i32), 13, "beq -> END");
        assert_eq!(r[13], HOOK_GRANT_SHIFT_W0, "replays the count load");
    }

    #[test]
    fn menu_color_no_mask() {
        let disp = [HOOK_MENU_W0, 0x9042_0729]; // lw v1,0x46b0(v1) ; lbu v0,0x729(v0)
        let r = assemble_menu_color(disp, HOOK_MENU_VA + 8);
        assert_eq!(
            r[0],
            lbu(T8, V0, SHINY_BYTE_OFF),
            "reads the slot shiny byte"
        );
        assert_eq!(r[1], HOOK_MENU_W0, "replays the lw");
        assert_eq!(r[2], 0x9042_0729, "replays the (clean) level digit load");
        assert_eq!(r[3], andi(T8, T8, SHINY_FLAG), "tests the shiny bit");
        assert_eq!(
            r[8],
            sw(T0, T9, lo(TEXT_COLOR_GLOBAL_VA)),
            "set shiny colour"
        );
        // No level masking - the byte is clean.
        assert!(!r.iter().any(|&w| w == andi(V0, V0, 0x7F)), "no digit mask");
    }

    #[test]
    fn hook_words_match_documented_disassembly() {
        assert_eq!(HOOK_SETUP_W0, lui(V0, 0x8008));
        assert_eq!(HOOK_CAPTURE_W0, sb(V0, 4, 0x269)); // sb v0,0x269(a0)
        assert_eq!(HOOK_GRANT_W0, sb(V1, V0, 0x729));
        assert_eq!(HOOK_GRANT_SHIFT_W0, lbu(A2, V0, 0x704));
        assert_eq!(HOOK_DAMAGE_W0, lbu(V0, V0, 0x729));
        assert_eq!(HOOK_MENU_W0, lw(V1, V1, 0x46b0));
    }

    #[test]
    fn plan_rejects_out_of_range_pct() {
        let scus = vec![0u8; 0x100];
        let ov = vec![0u8; 0x100];
        let ids = [10u16, 25];
        assert!(ShinySeruInjection::plan(&scus, &ov, &ov, 0, &ids).is_err());
        assert!(ShinySeruInjection::plan(&scus, &ov, &ov, 101, &ids).is_err());
    }

    #[test]
    fn routines_fit_their_regions() {
        let bm = SCUS_GAP2_VA;
        // Region 1 (SCUS gap 1): scratch(4) + B + C1 (G1 removed).
        let r1 = 4
            + (assemble_setup(2, bm, [0, 0], 0).len() + assemble_capture_copy(0, [0, 0], 0).len())
                * 4;
        assert!(
            SCUS_GAP_VA + r1 as u32 <= SCUS_GAP_END_VA,
            "region 1 fits ({r1} bytes)"
        );
        // Region 3 (SCUS gap 2): just the bitmap now (G2/G3 removed).
        assert!(
            SCUS_GAP2_VA + BITMAP_BYTES as u32 <= SCUS_GAP2_END_VA,
            "region 3 fits"
        );
        // Menu run (F), battle-menu run (H), grant-shift run (K2): each its own run.
        let rf = assemble_menu_color([0, 0], 0).len() * 4;
        assert!(
            MENU_RUN_VA + rf as u32 <= MENU_RUN_END_VA,
            "menu run fits ({rf})"
        );
        let rh = assemble_bmenu([0, 0], 0).len() * 4;
        assert!(
            BMENU_RUN_VA + rh as u32 <= BMENU_RUN_END_VA,
            "bmenu run fits ({rh})"
        );
        let rk2 = assemble_grant_shift([0, 0], 0).len() * 4;
        assert!(
            SHIFT_RUN_VA + rk2 as u32 <= SHIFT_RUN_END_VA,
            "shift run fits ({rk2})"
        );
        // Region 2 (0898 cave): D + C2.
        let r2 = (assemble_damage([0, 0], 0).len() + assemble_grant_shiny(0, [0, 0], 0).len()) * 4;
        assert!(CAVE_VA + r2 as u32 <= CAVE_END_VA, "cave fits ({r2} bytes)");
    }

    #[test]
    fn bmenu_stamps_shiny_flag_from_byte() {
        let r = assemble_bmenu([HOOK_BMENU_LVL_W0, 0x9222_0002], HOOK_BMENU_LVL_VA + 8);
        assert_eq!(r[0], HOOK_BMENU_LVL_W0, "replays the (clean) level load");
        assert_eq!(
            r[1],
            lbu(T8, V0, SHINY_BYTE_OFF),
            "reads the slot shiny byte"
        );
        assert_eq!(r[3], andi(T8, T8, SHINY_FLAG), "isolates the shiny bit");
        assert_eq!(
            r[5],
            sb(T8, AT, lo(SHINY_CAST_FLAG_VA)),
            "stamps the shiny-cast flag"
        );
        // No display masking - the level byte is clean.
        assert!(
            !r.iter().any(|&w| w == andi(V1, V1, 0x7F)),
            "no display mask"
        );
    }

    #[test]
    fn banner_routine_and_string() {
        // HOOK_BANNER_W0 is `lw a0,0x18(s4)` (s4 = $20); verified against the disc
        // by the disc-gated `baseline_hooks_match_the_known_build`.
        assert_eq!(HOOK_BANNER_W0, lw(A0, 20, 0x18));
        let disp = [HOOK_BANNER_W0, 0x2402_0007]; // lw a0,0x18(s4) ; li v0,7
        let r = assemble_banner_replace(BANNER_STR_VA, disp, HOOK_BANNER_RET_VA);
        assert_eq!(r.len(), 16);
        assert_eq!(r[0], HOOK_BANNER_W0, "replays the string load");
        // Gate on the shiny flag, then on the move-banner style tag.
        assert_eq!(
            r[3],
            lbu(AT, AT, lo(SHINY_CAST_FLAG_VA)),
            "loads shiny flag"
        );
        assert_eq!(
            r[4],
            ori(V1, ZERO, BANNER_STYLE_TAG),
            "loads the banner-style tag"
        );
        assert_eq!(r[5], andi(AT, AT, SHINY_FLAG));
        // beq (idx6) and bne (idx8) both skip to the closing j(ret) at idx14.
        assert_eq!(op(r[6]), 0x04, "idx6 is beq");
        assert_eq!(
            6 + 1 + ((r[6] & 0xffff) as i16 as i32),
            14,
            "beq targets RET"
        );
        assert_eq!(op(r[8]), 0x05, "idx8 is bne (a1 != style tag)");
        assert_eq!(
            8 + 1 + ((r[8] & 0xffff) as i16 as i32),
            14,
            "bne targets RET"
        );
        // Shiny banner path points a0 at the custom string + relocates Y to the box.
        assert_eq!(r[10], lui(A0, hi(BANNER_STR_VA)));
        assert_eq!(
            r[11],
            addiu(A0, A0, lo(BANNER_STR_VA)),
            "a0 = custom string"
        );
        assert_eq!(r[12], ori(AT, ZERO, BANNER_TOP_Y), "loads the top-box Y");
        assert_eq!(r[13], sw(AT, SP, 0x10), "overwrites the 5th-arg Y slot");
        // Returns to the `sw v0,0x13c(gp)` right after the load (hook + 8).
        assert_eq!(op(r[14]), 0x02);
        assert_eq!((r[14] & 0x03ff_ffff) << 2, HOOK_BANNER_RET_VA & 0x0fff_ffff);
        assert_eq!(HOOK_BANNER_RET_VA, HOOK_BANNER_VA + 8);
        // The string is plain ASCII with a 0x00 terminator and fits the run.
        let s = banner_string();
        assert_eq!(s[0], b'+', "plain ASCII (no colour escape)");
        assert_eq!(*s.last().unwrap(), 0x00, "0x00 terminator");
        assert!(
            BANNER_RUN_VA + (r.len() * 4) as u32 <= BANNER_STR_VA,
            "routine fits before string"
        );
        assert!(
            BANNER_STR_VA + s.len() as u32 <= BANNER_RUN_END_VA,
            "string fits the run"
        );
    }
}
