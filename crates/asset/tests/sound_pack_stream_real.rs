//! Disc-gated: the per-scene sound pack (`sound_data2` / `.dpk`) is a
//! type-`0x02`-terminated streaming-chunk container, the format
//! [`FUN_8001FE70`] consumes (the buffer `FUN_8001FA88` fills and
//! `FUN_800513F0` then walks). It shares the `(type << 24) | size` chunk
//! header with the zero-size-terminated DATA_FIELD walker (`FUN_8002541C`,
//! reached via [`legaia_asset::parse_streaming`]) but ends on a type-2 chunk
//! whose `size` is non-zero - so the DATA_FIELD walker mis-reads it.
//!
//! Asserts the real PROT `sound_data2` entry walks cleanly under
//! [`StreamTerminator::TypeTwo`] and that the zero-size rule does NOT stop at
//! the same point (the regression that motivates the variant). Skips silently
//! when the disc / extracted `PROT.DAT` is absent.

use std::path::PathBuf;

use legaia_asset::{StreamTerminator, parse_streaming, parse_streaming_with};
use legaia_prot::archive::Archive;

/// First `sound_data2` PROT entry (`CDNAME.TXT`: `#define sound_data2 877`).
const SOUND_DATA2_INDEX: usize = 877;

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
fn sound_data2_is_a_type2_terminated_chunk_stream() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot) = prot_path() else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };

    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(SOUND_DATA2_INDEX)
        .cloned()
        .expect("sound_data2 entry present");
    let mut buf = Vec::new();
    archive
        .read_entry(&entry, &mut buf)
        .expect("read sound_data2 entry");
    assert!(buf.len() > 64, "sound_data2 entry is non-trivial");

    // The FUN_8001FE70 walker terminates cleanly on a type-2 chunk.
    let r = parse_streaming_with(&buf, 4096, StreamTerminator::TypeTwo).expect("type2 walk");
    assert!(
        r.terminated,
        "sound_data2 terminates on a type-2 chunk under the FUN_8001FE70 rule"
    );
    assert!(r.chunks.len() >= 2, "more than just a terminator");
    let last = r.chunks.last().expect("at least one chunk");
    assert_eq!(last.type_byte, 0x02, "the final chunk is the type-2 marker");
    assert!(
        last.size > 0,
        "the terminator chunk carries a non-zero body (so the zero-size rule \
         can't see it) - the whole reason this variant exists"
    );
    assert!(
        r.bytes_consumed <= buf.len(),
        "the walk stays inside the entry footprint"
    );
    // Every non-terminator chunk is type 0 or 1 (the lead + FUN_800198E0 body).
    for c in &r.chunks[..r.chunks.len() - 1] {
        assert!(
            c.type_byte == 0x00 || c.type_byte == 0x01,
            "interior chunk type {:#x} is one of {{0,1}}",
            c.type_byte
        );
    }

    // The zero-size-terminated DATA_FIELD walker does NOT stop at the type-2
    // chunk: it either keeps walking past it or bails on a bounds violation,
    // consuming a different number of bytes. This is the regression the
    // TypeTwo variant fixes.
    let z = parse_streaming(&buf, 4096).expect("zero-size walk");
    assert_ne!(
        z.bytes_consumed, r.bytes_consumed,
        "the zero-size rule mis-walks the type-2-terminated stream"
    );
}
