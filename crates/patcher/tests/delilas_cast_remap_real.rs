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
            (0x0C48, 0x2442_0001, 0x0000_0000), // addiu v0,v0,1 -> nop
            (0x19C4, 0x2442_FFFE, 0x2442_FFFF), // addiu v0,v0,-2 -> -1
            (0x1FD4, 0x2402_000D, 0x2402_000B), // li v0,0xD -> li v0,0xB
        ],
    ),
    (
        960,
        patch_module_960,
        &[
            (0x0D68, 0x2402_000E, 0x2402_000A), // li v0,0xE -> li v0,0xA
            (0x1090, 0x2403_000C, 0x2403_000A), // li v1,0xC -> li v1,0xA
            (0x1104, 0x2402_000D, 0x2402_000A), // li v0,0xD -> li v0,0xA
            (0x1834, 0x2402_000F, 0x2402_000B), // li v0,0xF -> li v0,0xB
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
