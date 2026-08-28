//! Disc oracle for the 958/960 staged-id remap: on retail images every
//! expect word matches, the apply changes exactly the edit words (rest of
//! the module byte-identical), a second apply is a clean no-op skip, a
//! partially-patched module refuses, and every touched sector stays
//! EDC/ECC-valid. Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_patcher::delilas_cast::{
    CastRoute, assemble_delilas_arena, assemble_hook, install_cast_hook, install_delilas_arena,
    install_stage_caves, install_strike_morph, patch_module_958, patch_module_960,
};
use legaia_patcher::disc::DiscPatcher;

fn load_disc() -> Option<Vec<u8>> {
    let path = std::env::var("LEGAIA_DISC_BIN").ok()?;
    if path.is_empty() {
        return None;
    }
    std::fs::read(path).ok()
}

/// Synthetic (but realistic-shaped) authored-entry offsets for the
/// un-fold cases: the module oracle only needs the encoders to be
/// checked against hand-assembled words, not the real authored layout
/// (the e2e stage test covers that composition).
const OFFS_958: [usize; 4] = [0x76A8, 0x7D24, 0x8DA4, 0x9A74];
const OFFS_960: [usize; 5] = [0x5E00, 0x6AD0, 0x7300, 0x7A00, 0x9000];

fn apply_958_fold(p: &mut DiscPatcher) -> anyhow::Result<bool> {
    patch_module_958(p, None)
}
fn apply_960_fold(p: &mut DiscPatcher) -> anyhow::Result<bool> {
    patch_module_960(p, None)
}
fn apply_958_unfold(p: &mut DiscPatcher) -> anyhow::Result<bool> {
    patch_module_958(p, Some(&OFFS_958))
}
fn apply_960_unfold(p: &mut DiscPatcher) -> anyhow::Result<bool> {
    patch_module_960(p, Some(&OFFS_960))
}

/// The un-fold stage edits for PROT 958 (hand-assembled - an
/// independent check on the encoder): staged-id literals, the five
/// staging stores as `jal`s into the caves, and the three wipe-body
/// stubs. The damage/wipe words are shared with the fold case.
const UNFOLD_958_STAGE: &[(u64, u32, u32)] = &[
    (0x0C48, 0x2442_0001, 0x2402_000A), // addiu v0,v0,1 -> li v0,0xA
    (0x19C4, 0x2442_FFFE, 0x2402_000A), // addiu v0,v0,-2 -> li v0,0xA
    (0x1FD4, 0x2402_000D, 0x2402_000B), // li v0,0xD -> li v0,0xB
    (0x05B0, 0xA242_01DA, 0x0C01_E2AA), // opener sb -> jal stub (SLOT6)
    (0x08E8, 0xA242_01DA, 0x0C01_E2AC), // s2 sb -> jal stub (SLOT6)
    (0x0C50, 0xA242_01DA, 0x0C07_E2E3), // s3 sb -> jal stub (wipe body)
    (0x19C8, 0xA242_01DA, 0x0C07_E2E5), // s4 sb -> jal stub (wipe body)
    (0x1FD8, 0xA242_01DA, 0x0C07_E2E7), // s6 sb -> jal stub (wipe body)
    // Wipe-body stubs, 2-word form (j core; ori t7 in the delay slot):
    // s3 -> slash / s4 -> crouch via shared_a, s6 -> finale via
    // shared_b. The body tail is the HP gate (shared list).
    (0x21B4, 0x3C03_8008, 0x0801_EBFE),
    (0x21B8, 0x2402_00FE, 0x340F_8DA4),
    (0x21BC, 0x3C06_8008, 0x0801_EBFE),
    (0x21C0, 0x3C05_8008, 0x340F_76A8),
    (0x21C4, 0xA062_BD71, 0x0801_EC07),
    (0x21C8, 0x90A2_BD60, 0x340F_9A74),
];

/// The un-fold stage edits for PROT 960: the four staging stores as
/// `jal`s (the folded id literals stay - they are asserted by the fold
/// case's shared list).
const UNFOLD_960_STAGE: &[(u64, u32, u32)] = &[
    (0x0C80, 0x0C01_3F32, 0x0C01_DE07), // bed jingle -> preempt cave
    (0x01C0, 0xA282_01DA, 0x0C01_E2A2), // opener sb($s4) -> jal cave
    (0x0D6C, 0xA242_01DA, 0x0C01_E2AE), // charge sb -> jal stub (SLOT6)
    (0x1094, 0xA243_01DA, 0x0C01_DE03), // channel sb($v1) -> jal stub
    (0x1108, 0xA242_01DA, 0x0C01_DE05), // strike sb -> jal stub
];

/// The fold-variant staged-walk edits for PROT 958.
const FOLD_958_STAGE: &[(u64, u32, u32)] = &[
    (0x0C48, 0x2442_0001, 0x0000_0000), // addiu v0,v0,1 -> nop
    (0x19C4, 0x2442_FFFE, 0x2442_FFFF), // addiu v0,v0,-2 -> -1
    (0x1FD4, 0x2402_000D, 0x2402_000B), // li v0,0xD -> li v0,0xB
];

/// The stage-variant-independent PROT 958 edits (damage + wipe skip).
const SHARED_958: &[(u64, u32, u32)] = &[
    // Damage retargets: arm 1 banks the victim in the cell at
    // VA 0x801F8BB4 (last dead wipe-body word), arms 1-4 move
    // from $s1, the finale arms reload the cell.
    (0x0B30, 0x3C05_801D, 0x3C05_8020), // lui a1 -> cell page
    (0x0B34, 0x8CA3_9370, 0xACB1_8BB4), // lw v1,tbl0 -> sw s1,cell
    (0x0B38, 0x0000_0000, 0x0220_1821), // nop -> move v1,s1
    (0x0B64, 0x8CA4_9370, 0x0220_2021), // lw a0,tbl0 -> move a0,s1
    (0x0E9C, 0x8D07_9370, 0x0220_3821), // lw a3,tbl0 -> move a3,s1
    (0x0ED4, 0x8D08_9370, 0x0220_4021), // lw t0,tbl0 -> move t0,s1
    (0x12EC, 0x8D07_9370, 0x0220_3821),
    (0x1324, 0x8D08_9370, 0x0220_4021),
    (0x172C, 0x8D07_9370, 0x0220_3821),
    (0x1764, 0x8D08_9370, 0x0220_4021),
    (0x1D98, 0x3C05_801D, 0x3C05_8020), // finale A: lui -> cell page
    (0x1D9C, 0x8CA3_9370, 0x8CA3_8BB4), // lw v1 <- cell
    (0x1DCC, 0x8CA5_9370, 0x8CA5_8BB4), // lw a1 <- cell
    (0x1F1C, 0x3C05_801D, 0x3C05_8020), // finale B: lui -> cell page
    (0x1F20, 0x8CA3_9370, 0x8CA3_8BB4), // lw v1 <- cell
    (0x1F50, 0x8CA4_9370, 0x8CA4_8BB4), // lw a0 <- cell
    // Dead-victim wipe skip.
    (0x21A4, 0x1040_0003, 0x0000_0000), // beqz -> nop
    // Dead-victim reaction-row wait gate (the Blazing Slash kill
    // softlock): HP into dead $t4, wait beq -> the 4-word gate cave in
    // the wipe-body tail (alive -> retail wait, dead -> convergence).
    (0x213C, 0x0000_0000, 0x962C_014C), // nop -> lhu t4, 0x14C(s1)
    (0x2140, 0x1062_0078, 0x1062_0022), // wait beq -> gate at 0x801F8BA4
    (0x21CC, 0x2403_0005, 0x1580_0055), // gate: bnez t4 -> 0x801F8CFC
    (0x21D0, 0xACC3_BD2C, 0x0000_0000), // branch delay
    (0x21D4, 0x3042_007F, 0x0807_E2EE), // dead -> j 0x801F8BB8
    (0x21D8, 0x0C00_C66A, 0x0000_0000), // j delay slot
];

/// The stage-variant-independent PROT 960 edits (fold id literals -
/// shared with the un-fold, damage, wipe, hazard nops, teardown).
const SHARED_960: &[(u64, u32, u32)] = &[
    // Staged-walk fold + confirmation gate (shared by fold AND
    // un-fold - 960's id literals never change again).
    (0x0D68, 0x2402_000E, 0x2402_000A), // li v0,0xE -> li v0,0xA
    (0x1090, 0x2403_000C, 0x2403_000A), // li v1,0xC -> li v1,0xA
    (0x1104, 0x2402_000D, 0x2402_000A), // li v0,0xD -> li v0,0xA
    (0x1834, 0x2402_000F, 0x2402_000B), // li v0,0xF -> li v0,0xB
    (0x118C, 0x2402_000D, 0x2402_000A), // mp5 played-id gate follows
    // mp5 hold: cursor gate -> deterministic tick counter in the dead
    // wipe-body word 0x801F85B0 (self-resets on module re-stream).
    (0x1198, 0x8E42_022C, 0x3C03_8020), // lw v0,0x22C(s2) -> lui v1,0x8020
    (0x119C, 0x0000_0000, 0x8C64_85B0), // nop -> lw a0,-0x7A50(v1)
    (0x11A0, 0x8442_0068, 0x0000_0000), // lh v0,0x68(v0) -> nop (load delay)
    (0x11A4, 0x0000_0000, 0x2484_0001), // nop -> addiu a0,a0,1
    (0x11A8, 0x2842_0090, 0x2882_001E), // slti v0,v0,0x90 -> slti v0,a0,0x1E
    (0x11B0, 0x03C0_1021, 0xAC64_85B0), // move v0,fp -> sw a0,-0x7A50(v1)
    (0x1BD8, 0xA0A2_BD60, 0x0000_0000), // dead word -> counter cell
    // Damage retargets (victim lives in $s3 tick-wide).
    (0x17AC, 0x8EC5_9370, 0x0260_2821), // lw a1,tbl0 -> move a1,s3
    (0x17DC, 0x8EC6_9370, 0x0260_3021), // lw a2,tbl0 -> move a2,s3
    // Dead-victim wipe skip.
    (0x1B88, 0x1040_0009, 0x0000_0000), // beqz -> nop
    // Seat-3 record-toggle stores neutralised.
    (0x1230, 0xA440_000C, 0x0000_0000), // sh zero,0xC(v0) -> nop
    (0x1C20, 0xA445_000C, 0x0000_0000), // sh a1,0xC(v0) -> nop
    // Finale teardown: settle exit rerouted through the dead
    // wipe body, which neutralises the cached finale entity.
    (0x1BA8, 0x0807_E170, 0x0807_E162), // j settle -> j wipe body
    (0x1BB0, 0x3C03_8008, 0x8E86_102C), // lw a2, 0x102C(s4)
    (0x1BB4, 0x2402_00FE, 0x0000_0000), // load delay
    (0x1BB8, 0x3C06_8008, 0x10C0_0005), // beqz a2 -> guard target
    (0x1BBC, 0x3C05_8008, 0xAE80_102C), // sw zero, 0x102C(s4)
    (0x1BC0, 0xA062_BD71, 0xACC0_0010), // sw zero, 0x10(a2)
    (0x1BC4, 0x90A2_BD60, 0xACC0_0014), // sw zero, 0x14(a2)
    (0x1BC8, 0x2403_0005, 0x0807_E170), // j phase-0xFF write
    (0x1BCC, 0xACC3_BD2C, 0x0000_0000), // j delay slot
    (0x1BD0, 0x3042_007F, 0x0807_E170), // guard target: j rejoin
    (0x1BD4, 0x0C00_C66A, 0x0000_0000), // j delay slot
];

fn word(entry: &[u8], off: u64) -> u32 {
    let off = off as usize;
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

/// `(entry, apply, expected per-offset old->new words)` for one case.
type Case = (
    usize,
    fn(&mut DiscPatcher) -> anyhow::Result<bool>,
    Vec<(u64, u32, u32)>,
);

/// The four module cases: each stage variant composed with its module's
/// shared edit list.
fn cases() -> Vec<Case> {
    vec![
        (958, apply_958_fold, [FOLD_958_STAGE, SHARED_958].concat()),
        (960, apply_960_fold, SHARED_960.to_vec()),
        (
            958,
            apply_958_unfold,
            [UNFOLD_958_STAGE, SHARED_958].concat(),
        ),
        (
            960,
            apply_960_unfold,
            [UNFOLD_960_STAGE, SHARED_960].concat(),
        ),
    ]
}

#[test]
fn stage_remap_folds_ids_into_the_player_rows() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    for (prot, apply, edits) in cases() {
        let edits: &[(u64, u32, u32)] = &edits;
        let retail = DiscPatcher::open(original.clone()).expect("open retail");
        let retail_entry = retail.read_entry(prot).expect("read retail module");
        for &(off, expect, _) in edits {
            assert_eq!(
                word(&retail_entry, off),
                expect,
                "PROT {prot} +{off:#x}: retail word"
            );
        }

        let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
        assert!(apply(&mut patcher).expect("apply"), "PROT {prot}: applied");
        let image = patcher.into_image();
        assert_eq!(image.len(), original.len(), "image length preserved");

        // Re-open validates EDC/ECC on every touched sector.
        let mut reopened = DiscPatcher::open(image).expect("re-open patched");
        let live = reopened.read_entry(prot).expect("read patched module");
        let mut expected = retail_entry.clone();
        for &(off, _, replace) in edits {
            assert_eq!(word(&live, off), replace, "PROT {prot} +{off:#x}: patched");
            expected[off as usize..off as usize + 4].copy_from_slice(&replace.to_le_bytes());
        }
        assert_eq!(live, expected, "PROT {prot}: only the edit words changed");

        // Idempotence: a second apply is a clean skip.
        assert!(
            !apply(&mut reopened).expect("re-apply"),
            "PROT {prot}: second apply skips"
        );

        // A partial patch (one edit word already replaced) refuses.
        let mut partial = DiscPatcher::open(original.clone()).expect("open disc");
        let (off, _, replace) = edits[0];
        partial
            .patch_prot_entry(prot, off, &replace.to_le_bytes())
            .expect("hand-patch one site");
        assert!(
            apply(&mut partial).is_err(),
            "PROT {prot}: partially-patched module must refuse"
        );
    }
}

/// The SCUS-resident stage-cave code: written into the injection-gap
/// tail + shiny-seru's option-exclusive ARENA2/SLOT6 pockets, every
/// word matching the hand-assembled sequences below, idempotent on
/// re-apply, and refused when a pool byte is already owned.
#[test]
fn stage_caves_land_in_the_scus_pools() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    // (VA, hand-assembled words) - independent of the crate's encoders.
    const PIECES: &[(u32, &[u32])] = &[
        // 960: 9-word shared core (repoint + entry +0x88 stream ptr +
        // staging store) + two stubs in the SCUS gap tail
        // (0x80077728 + 192-byte hook), opener cave + third stub in SLOT6.
        (
            0x800777E8,
            &[
                0x3C18_801D, // lui t8, 0x801D
                0x8F18_9360, // lw  t8, -0x6CA0(t8)   ; DAT_801C9360[0]
                0x240E_000A, // li  t6, 0xA           ; load-delay filler
                0x030F_C821, // addu t9, t8, t7
                0x272D_00AC, // addiu t5, t9, 0xAC
                0xAF2D_0088, // sw  t5, 0x88(t9)      ; entry stream ptr
                0xAF19_0028, // sw  t9, 0x28(t8)      ; row 0x0A word
                0x03E0_0008, // jr  ra
                0xA24E_01DA, // sb  t6, 0x1DA(s2)     ; delay slot
            ],
        ),
        // 2-word stubs: j core with the ori in the delay slot.
        (0x8007780C, &[0x0801_DDFA, 0x340F_7300]), // channel
        (0x80077814, &[0x0801_DDFA, 0x340F_7A00]), // strike
        // Bed-preempt cave half A (gap tail): a0=slot 0x13, a1=chan 2,
        // tail into half B.
        (0x8007781C, &[0x2404_0013, 0x0801_E2B0, 0x2405_0002]),
        (0x80078AB8, &[0x0801_DDFA, 0x340F_6AD0]), // charge
        // Bed-preempt cave half B (SLOT6): guard-free XA play tail-call,
        // a2 = dur 0x3F6 (16.9 s).
        (0x80078AC0, &[0x0800_F54F, 0x2406_03F6]),
        (
            0x80078A88, // 960 opener (caster in $s4), offset inline; no
            // +0x88 write - the chain head is table-bound (loader wrote it)
            &[
                0x3C18_801D,
                0x8F18_9360,
                0x340F_5E00, // ori t7, zero, raise-entry offset
                0x030F_C821,
                0x240E_000A,
                0xAF19_0028,
                0x03E0_0008,
                0xA28E_01DA, // sb t6, 0x1DA(s4)
            ],
        ),
        // 958: two 9-word shared cores filling ARENA2 exactly, opener +
        // s2-reset stubs in SLOT6 (the other three stubs live in the
        // module's wipe body).
        (
            0x8007AFF8, // shared A: row 0x0A word, id 0xA
            &[
                0x3C18_801D,
                0x8F18_9364, // DAT_801C9360[1] (Noa)
                0x240E_000A,
                0x030F_C821,
                0x272D_00AC,
                0xAF2D_0088,
                0xAF19_0028,
                0x03E0_0008,
                0xA24E_01DA,
            ],
        ),
        (
            0x8007B01C, // shared B: row 0x0B word, id 0xB
            &[
                0x3C18_801D,
                0x8F18_9364,
                0x240E_000B,
                0x030F_C821,
                0x272D_00AC,
                0xAF2D_0088,
                0xAF19_002C,
                0x03E0_0008,
                0xA24E_01DA,
            ],
        ),
        (0x80078AA8, &[0x0801_EBFE, 0x340F_76A8]), // opener
        (0x80078AB0, &[0x0801_EC07, 0x340F_7D24]), // s2 reset
    ];

    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    assert!(
        install_stage_caves(&mut patcher, Some(&OFFS_958), Some(&OFFS_960)).expect("install"),
        "caves written"
    );
    let image = patcher.into_image();
    let mut reopened = DiscPatcher::open(image).expect("re-open patched");
    let scus = reopened.read_named_file("SCUS_942.54").expect("read SCUS");
    for &(va, words) in PIECES {
        let off = legaia_asset::item_names::file_offset_for_va(&scus, va).expect("VA offset");
        for (k, &w) in words.iter().enumerate() {
            let got = u32::from_le_bytes(scus[off + k * 4..off + k * 4 + 4].try_into().unwrap());
            assert_eq!(got, w, "cave word {va:#010x}+{:#x}", k * 4);
        }
    }

    // Idempotent skip.
    assert!(
        !install_stage_caves(&mut reopened, Some(&OFFS_958), Some(&OFFS_960)).expect("re-apply"),
        "second install skips"
    );
    // Different offsets = a different build's caves: refused.
    let other: [usize; 5] = [0x5E04, 0x6AD4, 0x7304, 0x7A04, 0x9004];
    assert!(
        install_stage_caves(&mut reopened, Some(&OFFS_958), Some(&other)).is_err(),
        "pool with someone else's bytes refuses"
    );
    // Nothing un-folded = no writes.
    let mut clean = DiscPatcher::open(original).expect("open disc");
    assert!(
        !install_stage_caves(&mut clean, None, None).expect("no-op"),
        "no un-fold, no writes"
    );
}

/// The leading-arts machinery: the ARENA1 image (queue-edit + strike
/// morph) lands byte-exact at `0x8007AE00`, the 0898 strike-fetch
/// detour rewrites exactly its two words (`jal` + the fetch riding the
/// delay slot), both installs are idempotent, and the arena claim
/// refuses a dirty arena.
#[test]
fn arena_and_strike_morph_land() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const ARENA1_VA: u32 = 0x8007_AE00;
    const STRIKE_FETCH_OFF: usize = 0x14F34; // 0x801E374C - 0x801CE818
    let routes = vec![
        CastRoute {
            char_index: 0,
            art_constant: 0x1C,
            spell_id: 0x7B,
        },
        CastRoute {
            char_index: 1,
            art_constant: 0x1F,
            spell_id: 0x79,
        },
        CastRoute {
            char_index: 2,
            art_constant: 0x1C,
            spell_id: 0x7A,
        },
    ];

    let mut p = DiscPatcher::open(original.clone()).expect("patcher");
    install_cast_hook(&mut p, &routes).expect("hook installs");
    assert!(install_delilas_arena(&mut p).expect("arena installs"));
    assert!(install_strike_morph(&mut p).expect("morph installs"));

    // The arena bytes are exactly the assembly (stub table + DONE label
    // derived from the fixed-shape hook at the SCUS gap).
    const SCUS_GAP_VA: u32 = 0x8007_7728;
    let code_len = (assemble_hook(SCUS_GAP_VA, &[]).len() - 8) as u32;
    let want = assemble_delilas_arena(SCUS_GAP_VA + code_len - 16, SCUS_GAP_VA + code_len);
    let scus = p.read_named_file("SCUS_942.54").expect("scus");
    let off = legaia_asset::item_names::file_offset_for_va(&scus, ARENA1_VA).expect("va");
    assert_eq!(&scus[off..off + want.len()], want.as_slice(), "arena bytes");

    // The 0898 detour words.
    let entry = p.read_entry(898).expect("prot 898");
    let w = |o: usize| u32::from_le_bytes(entry[o..o + 4].try_into().unwrap());
    let morph_va = ARENA1_VA + 35 * 4;
    assert_eq!(
        w(STRIKE_FETCH_OFF),
        0x0C00_0000 | ((morph_va & 0x0FFF_FFFF) >> 2),
        "jal into the morph"
    );
    assert_eq!(
        w(STRIKE_FETCH_OFF + 4),
        0x9043_01DF,
        "fetch rides the delay"
    );
    // Retail words held before the edit.
    let retail = DiscPatcher::open(original.clone()).expect("patcher");
    let rentry = retail.read_entry(898).expect("prot 898");
    let rw = |o: usize| u32::from_le_bytes(rentry[o..o + 4].try_into().unwrap());
    assert_eq!(rw(STRIKE_FETCH_OFF), 0x9043_01DF);
    assert_eq!(rw(STRIKE_FETCH_OFF + 4), 0x9262_01DC);

    // Idempotence.
    assert!(!install_delilas_arena(&mut p).expect("arena re-apply"));
    assert!(!install_strike_morph(&mut p).expect("morph re-apply"));

    // A dirty arena refuses.
    let mut dirty = DiscPatcher::open(original).expect("patcher");
    let scus0 = dirty.read_named_file("SCUS_942.54").expect("scus");
    let off0 = legaia_asset::item_names::file_offset_for_va(&scus0, ARENA1_VA).expect("va");
    dirty
        .patch_named_file("SCUS_942.54", off0 as u64, &[0xAA])
        .expect("dirty byte");
    assert!(
        install_delilas_arena(&mut dirty).is_err(),
        "dirty arena bails"
    );
}
