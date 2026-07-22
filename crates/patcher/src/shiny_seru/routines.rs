//! Routine assemblers. Each takes the two displaced words it must replay (read
//! from the image at plan time) plus its return VA, and emits the hand-assembled
//! MIPS body as a `Vec<u32>`. Also the capturable-Seru allowlist derivation.

use anyhow::Result;

use super::encode::*;
use super::layout::*;

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
pub(crate) fn assemble_setup(pct: u8, bitmap_va: u32, disp: [u32; 2], ret: u32) -> Vec<u32> {
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
pub(crate) fn build_bitmap(ids: &[u16]) -> Vec<u8> {
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
pub(crate) fn assemble_capture_copy(scratch_va: u32, disp: [u32; 2], ret: u32) -> Vec<u32> {
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
pub(crate) fn assemble_grant_shiny(scratch_va: u32, disp: [u32; 2], ret: u32) -> Vec<u32> {
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
pub(crate) fn assemble_grant_shift(disp: [u32; 2], ret: u32) -> Vec<u32> {
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
pub(crate) fn assemble_damage(disp: [u32; 2], ret: u32) -> Vec<u32> {
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
pub(crate) fn assemble_menu_color(disp: [u32; 2], ret: u32) -> Vec<u32> {
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
pub(crate) fn assemble_bmenu(disp: [u32; 2], ret: u32) -> Vec<u32> {
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
pub(crate) fn assemble_summon_fade(disp: [u32; 2], ret: u32) -> Vec<u32> {
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
pub(crate) fn assemble_banner_replace(str_va: u32, disp: [u32; 2], ret: u32) -> Vec<u32> {
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
pub(crate) fn banner_string() -> Vec<u8> {
    let mut s = b"+35% DMG!".to_vec();
    s.push(0x00);
    s
}
