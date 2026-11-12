//! `PORT:`-tagged routines the interpreter cannot compare, each with the
//! reason, held to one fact the disc can check: the tagged address is the
//! first instruction of a routine in the image named (a frame prologue
//! `addiu sp, sp, -n`, or for a leaf the instruction the tag's doc names).

use crate::common::disc;

/// `(PROT entry or None for SCUS_942.54, VA, frame prologue expected, why)`.
const UNDRIVEN: [(Option<usize>, u32, bool, &str); 5] = [
    (
        None,
        0x8001_5E90,
        true,
        "the boot `main`: its mode-table loop never returns, and the port is a seat struct with no kernel of its own",
    ),
    (
        None,
        0x8002_4D78,
        false,
        "REPLACED-BY: the actor's OBJECT table has no port counterpart; every renderer walks the bound TMD",
    ),
    (
        None,
        0x8001_6444,
        // `lw v0,0x3b8(gp)` sits ahead of the frame prologue.
        false,
        "the per-frame game loop: five actor-list tick passes, five GPU render passes and the display flip; the port keeps only the pass order, its passes are the World tick's loops and the host renderer",
    ),
    (
        Some(970),
        0x801C_EA3C,
        true,
        "the STR overlay's entry: display, CD and MDEC setup around the play loop",
    ),
    (
        Some(970),
        0x801C_F098,
        true,
        "the STR play loop busy-waits on the CD stream and the MDEC DMA; its pieces run in `str_player`",
    ),
];

#[test]
fn the_undriven_ports_name_routine_entries() {
    not_a_differential!(
        "reads the entry instruction of each PORT routine the interpreter cannot drive; runs no routine"
    );
    let Some(d) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = d.read_named_file("SCUS_942.54").expect("SCUS_942.54");
    for (prot, va, prologue, why) in UNDRIVEN {
        assert!(!why.is_empty());
        let (image, base, header) = match prot {
            None => (scus.clone(), 0x8001_0000u32, 0x800usize),
            Some(n) => (d.read_entry(n).expect("entry"), 0x801C_E818, 0),
        };
        let o = header + (va - base) as usize;
        let word = u32::from_le_bytes(image[o..o + 4].try_into().unwrap());
        if prologue {
            // `addiu sp, sp, -n`.
            assert_eq!(word >> 16, 0x27BD, "{va:#010x}: {word:#010x}");
            assert!(word & 0x8000 != 0, "{va:#010x}: a frame grows down");
        } else {
            // A leaf: the instruction before it ends the previous routine
            // (`jr ra`'s delay slot), so the tag is not interior.
            let prev = u32::from_le_bytes(image[o - 8..o - 4].try_into().unwrap());
            assert_eq!(
                prev, 0x03E0_0008,
                "{va:#010x}: the routine before ends with jr ra"
            );
        }
    }
}
