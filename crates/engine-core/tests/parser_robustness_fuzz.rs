//! Parser robustness fuzz: hammer every pure format parser the engine feeds
//! untrusted disc / save bytes into with truncations, random bytes, and
//! valid-magic-plus-garbage inputs, asserting that each returns `Err` rather
//! than panicking.
//!
//! The end-user model is "ship the engine, the user supplies their own disc
//! image" - so any parser reachable from raw disc bytes must degrade to an
//! error on a bad dump, a foreign-region disc, or a corrupted save, never a
//! panic. These parsers already route reads through `legaia-bytes` checked
//! readers and bounds-check before slicing; this test is the standing guard
//! that keeps it that way. Deterministic (seeded) and disc-free, so it runs
//! in the normal `cargo test` set.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Deterministic xorshift so every run explores the same corpus.
struct Rng(u64);
impl Rng {
    fn next_u8(&mut self) -> u8 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 24) as u8
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next_u8()).collect()
    }
}

/// Builds a corpus of hostile inputs: every short length as zeros and as
/// random bytes, a wave of random buffers, and (when a `magic` prefix is
/// given) valid-magic-plus-random-tail plus magic-then-truncated inputs so
/// the parser gets past its header gate into the interesting body code.
fn corpus(magic: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut rng = Rng(0x1234_5678_9abc_def0);
    for n in 0..48usize {
        out.push(vec![0u8; n]);
        out.push(rng.bytes(n));
    }
    for _ in 0..3000 {
        let n = (rng.next_u8() as usize) * 4 + (rng.next_u8() as usize);
        out.push(rng.bytes(n));
    }
    if !magic.is_empty() {
        for _ in 0..3000 {
            let n = (rng.next_u8() as usize) * 2;
            let mut b = magic.to_vec();
            b.extend(rng.bytes(n));
            out.push(b);
        }
        for cut in 0..magic.len() + 96 {
            let mut b = magic.to_vec();
            b.extend(rng.bytes(256));
            b.truncate(cut);
            out.push(b);
        }
    }
    out
}

fn run<F: Fn(&[u8])>(name: &str, magic: &[u8], f: F) -> usize {
    let mut panics = 0;
    for input in corpus(magic) {
        if catch_unwind(AssertUnwindSafe(|| f(&input))).is_err() {
            panics += 1;
            if panics <= 3 {
                eprintln!(
                    "PANIC in {name} on input len={} head={:02X?}",
                    input.len(),
                    &input[..input.len().min(16)]
                );
            }
        }
    }
    if panics > 0 {
        eprintln!("==> {name}: {panics} panicking inputs");
    }
    panics
}

#[test]
fn parsers_never_panic_on_malformed_input() {
    let mut total = 0;

    // TMD: magic 0x80000002.
    total += run("tmd::parse", &[0x02, 0x00, 0x00, 0x80], |b| {
        let _ = legaia_tmd::parse(b);
    });
    // TIM: magic 0x00000010.
    total += run("tim::parse", &[0x10, 0x00, 0x00, 0x00], |b| {
        let _ = legaia_tim::parse(b);
    });
    total += run("tim::parse_strict", &[0x10, 0x00, 0x00, 0x00], |b| {
        let _ = legaia_tim::parse_strict(b);
    });
    // VAB: magic 'pBAV'.
    total += run("vab::parse", &[0x70, 0x42, 0x41, 0x56], |b| {
        let _ = legaia_vab::parse(b, 0);
    });
    total += run("vab::parse_header", &[0x70, 0x42, 0x41, 0x56], |b| {
        let _ = legaia_vab::parse_header(b, 0);
    });
    total += run("vab::decode_vag", &[], |b| {
        let _ = legaia_vab::decode_vag(b);
    });
    // ANM: no hard magic - a leading count word drives the table walk.
    total += run("anm::parse", &[], |b| {
        let _ = legaia_anm::parse(b);
    });
    // MES.
    total += run("mes::parse", &[], |b| {
        let _ = legaia_mes::parse(b);
    });
    // LZS container + raw decompress across a range of declared output sizes.
    total += run("lzs::parse_container", &[], |b| {
        let _ = legaia_lzs::parse_container(b);
    });
    total += run("lzs::decompress", &[], |b| {
        for sz in [0usize, 1, 64, 4096, 65536] {
            let _ = legaia_lzs::decompress(b, sz);
        }
    });
    // MDT record table.
    total += run("mdt::RecordTable::parse", &[], |b| {
        let _ = legaia_mdt::RecordTable::parse(b);
    });
    // SEQ.
    total += run("seq::parse_header", &[], |b| {
        let _ = legaia_seq::parse_header(b);
    });
    // Save formats (engine-owned, but loading a corrupted / foreign save
    // must not panic either).
    total += run("save::SaveFile::parse", b"LGSF", |b| {
        let _ = legaia_save::SaveFile::parse(b);
    });
    total += run("save::parse_card", b"MC", |b| {
        let _ = legaia_save::parse_card(b);
    });
    total += run("save::CharacterRecord::parse", &[], |b| {
        let _ = legaia_save::CharacterRecord::parse(b);
    });

    // Asset-container parsers: these read data-derived offsets out of
    // decompressed disc containers, so a truncated / hostile container must
    // not slice-panic before the offset is used.
    total += run("effect_bundle::detect", &[], |b| {
        let _ = legaia_asset::effect_bundle::detect(b);
    });
    total += run("kingdom_bundle::parse", &[], |b| {
        let _ = legaia_asset::kingdom_bundle::parse(b);
    });
    total += run("kingdom_bundle::decode_slot", &[], |b| {
        for slot in 0u8..6 {
            let _ = legaia_asset::kingdom_bundle::decode_slot(b, slot);
        }
    });
    total += run("world_map_overlay::parse", &[], |b| {
        let _ = legaia_asset::world_map_overlay::parse(b);
    });
    total += run("battle_char_palette::parse_record", &[], |b| {
        // rec0 is a caller-supplied cursor into the file; sweep several,
        // including out-of-range values.
        for rec0 in [
            0usize,
            4,
            0x20,
            b.len().saturating_sub(1),
            b.len(),
            b.len() + 64,
        ] {
            let _ = legaia_asset::battle_char_palette::parse_record(b, rec0);
        }
    });
    total += run("character_pack::parse", &[], |b| {
        let _ = legaia_asset::character_pack::parse(b);
    });
    total += run("scene_v12_table::detect", &[], |b| {
        if let Some(t) = legaia_asset::scene_v12_table::detect(b) {
            // Also exercise the offset-driven payload slicer.
            for i in 0..64 {
                let _ = t.script_payload(b, i);
            }
        }
    });
    total += run("field_pack::detect", &[], |b| {
        if let Some(fp) = legaia_asset::field_pack::detect(b) {
            for i in 0..64 {
                let _ = fp.slot_bytes_in_assets(b, i);
            }
        }
    });
    total += run("player_anm::parse", &[], |b| {
        let _ = legaia_asset::player_anm::parse(b);
    });

    assert_eq!(total, 0, "{total} parser inputs panicked (see stderr)");
}

/// Focused regression for the LGSF ext-block size handling: a crafted header
/// declaring a huge `ext_total_size` must be rejected, not slice-panic. This
/// pins the checked-arithmetic guard against the 32-bit (wasm) wrap where
/// `cursor + size` could slip past the length check into a reversed range.
#[test]
fn lgsf_hostile_ext_sizes_are_rejected_cleanly() {
    // Minimal valid v4 LGSF prelude: magic, version, story flags, money,
    // inv_count=0, party_count=0, then an LGX2 ext header with a hostile size.
    let mut v = Vec::new();
    v.extend_from_slice(b"LGSF");
    v.push(legaia_save::SAVE_FILE_VERSION); // v4
    v.extend_from_slice(&0u32.to_le_bytes()); // story flags
    v.extend_from_slice(&0i32.to_le_bytes()); // money
    v.push(0); // inv_count
    v.push(0); // party_count
    v.extend_from_slice(&legaia_save::SAVE_FILE_EXT_MAGIC); // LGX2
    let prefix = v.len();

    for size in [0u32, 1, 0x10, 0x7FFF_FFFF, 0x8000_0000, 0xFFFF_FFFF] {
        let mut b = v.clone();
        b.extend_from_slice(&size.to_le_bytes());
        // No actual ext payload - every non-zero size must be reported as
        // truncated rather than panicking.
        let r = catch_unwind(AssertUnwindSafe(|| legaia_save::SaveFile::parse(&b)));
        assert!(r.is_ok(), "SaveFile::parse panicked on ext size {size:#x}");
        if size as usize > b.len() - prefix - 4 {
            assert!(
                r.unwrap().is_err(),
                "oversized ext size {size:#x} should be rejected"
            );
        }
    }
}
