//! Disc oracle for the 958/960 staged-id remap: on retail images every
//! expect word matches, the apply changes exactly the edit words (rest of
//! the module byte-identical), a second apply is a clean no-op skip, a
//! partially-patched module refuses, and every touched sector stays
//! EDC/ECC-valid. Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_patcher::delilas_cast::{patch_module_958, patch_module_960};
use legaia_patcher::disc::DiscPatcher;

fn load_disc() -> Option<Vec<u8>> {
    let path = std::env::var("LEGAIA_DISC_BIN").ok()?;
    if path.is_empty() {
        return None;
    }
    std::fs::read(path).ok()
}

/// `(entry, expected per-offset old->new words)` for one module.
type Case = (
    usize,
    fn(&mut DiscPatcher) -> anyhow::Result<bool>,
    &'static [(u64, u32, u32)],
);

const CASES: &[Case] = &[
    (
        958,
        patch_module_958,
        &[
            // Staged-walk fold.
            (0x0C48, 0x2442_0001, 0x0000_0000), // addiu v0,v0,1 -> nop
            (0x19C4, 0x2442_FFFE, 0x2442_FFFF), // addiu v0,v0,-2 -> -1
            (0x1FD4, 0x2402_000D, 0x2402_000B), // li v0,0xD -> li v0,0xB
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
        ],
    ),
    (
        960,
        patch_module_960,
        &[
            // Staged-walk fold + confirmation gate.
            (0x0D68, 0x2402_000E, 0x2402_000A), // li v0,0xE -> li v0,0xA
            (0x1090, 0x2403_000C, 0x2403_000A), // li v1,0xC -> li v1,0xA
            (0x1104, 0x2402_000D, 0x2402_000A), // li v0,0xD -> li v0,0xA
            (0x1834, 0x2402_000F, 0x2402_000B), // li v0,0xF -> li v0,0xB
            (0x118C, 0x2402_000D, 0x2402_000A), // mp5 played-id gate follows
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
        ],
    ),
];

fn word(entry: &[u8], off: u64) -> u32 {
    let off = off as usize;
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

#[test]
fn stage_remap_folds_ids_into_the_player_rows() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };

    for &(prot, apply, edits) in CASES {
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
