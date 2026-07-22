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
//! both displaced words and returns to `hook+8`). All routines + data are
//! `SCUS_942.54`-resident now (a `j` from an overlay-0898 hook reaches SCUS), in
//! regions that are each (a) all-zero in the clean image, (b) constant-zero
//! across diverse in-battle save states, and (c) **outside every known live
//! table**:
//!
//! - **gap 1** `0x80077728` (padding before the steal table): scratch word +
//!   setup (B) + capture-copy (C1).
//! - **arena 1** `0x8007AE00` (high tail of the shared `0x8007AB38` rodata gap,
//!   above the bonus-drop/charm/flee-EXP routines): damage (D), grant (C2),
//!   summon-fade (K), grant-shift (K2).
//! - **arena 2** `0x8007AFF8`: the +35% cast-banner routine (J).
//! - **arena 3** `0x8007075C`: field-menu colour routine (F).
//! - **arena 4** `0x80079340`: in-battle menu shiny-flag stamper (H) + the
//!   one-byte `SHINY_CAST_FLAG`.
//! - **arena 5** `0x80079509`: data only - the 32-byte capturable allowlist
//!   bitmap + the "+35% DMG!" display string.
//!
//! Every **routine** VA is **4-byte aligned**: a routine is the target of a `j`
//! detour, and `j` drops the target's low 2 bits, so an unaligned entry jumps 2-3
//! bytes into garbage. The zero-run scan returns run *starts* that are often
//! unaligned (a run begins right after the previous non-zero byte), so each
//! routine VA is rounded up to a word boundary; only byte-addressed data (arena
//! 5) may sit unaligned. (An earlier relocation skipped this and put J/F/H at
//! unaligned arena starts, which froze the Tetsu tutorial when J's detour fired.
//! `place` now refuses an unaligned routine VA.)
//!
//! ### Why "reference-free zero region" was not enough
//!
//! An earlier layout placed routines in the zero *padding* of two live indexed
//! tables - the victory mouth-override table (`ART_MOUTH_VA = 0x80077E80`,
//! addressed rows `0x800781B0..`, 0x30-byte rows with zero keyframe tails) and
//! the move-power table (`0x801F4F5C`, 26-byte records, records 4..8 zero). Those
//! slots are zero in the file but are still **indexed at runtime**: the victory
//! face animator read our routine bytes as facial keyframes (corrupted mouth) and
//! six move ids (`0x07/0x12..0x15/0x19`) read them as move-power records (garbage
//! damage + trail texpage). The fix relocates everything out, and a structural
//! guard ([`assert_not_in_tables`] over [`SCUS_TABLE_RANGES`] /
//! [`OVERLAY_TABLE_RANGES`]) now refuses any region that overlaps a known table,
//! even all-zero - "is it zero?" is necessary but not sufficient.
//!
//! All edits are guarded: each hook's instruction word must match the recognized
//! US build, each region must be all-zero dead space, AND outside every live
//! table. A differently-laid-out image is refused, not corrupted. No Sony bytes
//! are embedded; the routines are the randomizer's own code.

mod encode;
mod layout;
mod plan;
mod routines;

pub use layout::*;
pub use plan::*;
pub use routines::*;

#[cfg(test)]
mod tests {
    use super::encode::*;
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
        // gap 1: scratch(4) + B + C1 + bitmap + cast flag + "+35% DMG!" string.
        let bm = bitmap_va_for_test();
        let r1 = 4
            + (assemble_setup(2, bm, [0, 0], 0).len() + assemble_capture_copy(0, [0, 0], 0).len())
                * 4
            + BITMAP_BYTES
            + 1
            + banner_string().len();
        assert!(
            SCUS_GAP_VA + r1 as u32 <= SCUS_GAP_END_VA,
            "gap 1 fits ({r1} bytes)"
        );
        assert_eq!(
            bm,
            SHINY_CAST_FLAG_VA - BITMAP_BYTES as u32,
            "bitmap before flag"
        );
        assert_eq!(BANNER_STR_VA, SHINY_CAST_FLAG_VA + 1, "string after flag");
        // arena 1: D + C2 + K2 + H + F (the five battle/menu routines; K moved out).
        let a1 = (assemble_damage([0, 0], 0).len()
            + assemble_grant_shiny(0, [0, 0], 0).len()
            + assemble_grant_shift([0, 0], 0).len()
            + assemble_bmenu([0, 0], 0).len()
            + assemble_menu_color([0, 0], 0).len())
            * 4;
        assert!(
            ARENA1_VA + a1 as u32 <= ARENA1_END_VA,
            "arena 1 fits ({a1} bytes)"
        );
        // slot 6: summon-fade (K).
        let k = assemble_summon_fade([0, 0], 0).len() * 4;
        assert!(SLOT6_VA + k as u32 <= SLOT6_END_VA, "slot 6 (K) fits ({k})");
        // arena 2: banner routine (J) (string lives in gap 1).
        let banner = assemble_banner_replace(0, [0, 0], 0).len() * 4;
        assert!(
            ARENA2_VA + banner as u32 <= ARENA2_END_VA,
            "arena 2 (banner) fits ({banner})"
        );
    }

    /// The bitmap VA the plan computes (after scratch + B + C1 in gap 1).
    fn bitmap_va_for_test() -> u32 {
        SHINY_CAST_FLAG_VA - BITMAP_BYTES as u32
    }

    #[test]
    fn no_region_overlaps_a_live_table() {
        // The whole point of the relocation: every shiny SCUS region is outside
        // every live static table AND the SsAPI sound/effect tables (the old
        // arena3/4/5 trap). These are the only read-watch-verified-dead regions.
        for (va, len) in [
            (SCUS_GAP_VA, SCUS_GAP_END_VA - SCUS_GAP_VA),
            (ARENA1_VA, ARENA1_END_VA - ARENA1_VA),
            (ARENA2_VA, ARENA2_END_VA - ARENA2_VA),
            (SLOT6_VA, SLOT6_END_VA - SLOT6_VA),
        ] {
            assert_not_in_tables(va, len, SCUS_TABLE_RANGES, "arena").unwrap_or_else(|e| {
                panic!("region {va:#x}..+{len} should be table-free: {e}");
            });
        }
        // And the guard fires for the old, live regions (mouth-override + the
        // 0x80079xxx SsAPI sound tables the old arena4/arena5 squatted in).
        assert!(assert_not_in_tables(0x8007_81BC, 0x40, SCUS_TABLE_RANGES, "x").is_err());
        assert!(assert_not_in_tables(0x8007_9340, 0x20, SCUS_TABLE_RANGES, "old-arena4").is_err());
        assert!(assert_not_in_tables(0x8007_9509, 0x3B, SCUS_TABLE_RANGES, "old-arena5").is_err());
        assert!(assert_not_in_tables(0x8007_075C, 0x30, SCUS_TABLE_RANGES, "old-arena3").is_err());
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
        // The string is plain ASCII with a 0x00 terminator.
        let s = banner_string();
        assert_eq!(s[0], b'+', "plain ASCII (no colour escape)");
        assert_eq!(*s.last().unwrap(), 0x00, "0x00 terminator");
        // The J routine is 4-byte aligned and fits arena 2 (string lives in gap 1).
        assert_eq!(BANNER_RUN_VA & 3, 0, "banner routine is 4-byte aligned");
        assert!(
            BANNER_RUN_VA + (r.len() * 4) as u32 <= ARENA2_END_VA,
            "routine fits arena 2"
        );
        // The string follows the bitmap + cast flag in gap 1.
        assert_eq!(
            BANNER_STR_VA,
            SHINY_CAST_FLAG_VA + 1,
            "string after cast flag"
        );
        assert!(
            BANNER_STR_VA + s.len() as u32 <= SCUS_GAP_END_VA,
            "string fits gap 1"
        );
    }

    #[test]
    fn all_routine_arenas_are_word_aligned() {
        // A routine VA is a `j` target; `j` drops the low 2 bits, so an unaligned
        // entry jumps into garbage (the bug that froze the Tetsu tutorial).
        for (va, what) in [
            (SCUS_GAP_VA, "gap1"),
            (ARENA1_VA, "arena1"),
            (ARENA2_VA, "arena2/banner"),
            (SLOT6_VA, "slot6/summon-fade"),
            (SUMMON_FADE_RUN_VA, "summon-fade"),
            (SHIFT_RUN_VA, "grant-shift"),
            (BMENU_RUN_VA, "bmenu"),
            (MENU_RUN_VA, "menu"),
        ] {
            assert_eq!(
                va & 3,
                0,
                "{what} routine VA {va:#x} must be 4-byte aligned"
            );
        }
    }
}
