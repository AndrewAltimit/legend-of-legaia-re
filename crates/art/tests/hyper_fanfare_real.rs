//! Validate the fanfare duration table ([`legaia_art::hyper_fanfare`]) against
//! the real `SCUS_942.54`: for every capture-witnessed fanfare cue, the
//! `0x800788B8` table's `(entry * 0x3C + 99) / 100` arithmetic must reproduce
//! the `dur` argument the live cue staged. Skips and passes when neither
//! `extracted/SCUS_942.54` nor `LEGAIA_DISC_BIN` is available - the same
//! gating pattern as the other disc-dependent tests.

use legaia_art::hyper_fanfare::{
    CAPTURED_FANFARES, FanfareDurTable, generic_fanfare_id, hyper_fanfare, jingle_decode,
};
use std::path::PathBuf;

/// `extracted/SCUS_942.54` if present, else sliced out of `LEGAIA_DISC_BIN`.
fn scus_bytes() -> Option<Vec<u8>> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(workspace) = manifest.parent().and_then(|p| p.parent()) {
        let p = workspace.join("extracted").join("SCUS_942.54");
        if p.is_file() {
            return std::fs::read(&p).ok();
        }
    }
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let image = std::fs::read(disc).ok()?;
    let (lba, size) = legaia_iso::iso9660::find_file_in_image(&image, "SCUS_942.54")?;
    // Form-1 user data: 2048 bytes at offset 24 of each 2352-byte sector.
    let sectors = size.div_ceil(2048) as usize;
    let mut out = Vec::with_capacity(sectors * 2048);
    for i in 0..sectors {
        let base = (lba as usize + i) * 2352 + 24;
        out.extend_from_slice(image.get(base..base + 2048)?);
    }
    out.truncate(size as usize);
    Some(out)
}

#[test]
fn captured_fanfare_durs_reproduce_the_scus_table_or_skip() {
    let Some(scus) = scus_bytes() else {
        eprintln!("no extracted/SCUS_942.54 and no LEGAIA_DISC_BIN - skipping");
        return;
    };
    let table = FanfareDurTable::parse_from_scus(&scus).expect("parse fanfare dur table");

    // (jingle id, capture-witnessed FUN_8003D53C dur) - frame-tagged recomp
    // captures off the cue globals (scripts/recomp/xa_cue_capture.py).
    let witnessed: &[(u16, u32)] = &[
        (0x102, 0x165), // Vahn Tornado Flame, chan 2
        (0x105, 0x165), // Vahn Tornado Flame, chan 5 (repeat fire)
        (0x106, 0x1A7), // Vahn Fire Blow, chan 6
        (0x104, 0x1A7), // Vahn Burning Flare, chan 4
        (0x112, 0x16E), // Noa Frost Breath, chan 2 (two fires, same dur)
        (0x113, 0x13B), // Noa Vulture Blade, chan 3
        (0x114, 0x186), // Noa Hurricane Kick, chan 4
        (0x123, 0x165), // Gala Lightning Storm, chan 3
        (0x125, 0x143), // Gala Thunder Punch, chan 5
        (0x127, 0x15A), // Gala Explosive Fist, chan 7
        (0x101, 0xB4),  // Vahn generic (Super) fanfare, chan 1
        (0x111, 0xB4),  // Noa generic (Super) fanfare, chan 1
        (0x121, 0xB4),  // Gala generic (Miracle) fanfare, chan 1
        (0x12D, 0xDE),  // Gala Miracle finisher cue-track id (XA29 chan 5)
    ];
    for &(id, dur) in witnessed {
        assert_eq!(
            table.dur(id),
            Some(dur),
            "jingle id {id:#X}: table arithmetic vs captured dur"
        );
    }

    // Every witnessed per-art channel resolves to a jingle id whose decode
    // lands in the character's own even fanfare clip slot.
    for c in CAPTURED_FANFARES {
        let row = hyper_fanfare(c.cslot as usize, c.action_constant).expect("selector row");
        let (a, _) = row.channel_pair();
        let id = row.base_id + u16::from(c.channel - a) / 3 * 3;
        let (clip, chan) = jingle_decode(id).expect("decode");
        assert_eq!(clip, c.cslot * 2);
        assert_eq!(chan, c.channel);
    }
    for cslot in 0..3usize {
        let id = generic_fanfare_id(cslot).unwrap();
        assert_eq!(table.dur(id), Some(0xB4), "generic fanfare dur");
    }
}
