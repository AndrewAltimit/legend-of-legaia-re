//! The battle-overlay AI edits: the hook and block addresses, the Gi arm
//! constants, and the two assembled bodies.

use super::{COURSE_WORD_VA, COURSE3_SEED_WORD, DISPLAY_ROUTINE_VA};
use legaia_code_hooks::mips::*;

// --- AI retime (the bespoke Delilas arm, battle overlay) ---------------------

/// AI-retime hook site: the bespoke Delilas AI arm (`case 0xa2..0xa4` in the
/// monster picker `FUN_801E9FD4`, battle overlay) queues the signature
/// special (`action = formation_id - 0x29`) whenever the shared battle turn
/// counter `ctx[0x28A]` satisfies `% 3 == 2` - attack, attack, special, with
/// both siblings synchronized in the 1v2. The retimed course round wants the
/// special once per **4** turns with the siblings **staggered 2 apart**, so
/// the arm's ctx reload `lw v0,-0x42dc(v0)` (its `lui` already executed, its
/// delay slot is the stock load-delay `nop` - both safe) becomes
/// `j AI_BLOCK_VA`. The block is course-gated: the Che & Lu round
/// ([`COURSE3_SEED_WORD`]) takes the local retime arm, the Gi round
/// ([`GI_ROUND_WORD`]) is forwarded to the second cave
/// ([`assemble_gi_arm`]), and every other context (ravine duels, Master
/// rounds) resumes the stock arm with every register as retail left it.
pub const AI_HOOK_VA: u32 = 0x801E_B7C4;
/// The stock instruction at [`AI_HOOK_VA`] (`lw v0,-0x42dc(v0)` - the ctx
/// reload the block replays first) - the build fingerprint.
pub const AI_HOOK_ORIG: u32 = lw(V0, V0, 0xBD24);
/// The word before the hook (`lui v0,0x8008`) - pinned so the hook's
/// register assumption (v0 = `0x80080000` on block entry) stays true.
pub const AI_HOOK_PREV_ORIG: u32 = lui(V0, 0x8008);
/// Stock-arm resume point (the `lbu a0,0x28a(v0)` counter load): the block
/// jumps back here for every non-course-3 context.
pub const AI_STOCK_RESUME_VA: u32 = 0x801E_B7CC;
/// The arm's join (the switch `break`): where both the special-queued and
/// no-special exits land.
pub const AI_ARM_JOIN_VA: u32 = 0x801E_BDAC;

/// The AI block's home: `FUN_80035274`, the SCUS item/equipment
/// passive-NAME draw - a real 48-instruction function with **zero
/// references of any form** in any image (five-form address-word scan;
/// `scripts/ci/port-catalog-ignore.toml` `[unreferenced]`). Overwriting an
/// unreferenced body is the only way left into always-resident SCUS: the
/// preserved rodata gap `0x8007AB38..0x8007AF00` is fully allocated across
/// the injection features, and the battle overlay's own image is packed
/// (`static-overlays.toml`: all `.text+.rodata` RAM-matched live).
pub const AI_BLOCK_VA: u32 = 0x8003_5274;
/// First stock word of the overwritten body (`addiu sp,sp,-0x20`) - the
/// build fingerprint.
pub const AI_BLOCK_ORIG_HEAD: u32 = addiu(SP, SP, 0xFFE0);
/// Second stock word (`lui v1,0x8007`) - a second fingerprint pin.
pub const AI_BLOCK_ORIG_HEAD2: u32 = lui(V1, 0x8007);
/// Capacity of the overwritten body (48 instructions).
pub const AI_BLOCK_CAPACITY: usize = 48 * 4;

/// Retimed special period mask (`% 4`) and firing phase: a sibling queues
/// its special when `(counter + offset) & 3 == 3`. With the stock counter
/// starting the first turn at 0, Lu (offset 2) fires on turns 2, 6, 10...
/// and Che (offset 0) on turns 4, 8, 12... - three regular attacks between
/// each sibling's specials, staggered two turns apart.
pub const AI_PERIOD_MASK: u16 = 3;
/// The remainder that queues the special.
pub const AI_SPECIAL_PHASE: u16 = 3;

/// The Gi round's course word: the installer bumps the packed word once per
/// round, so round 1 of course 3 is exactly `COURSE3_SEED_WORD + 1`.
pub const GI_ROUND_WORD: u32 = COURSE3_SEED_WORD + 1;
/// Divide (Green Slime's split) - a capture-class spell (module PROT 0940):
/// queueing `actor+0x1DE = 2` / `+0x1DF = 0x50` casts it through the same
/// battle-action band the Delilas signature specials ride.
pub const GI_DIVIDE_SPELL: u16 = 0x50;
/// Spore Gas (Berserker's status cloud) - capture-class, module PROT 0939.
pub const GI_SPORE_GAS_SPELL: u16 = 0x4F;
/// The turn-counter floor for Spore Gas: Gi's first pick with the shared
/// counter at/after this value casts it (one-shot via [`GI_FLAGS_VA`]).
/// A floor, not exact equality: post-Divide the round has three combatants,
/// so Gi's picks no longer land on every odd counter value and an exact
/// test could skip past its turn forever.
pub const GI_SPORE_GAS_MIN_TURN: u16 = 3;

/// The Gi arm's home: `FUN_80050d40`, the SCUS 12-bit angle tween - a
/// 48-instruction frameless leaf with **zero references of any form** in
/// any image (the same five-form sweep that cleared the other two cave
/// homes; `scripts/ci/port-catalog-ignore.toml` `[unreferenced]`). The
/// HP-gated Divide + Blazing Slash lockout grew the Gi logic past sharing
/// `FUN_80035274` with the Che & Lu arm, so it claims the third
/// unreferenced body; the main block reaches it by `j`.
pub const GI_ARM_VA: u32 = 0x8005_0D40;
/// First stock word of the overwritten tween (`andi a0,a0,0xfff`) - the
/// build fingerprint.
pub const GI_ARM_ORIG_HEAD: u32 = andi(A0, A0, 0xFFF);
/// Third stock word (`andi a1,a1,0xfff`, at `+8`) - the second fingerprint
/// pin. Word 1 is a `move` pseudo-op whose concrete encoding (`addu` vs
/// `or`) the dump does not disambiguate, so it is not pinned.
pub const GI_ARM_ORIG_WORD2: u32 = andi(A1, A1, 0xFFF);
/// Capacity of the overwritten tween body (48 instructions).
pub const GI_ARM_CAPACITY: usize = 48 * 4;

/// The Gi one-shot flags word: bit 0 = Divide cast, bit 1 = Spore Gas cast.
/// Lives in the display cave's data tail (word 8 of the overwritten
/// `FUN_800260DC` body, materialised as the trailing zero word of
/// [`assemble_display_routine`]); the Che & Lu arm zeroes it on every
/// round-0 pick, so a re-enrolled course (which always replays round 0)
/// re-arms both casts with no extra reset hook.
pub const GI_FLAGS_VA: u32 = DISPLAY_ROUTINE_VA + 8 * 4;

/// Assemble the course-3 AI main block (30 words, capacity 48): the course
/// dispatch, the Che & Lu retime, and the Gi flags reset. The Gi round's
/// own logic lives in the second cave ([`assemble_gi_arm`]), reached by `j`
/// with `a0 = word - 0x131` still live.
///
/// Entry state (from the hooked arm): `v0 = 0x80080000` (the arm's own
/// `lui`), `s4` = actor, `s7` = battle slot (the formation-cell index);
/// `v1`/`a0`/`a3` are dead (the stock arm overwrites each before reading).
/// The stock-resume path must leave `v0` = the battle ctx pointer, which
/// the replayed `lw` provides.
///
/// The round-0 arm zeroes [`GI_FLAGS_VA`] on every pick: a course always
/// enters through round 0, so the Gi one-shots are re-armed for every
/// enrollment with no dedicated reset hook.
pub fn assemble_ai_block() -> Vec<u32> {
    const CHELU: usize = 9;
    const NOSPEC: usize = 28;
    let neg_word = -(COURSE3_SEED_WORD as i32) as u32 as u16;
    let words = vec![
        lw(V0, V0, 0xBD24),                      //  0: replay: v0 = battle ctx
        lui(A3, 0x8008),                         //  1
        lw(A0, A3, lo(COURSE_WORD_VA)),          //  2: a0 = course word
        addiu(A3, A3, 0xBD0C),                   //  3: a3 = formation base (delay)
        addiu(A0, A0, neg_word),                 //  4: a0 -= 0x131
        beq(A0, ZERO, (CHELU - (5 + 1)) as i16), //  5: Che&Lu round -> local arm
        nop(),                                   //  6
        j(GI_ARM_VA),                            //  7: all else -> the Gi cave
        nop(),                                   //  8
        // CHELU (idx 9): the retime arm; first re-arm the Gi one-shots.
        lui(A0, hi(GI_FLAGS_VA)),                //  9
        sw(ZERO, A0, lo(GI_FLAGS_VA)),           // 10: flags = 0 (every pick)
        lbu(A0, V0, 0x28A),                      // 11: a0 = shared turn counter
        addu(V1, S7, A3),                        // 12
        lbu(V1, V1, 0),                          // 13: v1 = formation id
        nop(),                                   // 14: (load delay)
        andi(A3, V1, 1),                         // 15: Che(163)=1, Lu(164)=0
        xori(A3, A3, 1),                         // 16: Che=0, Lu=1
        sll(A3, A3, 1),                          // 17: Che=0, Lu=2 (stagger)
        addu(A0, A0, A3),                        // 18: counter + offset
        andi(A0, A0, AI_PERIOD_MASK),            // 19: % 4
        addiu(A3, ZERO, AI_SPECIAL_PHASE),       // 20
        bne(A0, A3, (NOSPEC - (21 + 1)) as i16), // 21: wrong phase -> no special
        nop(),                                   // 22
        addiu(V0, ZERO, 2),                      // 23
        sb(V0, S4, 0x1DE),                       // 24: actor+0x1DE = 2
        addiu(V1, V1, 0xFFD7),                   // 25: action = id - 0x29
        j(AI_ARM_JOIN_VA),                       // 26
        sb(V1, S4, 0x1DF),                       // 27: queue special (delay)
        // NOSPEC (idx 28):
        j(AI_ARM_JOIN_VA), // 28
        nop(),             // 29
    ];
    debug_assert!(words.len() * 4 <= AI_BLOCK_CAPACITY);
    words
}

/// Assemble the Gi arm (47 words, capacity 48, over `FUN_80050d40`).
/// Entered by `j` from the main block with `a0 = word - 0x131`, `v0` =
/// battle ctx, `s4` = actor, `s7` = seat; `v1`/`a3` dead both ways.
///
/// Per Gi pick (`word == 0x132`, seat 0 = the real Gi):
///
/// 1. **Divide** - queued on the first pick with HP strictly below half of
///    max (`+0x14C < +0x14E >> 1`), once ([`GI_FLAGS_VA`] bit 0).
/// 2. **Spore Gas** - queued on the first pick with the shared counter at
///    or past [`GI_SPORE_GAS_MIN_TURN`], once (bit 1); checked on both the
///    pre- and post-Divide paths so neither ordering can starve it. When a
///    pick qualifies for both casts, Divide wins and Spore Gas follows on
///    the next pick (the HP and counter conditions both persist).
/// 3. **Blazing Slash lockout** - before Divide, non-cast picks resume the
///    stock arm (the retail `% 3` Blazing Slash cadence stands); after
///    Divide, they jump straight to the arm join, so the generic
///    attack-picker runs and the special can never be queued again. The
///    Divide clone sits in a non-zero seat and always takes the join path:
///    it melees only, and can neither re-Divide nor Blazing Slash.
pub fn assemble_gi_arm() -> Vec<u32> {
    const TRYSPORE_PD: usize = 20;
    const TRYSPORE_ST: usize = 29;
    const SPOREQ: usize = 36;
    const QUEUE: usize = 39;
    const GNOSPEC: usize = 43;
    const GSTOCK: usize = 45;
    let words = vec![
        addiu(A0, A0, 0xFFFF), //                     0: a0 = word - 0x132
        bne(A0, ZERO, (GSTOCK - (1 + 1)) as i16), //  1: not the Gi round -> stock
        lui(A3, hi(GI_FLAGS_VA)), //                  2: (delay; a3 dead on stock)
        bne(S7, ZERO, (GNOSPEC - (3 + 1)) as i16), // 3: clone seat -> plain attack
        lw(V1, A3, lo(GI_FLAGS_VA)), //               4: v1 = one-shot flags (delay)
        nop(),                 //                     5: (load delay)
        andi(A0, V1, 1),       //                     6
        bne(A0, ZERO, (TRYSPORE_PD - (7 + 1)) as i16), // 7: already divided
        lhu(A0, S4, 0x14C),    //                     8: a0 = current HP (delay)
        lhu(A3, S4, 0x14E),    //                     9: a3 = max HP
        nop(),                 //                    10: (load delay)
        srl(A3, A3, 1),        //                    11: a3 = max/2
        sltu(A0, A0, A3),      //                    12: a0 = (hp < max/2)
        beq(A0, ZERO, (TRYSPORE_ST - (13 + 1)) as i16), // 13: healthy -> spore?
        nop(),                 //                    14
        ori(V1, V1, 1),        //                    15: mark Divide used
        lui(A3, hi(GI_FLAGS_VA)), //                 16: (a3 held max/2)
        sw(V1, A3, lo(GI_FLAGS_VA)), //              17
        beq(ZERO, ZERO, (QUEUE - (18 + 1)) as i16), // 18
        addiu(V1, ZERO, GI_DIVIDE_SPELL), //         19: (delay)
        // TRYSPORE_PD (20): divided - a special only if Spore Gas is owed;
        // otherwise join (Blazing Slash locked out).
        andi(A0, V1, 2),                             //                          20
        bne(A0, ZERO, (GNOSPEC - (21 + 1)) as i16),  // 21: spore done -> join
        lbu(A0, V0, 0x28A), //                       22: a0 = counter (delay)
        nop(),              //                          23: (load delay)
        sltiu(A0, A0, GI_SPORE_GAS_MIN_TURN), //     24
        bne(A0, ZERO, (GNOSPEC - (25 + 1)) as i16), // 25: too early -> join
        ori(V1, V1, 2),     //                           26: (delay; v1 dead if taken)
        beq(ZERO, ZERO, (SPOREQ - (27 + 1)) as i16), // 27
        nop(),              //                           28
        // TRYSPORE_ST (29): not divided & healthy - a special only if Spore
        // Gas is due; otherwise stock (Blazing Slash cadence stands).
        andi(A0, V1, 2),                           //                          29
        bne(A0, ZERO, (GSTOCK - (30 + 1)) as i16), // 30: spore done -> stock
        lbu(A0, V0, 0x28A),                        //                       31: (delay)
        nop(),                                     //                          32: (load delay)
        sltiu(A0, A0, GI_SPORE_GAS_MIN_TURN),      //     33
        bne(A0, ZERO, (GSTOCK - (34 + 1)) as i16), // 34: too early -> stock
        ori(V1, V1, 2), //                           35: (delay; v1 dead if taken)
        // SPOREQ (36): mark Spore Gas used, queue it.
        lui(A3, hi(GI_FLAGS_VA)),            //                 36
        sw(V1, A3, lo(GI_FLAGS_VA)),         //              37
        addiu(V1, ZERO, GI_SPORE_GAS_SPELL), //      38
        // QUEUE (39): mirror the stock arm's special-queue stores.
        addiu(V0, ZERO, 2), //                       39
        sb(V0, S4, 0x1DE),  //                       40
        j(AI_ARM_JOIN_VA),  //                       41
        sb(V1, S4, 0x1DF),  //                       42: queue the cast (delay)
        // GNOSPEC (43): no special - the generic attack-picker runs.
        j(AI_ARM_JOIN_VA), // 43
        nop(),             // 44
        // GSTOCK (45): resume the retail arm, v0 = ctx as it expects.
        j(AI_STOCK_RESUME_VA), // 45
        nop(),                 // 46
    ];
    debug_assert!(words.len() * 4 <= GI_ARM_CAPACITY);
    words
}
