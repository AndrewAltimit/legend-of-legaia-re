//! The production LZS decoder against the reference ring decoder over the
//! disc's streams: equal bytes, equal `consumed`, equal error text.
//!
//! The census is the byte accounting's: every PROT entry is accounted with
//! nesting on, and each decode a walker asks for is compared, the failed ones
//! included (a scanner's decode of the wrong bytes is the ordinary case). Each
//! entry is also tried as an LZS container and read as a stream outright. Each
//! decode that succeeds is then re-run truncated and stopped short, which on
//! real streams lands inside copies and across the ring's wrap.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset or `extracted/PROT` is
//! missing.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use legaia_disc_audit::byte_account::{AccountOptions, account, set_lzs_observer};

#[path = "../../lzs/src/reference_decoder.rs"]
mod reference_decoder;

fn prot_entries() -> Option<Vec<PathBuf>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = manifest.parent()?.parent()?.join("extracted").join("PROT");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| Some(e.ok()?.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "BIN"))
        .collect();
    paths.sort();
    (!paths.is_empty()).then_some(paths)
}

/// One decode, on both decoders. The decoded result, for the follow-up cases.
fn compare(input: &[u8], size: usize, origin: &str) -> Option<(Vec<u8>, usize)> {
    let new = psx_lzs::decompress_tracked(input, size).map_err(|e| e.to_string());
    let old = reference_decoder::decode(input, size);
    if new != old {
        let first = match (&new, &old) {
            (Ok((n, _)), Ok((o, _))) => n.iter().zip(o).position(|(a, b)| a != b),
            _ => None,
        };
        panic!(
            "{origin}: decoders differ, {} input bytes, size {size}, first byte {first:?}, \
             consumed {:?} and {:?}, errors {:?} and {:?}",
            input.len(),
            new.as_ref().map(|r| r.1).ok(),
            old.as_ref().map(|r| r.1).ok(),
            new.as_ref().err(),
            old.as_ref().err(),
        );
    }
    old.ok()
}

/// An input and the size asked of it.
type Request = (Vec<u8>, usize);

#[derive(Default)]
struct Tally {
    ok: usize,
    failed: usize,
    bytes: usize,
    wrapped: usize,
}

/// A stream the census found, and its truncated and stopped-short relatives.
fn compare_family(input: &[u8], size: usize, origin: &str, tally: &mut Tally) {
    let Some((_, consumed)) = compare(input, size, origin) else {
        tally.failed += 1;
        return;
    };
    tally.ok += 1;
    tally.bytes += size;
    tally.wrapped += usize::from(size > 0x1000);
    let stream = &input[..consumed];
    for cut in [
        consumed / 2,
        consumed.saturating_sub(2),
        consumed.saturating_sub(1),
    ] {
        compare(&stream[..cut], size, origin);
    }
    for stop in [
        1,
        2,
        3,
        size / 2 + 1,
        size.saturating_sub(1),
        size + 1,
        size + 19,
    ] {
        compare(stream, stop, origin);
    }
}

#[test]
fn decoders_agree_on_every_lzs_stream_on_the_disc() {
    let Some(entries) = prot_entries() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT missing");
        return;
    };
    let census = Rc::new(RefCell::new(Tally::default()));
    let current = Rc::new(RefCell::new(String::new()));
    {
        let (census, current) = (census.clone(), current.clone());
        set_lzs_observer(Some(Box::new(move |input, size| {
            compare_family(input, size, &current.borrow(), &mut census.borrow_mut());
        })));
    }
    let mut tried = Tally::default();
    for path in &entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(path).unwrap();
        *current.borrow_mut() = name.clone();
        let opts = AccountOptions {
            prot_index: name.split('_').next().and_then(|n| n.parse().ok()),
            label: name.clone(),
            depth: 4,
            ..AccountOptions::default()
        };
        account(&bytes, &opts);
        // Beyond what the walkers recognise: the same decode a detector tries.
        if let Ok(container) = psx_lzs::parse_container(&bytes) {
            for s in &container.sections {
                let input = &bytes[s.byte_offset as usize..];
                compare_family(input, s.size as usize, &name, &mut tried);
            }
        }
        compare_family(&bytes, bytes.len() * 2, &name, &mut tried);
    }
    set_lzs_observer(None);
    let census = census.borrow();
    eprintln!(
        "[lzs census] {} entries; walkers: {} decoded ({} bytes, {} past one ring), {} failed; \
         tried: {} decoded ({} bytes), {} failed",
        entries.len(),
        census.ok,
        census.bytes,
        census.wrapped,
        census.failed,
        tried.ok,
        tried.bytes,
        tried.failed,
    );
    assert!(census.ok > 1000, "census too small to mean anything");
    assert!(census.wrapped > 500, "too few streams wrap the ring");
}

/// Old against new over the census's streams, in alternating order within one
/// process, as a ratio of the two: a same-moment comparison a busy box does not
/// disturb the way it does a pair of runs. Not part of any gate, so ignored;
/// run by hand with `--ignored --nocapture` on an optimised profile.
#[test]
#[ignore = "a measurement, not a check"]
fn decoder_speed_ratio_over_the_census() {
    let Some(entries) = prot_entries() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT missing");
        return;
    };
    let streams: Rc<RefCell<Vec<Request>>> = Rc::default();
    {
        let streams = streams.clone();
        set_lzs_observer(Some(Box::new(move |input, size| {
            if let Ok((_, consumed)) = reference_decoder::decode(input, size) {
                streams
                    .borrow_mut()
                    .push((input[..consumed].to_vec(), size));
            }
        })));
    }
    for path in &entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(path).unwrap();
        let opts = AccountOptions {
            prot_index: name.split('_').next().and_then(|n| n.parse().ok()),
            label: name,
            depth: 4,
            ..AccountOptions::default()
        };
        account(&bytes, &opts);
    }
    set_lzs_observer(None);
    let streams = streams.borrow();

    report_ratio("census streams", &streams);

    // A detector's decode of bytes it has not yet proved are LZS, at a size
    // that is mostly wrong: the shape where the output buffer costs the most
    // against the work done.
    let tries: Vec<Request> = entries
        .iter()
        .map(|path| {
            let bytes = std::fs::read(path).unwrap();
            let size = bytes.len() * 2;
            (bytes, size)
        })
        .collect();
    report_ratio("whole entries at twice their size", &tries);
}

fn report_ratio(what: &str, streams: &[Request]) {
    let old = |s: &[Request]| {
        let start = std::time::Instant::now();
        for (input, size) in s {
            let _ = std::hint::black_box(reference_decoder::decode(input, *size));
        }
        start.elapsed().as_secs_f64()
    };
    let new = |s: &[Request]| {
        let start = std::time::Instant::now();
        for (input, size) in s {
            let _ = std::hint::black_box(psx_lzs::decompress_tracked(input, *size));
        }
        start.elapsed().as_secs_f64()
    };
    let mut ratios = Vec::new();
    for round in 0..40 {
        let (o, n) = if round % 2 == 0 {
            let o = old(streams);
            (o, new(streams))
        } else {
            let n = new(streams);
            (old(streams), n)
        };
        ratios.push(o / n);
    }
    ratios.sort_by(f64::total_cmp);
    eprintln!(
        "[lzs ratio] {what}: {} requests, old/new median {:.3}, quartiles {:.3}..{:.3}, \
         range {:.3}..{:.3}",
        streams.len(),
        ratios[ratios.len() / 2],
        ratios[ratios.len() / 4],
        ratios[ratios.len() * 3 / 4],
        ratios[0],
        ratios[ratios.len() - 1],
    );
}
