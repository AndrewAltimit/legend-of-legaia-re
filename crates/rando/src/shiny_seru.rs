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
//!    and it is marked shiny on a free per-actor byte (`+0x226`). The capturable
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
/// Damage hook (overlay 0898, `FUN_801dd864`): the spell level is read into the
/// summon-damage scaler.
pub const HOOK_DAMAGE_VA: u32 = 0x801D_DB08;
const HOOK_DAMAGE_W0: u32 = 0x9042_0729; // lbu v0,0x729(v0)
/// Level-up gate read (overlay 0898, `FUN_801E70BC`): the `< 9` cap test.
pub const HOOK_LVL_GATE_VA: u32 = 0x801E_71C8;
const HOOK_LVL_GATE_W0: u32 = 0x90C2_0729; // lbu v0,0x729(a2)
/// Level-up working read (overlay 0898, `FUN_801E70BC`): the value incremented.
pub const HOOK_LVL_READ_VA: u32 = 0x801E_71DC;
const HOOK_LVL_READ_W0: u32 = 0x90C7_0729; // lbu a3,0x729(a2)
/// Level-up writeback (overlay 0898, `FUN_801E70BC`): the new level store.
pub const HOOK_LVL_WRITE_VA: u32 = 0x801E_7224;
const HOOK_LVL_WRITE_W0: u32 = 0xA0C2_0729; // sb v0,0x729(a2)
/// Menu spell-list level-digit read (overlay 0899, `FUN_801d2e74`).
pub const HOOK_MENU_VA: u32 = 0x801D_2FA0;
const HOOK_MENU_W0: u32 = 0x8C63_46B0; // lw v1,0x46b0(v1)

// --- Runtime offsets / globals ---------------------------------------------

/// Battle-actor pointer table slot 3 (frontmost enemy) VA.
const ACTOR_SLOT3_VA: u32 = 0x801C_937C;
/// Per-actor free byte used as the shiny marker (zero-init each battle).
const ACTOR_SHINY_OFF: u16 = 0x226;
/// Spell-level byte offset in the live character record / SC block.
const LEVEL_OFF: u16 = 0x729;
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
/// Second SCUS rodata gap (reference-free padding) hosting the capturable
/// bitmap + the level-up read-mask routines.
pub const SCUS_GAP2_VA: u32 = 0x8007_83C4;
/// First VA past the second SCUS gap; its contents must end at or below this.
pub const SCUS_GAP2_END_VA: u32 = 0x8007_8420;

// --- MIPS R3000 encoders (little-endian) -----------------------------------

const ZERO: u32 = 0;
const V0: u32 = 2;
const V1: u32 = 3;
const A2: u32 = 6;
const A3: u32 = 7;
const T0: u32 = 8;
const T1: u32 = 9;
const T2: u32 = 10;
const T3: u32 = 11;
const T4: u32 = 12;
const T5: u32 = 13;
const T6: u32 = 14;
const T7: u32 = 15;
const T8: u32 = 24;
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
const fn or_(rd: u32, rs: u32, rt: u32) -> u32 {
    (rs << 21) | (rt << 16) | (rd << 11) | 0x25
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

/// (C2) Grant: OR `0x80` into the just-granted level byte when `scratch != 0`,
/// then replay. `v0`=record base (preserved), `v1`=1 (the level).
fn assemble_grant_or(scratch_va: u32, disp: [u32; 2], ret: u32) -> Vec<u32> {
    const W: i32 = 6; // index of the replayed level store (the `beq` skip target)
    vec![
        lui(T9, hi(scratch_va)),       // 0
        lbu(T8, T9, lo(scratch_va)),   // 1
        nop(),                         // 2 load delay
        beq(T8, ZERO, (W - 4) as i16), // 3 not shiny -> skip the OR
        nop(),                         // 4
        ori(V1, V1, SHINY_FLAG),       // 5 v1 = 1 | 0x80
        disp[0],                       // 6 W: sb v1,0x729(v0)
        disp[1],                       // 7 sw zero,0x5d0(v0)
        j(ret),                        // 8
        nop(),                         // 9
    ]
}

/// (D) Damage: when the level byte has `0x80`, multiply the running summon
/// damage (`*a2`) ×135/100; then strip the bit so the original `(level-1)/8`
/// math is correct. `v0`=level base (leave masked level), `a2`=damage ptr.
fn assemble_damage(disp: [u32; 2], ret: u32) -> Vec<u32> {
    const SKIP: i32 = 13; // index of the final `andi v0`
    vec![
        disp[0],                          // 0  lbu v0,0x729(v0)  (replay the level load)
        addiu(T0, ZERO, BONUS),           // 1  LOAD-DELAY SLOT: t0=135 (doesn't touch v0)
        andi(T8, V0, SHINY_FLAG),         // 2  v0 valid now
        beq(T8, ZERO, (SKIP - 4) as i16), // 3  not shiny -> skip boost
        nop(),                            // 4
        lw(T9, A2, 0),                    // 5  t9 = *a2 (running damage)
        nop(),                            // 6  LOAD-DELAY SLOT for the lw
        multu(T9, T0),                    // 7  t9 * 135
        mflo(T9),                         // 8
        addiu(T0, ZERO, 100),             // 9
        divu(T9, T0),                     // 10
        mflo(T9),                         // 11
        sw(T9, A2, 0),                    // 12 *a2 = damage*135/100
        andi(V0, V0, 0x7F),               // 13 SKIP: strip shiny bit
        disp[1],                          // 14 lw v1,0(a2)  (replay)
        j(ret),                           // 15
        nop(),                            // 16
    ]
}

/// (G1) Level-up gate read: mask the shiny bit so the `< 9` cap sees the real
/// level (a shiny Seru still levels up). `v0` = level (leave masked).
fn assemble_lvl_gate(disp: [u32; 2], ret: u32) -> Vec<u32> {
    vec![
        disp[0],            // lbu v0,0x729(a2)  (replay the level load)
        disp[1],            // LOAD-DELAY SLOT: replay the original successor (doesn't use v0)
        andi(V0, V0, 0x7F), // strip shiny bit (v0 valid now)
        j(ret),
        nop(),
    ]
}

/// (G2) Level-up working read: mask the shiny bit so the level math + threshold
/// index see the real level. `a3` = level (leave masked).
fn assemble_lvl_read(disp: [u32; 2], ret: u32) -> Vec<u32> {
    vec![
        disp[0],            // lbu a3,0x729(a2)  (replay the level load)
        disp[1],            // LOAD-DELAY SLOT: replay the original successor (doesn't use a3)
        andi(A3, A3, 0x7F), // strip shiny bit (a3 valid now)
        j(ret),
        nop(),
    ]
}

/// (G3) Level-up writeback: re-apply the shiny bit (read from the old byte
/// before the store) so leveling preserves it. `v0` = new level, `a2` = base.
fn assemble_lvl_write(disp: [u32; 2], ret: u32) -> Vec<u32> {
    vec![
        lbu(T8, A2, LEVEL_OFF),   // t8 = old byte (still has 0x80 if shiny)
        disp[1],                  // LOAD-DELAY SLOT: replay the original successor (doesn't use t8)
        andi(T8, T8, SHINY_FLAG), // keep just the shiny bit (t8 valid now)
        or_(V0, V0, T8),          // v0 = new level | shiny
        disp[0],                  // sb v0,0x729(a2)  (replay; v0 now carries 0x80)
        j(ret),
        nop(),
    ]
}

/// (F) Menu display: mask the shiny bit so the level digit renders correctly.
/// `v1` = table base (replay loads it), `v0` = level (leave masked).
fn assemble_menu_mask(disp: [u32; 2], ret: u32) -> Vec<u32> {
    vec![
        disp[1],            // lbu v0,0x729(v0)  (replay the level load FIRST)
        disp[0],            // LOAD-DELAY SLOT: lw v1,0x46b0(v1) (replay; doesn't use v0)
        andi(V0, V0, 0x7F), // strip shiny bit for the digit (v0 valid now)
        j(ret),
        nop(),
    ]
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
        let damage = ov_hook(ov0898, HOOK_DAMAGE_VA, HOOK_DAMAGE_W0)?;
        let lgate = ov_hook(ov0898, HOOK_LVL_GATE_VA, HOOK_LVL_GATE_W0)?;
        let lread = ov_hook(ov0898, HOOK_LVL_READ_VA, HOOK_LVL_READ_W0)?;
        let lwrite = ov_hook(ov0898, HOOK_LVL_WRITE_VA, HOOK_LVL_WRITE_W0)?;
        let menu = ov_hook(ov0899, HOOK_MENU_VA, HOOK_MENU_W0)?;

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
        // --- region 3: SCUS gap 2 (bitmap, then the level-up read-mask routines) ---
        let mut gap2_va = SCUS_GAP2_VA + bitmap.len() as u32;
        let mut place_gap2 = |words: Vec<u32>| -> Result<(u32, Vec<u32>)> {
            let va = gap2_va;
            gap2_va += (words.len() * 4) as u32;
            if gap2_va > SCUS_GAP2_END_VA {
                bail!("shiny routines overrun the second SCUS gap end {SCUS_GAP2_END_VA:#x}");
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
        let (g1_va, g1_words) = place_scus(assemble_lvl_gate(lgate.1, HOOK_LVL_GATE_VA + 8))?;
        let (f_va, f_words) = place_scus(assemble_menu_mask(menu.1, HOOK_MENU_VA + 8))?;
        let (g2_va, g2_words) = place_gap2(assemble_lvl_read(lread.1, HOOK_LVL_READ_VA + 8))?;
        let (g3_va, g3_words) = place_gap2(assemble_lvl_write(lwrite.1, HOOK_LVL_WRITE_VA + 8))?;
        let (d_va, d_words) = place_cave(assemble_damage(damage.1, HOOK_DAMAGE_VA + 8))?;
        let (c2_va, c2_words) =
            place_cave(assemble_grant_or(scratch_va, grant.1, HOOK_GRANT_VA + 8))?;

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
        // SCUS gap 2: bitmap + its routines (one contiguous span).
        let scus_gap2_off = item_names::file_offset_for_va(scus, SCUS_GAP2_VA)
            .ok_or_else(|| anyhow::anyhow!("can't resolve SCUS gap 2 VA"))?;
        assert_zero(
            scus,
            scus_gap2_off,
            (gap2_va - SCUS_GAP2_VA) as usize,
            SCUS_GAP2_VA,
        )?;
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
                file_off: damage.0,
                bytes: detour(d_va),
            },
            Edit {
                prot_index: Some(BATTLE_ACTION_OVERLAY_PROT_INDEX),
                file_off: lgate.0,
                bytes: detour(g1_va),
            },
            Edit {
                prot_index: Some(BATTLE_ACTION_OVERLAY_PROT_INDEX),
                file_off: lread.0,
                bytes: detour(g2_va),
            },
            Edit {
                prot_index: Some(BATTLE_ACTION_OVERLAY_PROT_INDEX),
                file_off: lwrite.0,
                bytes: detour(g3_va),
            },
            Edit {
                prot_index: Some(MENU_OVERLAY_PROT_INDEX),
                file_off: menu.0,
                bytes: detour(f_va),
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
                file_off: scus_off(g2_va),
                bytes: words_to_bytes(&g2_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(g3_va),
                bytes: words_to_bytes(&g3_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(f_va),
                bytes: words_to_bytes(&f_words),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(g1_va),
                bytes: words_to_bytes(&g1_words),
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
    fn damage_routine_boosts_and_masks() {
        let disp = [HOOK_DAMAGE_W0, 0x8CC3_0000];
        let r = assemble_damage(disp, HOOK_DAMAGE_VA + 8);
        assert_eq!(r.len(), 17);
        assert_eq!(r[0], HOOK_DAMAGE_W0, "replays the level load");
        // The shiny test is one instr later now (load-delay slot between).
        assert_eq!(r[2], andi(T8, V0, SHINY_FLAG), "tests the shiny bit");
        assert_eq!(r[6], nop(), "load-delay slot after the *a2 load");
        assert_eq!(r[7], multu(T9, T0));
        assert_eq!(r[13], andi(V0, V0, 0x7F), "strips the bit for level-1 math");
        assert_eq!(r[14], 0x8CC3_0000, "replays the displaced lw");
        // beq idx3 skips the boost to SKIP (idx13).
        let off = (r[3] & 0xffff) as i16 as i32;
        assert_eq!(3 + 1 + off, 13);
    }

    #[test]
    fn grant_sets_shiny_bit() {
        let disp = [HOOK_GRANT_W0, 0xAC40_05D0];
        let r = assemble_grant_or(SCUS_GAP_VA, disp, HOOK_GRANT_VA + 8);
        assert_eq!(r[5], ori(V1, V1, SHINY_FLAG));
        assert_eq!(r[6], HOOK_GRANT_W0, "replays the level store");
        assert_eq!(r[7], 0xAC40_05D0);
        // beq idx3 skips the OR to W (idx6).
        let off = (r[3] & 0xffff) as i16 as i32;
        assert_eq!(3 + 1 + off, 6);
    }

    #[test]
    fn levelup_write_preserves_shiny() {
        let disp = [HOOK_LVL_WRITE_W0, 0x2404_0065];
        let r = assemble_lvl_write(disp, HOOK_LVL_WRITE_VA + 8);
        assert_eq!(r[0], lbu(T8, A2, LEVEL_OFF));
        assert_eq!(r[1], 0x2404_0065, "load-delay slot replays the successor");
        assert_eq!(r[2], andi(T8, T8, SHINY_FLAG));
        assert_eq!(r[3], or_(V0, V0, T8));
        assert_eq!(r[4], HOOK_LVL_WRITE_W0);
    }

    #[test]
    fn menu_mask_strips_bit() {
        let disp = [HOOK_MENU_W0, 0x9042_0729];
        let r = assemble_menu_mask(disp, HOOK_MENU_VA + 8);
        // The level load (disp[1]) goes first; the lw (disp[0]) fills its delay slot.
        assert_eq!(r[0], 0x9042_0729, "lbu level load first");
        assert_eq!(r[1], HOOK_MENU_W0, "lw in the load-delay slot");
        assert_eq!(r[2], andi(V0, V0, 0x7F));
    }

    #[test]
    fn hook_words_match_documented_disassembly() {
        assert_eq!(HOOK_SETUP_W0, lui(V0, 0x8008));
        assert_eq!(HOOK_CAPTURE_W0, sb(V0, 4, 0x269)); // sb v0,0x269(a0)
        assert_eq!(HOOK_GRANT_W0, sb(V1, V0, 0x729));
        assert_eq!(HOOK_DAMAGE_W0, lbu(V0, V0, 0x729));
        assert_eq!(HOOK_LVL_GATE_W0, lbu(V0, A2, 0x729));
        assert_eq!(HOOK_LVL_READ_W0, lbu(A3, A2, 0x729));
        assert_eq!(HOOK_LVL_WRITE_W0, sb(V0, A2, 0x729));
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
        // Region 1 (SCUS gap 1): scratch(4) + B + C1 + G1 + F.
        let r1 = 4
            + (assemble_setup(2, bm, [0, 0], 0).len()
                + assemble_capture_copy(0, [0, 0], 0).len()
                + assemble_lvl_gate([0, 0], 0).len()
                + assemble_menu_mask([0, 0], 0).len())
                * 4;
        assert!(
            SCUS_GAP_VA + r1 as u32 <= SCUS_GAP_END_VA,
            "region 1 fits ({r1} bytes)"
        );
        // Region 3 (SCUS gap 2): bitmap + G2 + G3.
        let r3 = BITMAP_BYTES
            + (assemble_lvl_read([0, 0], 0).len() + assemble_lvl_write([0, 0], 0).len()) * 4;
        assert!(
            SCUS_GAP2_VA + r3 as u32 <= SCUS_GAP2_END_VA,
            "region 3 fits ({r3} bytes)"
        );
        // Region 2 (0898 cave): D + C2.
        let r2 = (assemble_damage([0, 0], 0).len() + assemble_grant_or(0, [0, 0], 0).len()) * 4;
        assert!(CAVE_VA + r2 as u32 <= CAVE_END_VA, "cave fits ({r2} bytes)");
    }
}
