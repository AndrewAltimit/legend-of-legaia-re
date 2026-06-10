//! Disc-gated: the per-scene sound pack (`sound_data2` / `.dpk`) decodes as a
//! **VAB + SEQ bundle**, resolving the open "what is the type-1 chunk payload"
//! question. The type-2-terminated streaming container holds chunk type 0 =
//! VAB header section (`pBAV`), chunk type 1 = VAB sample section (the
//! SPU-ADPCM waveform pool), chunk type 2 = SEQ (`pQES`, the terminator); and
//! type-0 + type-1 reconstitute one VAB whose declared `total_size` (`+0x0C`)
//! equals `chunk0.size + chunk1.size`.
//!
//! Asserts, for the clean `sound_data2` entries (`0877`..=`0885`), that
//! [`sound_pack::extract`] returns a VAB + SEQ, that the reconstituted VAB
//! parses with the real `legaia_vab` header parser (declared `fsize` ==
//! concatenated chunk sizes), and that the SEQ parses with `legaia_seq`.
//!
//! Non-vacuous: requires at least 3 clean entries to decode. Skips silently
//! when the disc / extracted `PROT.DAT` is absent.

use std::path::PathBuf;

use legaia_asset::sound_pack;
use legaia_prot::archive::Archive;

/// `sound_data2` PROT band that carries the clean `[VAB][samples][SEQ]` shape.
const SOUND_DATA2_CLEAN: std::ops::RangeInclusive<usize> = 877..=885;

fn prot_path() -> Option<PathBuf> {
    for base in ["extracted", "../../extracted"] {
        let prot = PathBuf::from(base).join("PROT.DAT");
        if prot.is_file() {
            return Some(prot);
        }
    }
    None
}

#[test]
fn sound_data2_decodes_as_vab_plus_seq() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot) = prot_path() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };

    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let mut buf = Vec::new();
    let mut decoded = 0usize;

    for idx in SOUND_DATA2_CLEAN {
        let Some(entry) = archive.entries.get(idx).cloned() else {
            continue;
        };
        archive.read_entry(&entry, &mut buf).expect("read entry");

        let pack = sound_pack::extract(&buf)
            .unwrap_or_else(|| panic!("PROT {idx} (sound_data2) should decode as a sound pack"));

        // The VAB header parses, and its declared total_size matches the
        // concatenated header+sample chunk bytes (the decisive invariant).
        assert!(
            pack.vab_complete,
            "PROT {idx}: VAB sample section is complete"
        );
        assert_eq!(
            pack.vab.len(),
            pack.vab_total_size as usize,
            "PROT {idx}: reconstituted VAB length == declared total_size"
        );
        let header = legaia_vab::parse_header(&pack.vab, 0)
            .unwrap_or_else(|e| panic!("PROT {idx}: reconstituted VAB header parse: {e}"));
        assert_eq!(
            header.fsize as usize,
            pack.vab.len(),
            "PROT {idx}: VAB header fsize == reconstituted length"
        );
        assert!(
            header.ps > 0 && header.ps <= 128,
            "PROT {idx}: plausible VAB program count ({})",
            header.ps
        );

        // The terminator is a real SEQ.
        let seq = pack
            .seq
            .as_ref()
            .unwrap_or_else(|| panic!("PROT {idx}: terminator should be a SEQ"));
        legaia_seq::Seq::parse(seq).unwrap_or_else(|e| panic!("PROT {idx}: SEQ parse: {e}"));

        decoded += 1;
    }

    eprintln!("[sound-pack] {decoded} sound_data2 entries decoded as VAB+SEQ");
    assert!(
        decoded >= 3,
        "expected >= 3 clean sound_data2 packs to decode (got {decoded})"
    );
}
