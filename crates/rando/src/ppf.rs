//! PPF 3.0 (PlayStation Patch File) writer.
//!
//! A PPF patch is the portable, redistributable deliverable: it carries only
//! the *bytes that changed* between the user's original disc and the patched
//! one, keyed by absolute file offset. Applying it to a fresh copy of the same
//! disc reproduces the patch. Because it ships only deltas the user already
//! owns (they supply the original disc), it embeds no standalone Sony asset —
//! the diff is meaningless without the original image.
//!
//! Format (PPF 3.0, "Undo data not available" variant):
//!
//! ```text
//! +0x00  5    magic  "PPF30"
//! +0x05  1    encoding = 0x02 (PPF 3.0)
//! +0x06  50   description (ASCII, space-padded)
//! +0x38  1    image-type = 0x00 (image is a raw disc; not a PSX EXE)
//! +0x39  1    block-check = 0x00 (no validation block)
//! +0x3A  1    undo-data   = 0x00 (none)
//! +0x3B  1    dummy       = 0x00
//! +0x3C  ...  records: [u64 LE offset][u8 length][length bytes]
//! ```
//!
//! Each record overwrites `length` bytes at the absolute disc offset. Records
//! are emitted in ascending offset order; runs longer than 255 bytes are split
//! across consecutive records.

/// Maximum bytes a single PPF 3.0 record can carry (the length field is `u8`).
const MAX_RUN: usize = 255;

/// A contiguous changed span: `bytes` overwrites the image at `offset`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchRun {
    pub offset: u64,
    pub bytes: Vec<u8>,
}

/// Diff `original` against `patched` (which must be the same length) into the
/// minimal set of contiguous changed runs, in ascending offset order.
pub fn diff_runs(original: &[u8], patched: &[u8]) -> Vec<PatchRun> {
    assert_eq!(
        original.len(),
        patched.len(),
        "PPF diff requires equal-length images (the patcher only does same-size edits)"
    );
    let mut runs = Vec::new();
    let mut i = 0usize;
    while i < original.len() {
        if original[i] == patched[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < original.len() && original[i] != patched[i] {
            i += 1;
        }
        // Split the changed span into <=255-byte records.
        let mut off = start;
        while off < i {
            let len = (i - off).min(MAX_RUN);
            runs.push(PatchRun {
                offset: off as u64,
                bytes: patched[off..off + len].to_vec(),
            });
            off += len;
        }
    }
    runs
}

/// Serialize changed runs into a PPF 3.0 patch file. `description` is truncated
/// or space-padded to the 50-byte field.
pub fn write_ppf3(description: &str, runs: &[PatchRun]) -> Vec<u8> {
    let mut out = Vec::with_capacity(0x3C + runs.len() * 16);
    out.extend_from_slice(b"PPF30");
    out.push(0x02); // encoding: PPF 3.0
    let mut desc = [b' '; 50];
    for (d, s) in desc.iter_mut().zip(description.bytes()) {
        *d = s;
    }
    out.extend_from_slice(&desc);
    out.push(0x00); // image type: disc image
    out.push(0x00); // block check: none
    out.push(0x00); // undo data: none
    out.push(0x00); // dummy
    for run in runs {
        debug_assert!(!run.bytes.is_empty() && run.bytes.len() <= MAX_RUN);
        out.extend_from_slice(&run.offset.to_le_bytes());
        out.push(run.bytes.len() as u8);
        out.extend_from_slice(&run.bytes);
    }
    out
}

/// Apply PPF 3.0 records to an image in place. Returns the number of records
/// applied. Used by the round-trip test (and handy for verification tooling).
pub fn apply_ppf3(image: &mut [u8], ppf: &[u8]) -> anyhow::Result<usize> {
    use anyhow::{bail, ensure};
    ensure!(ppf.len() >= 0x3C, "PPF too short");
    ensure!(&ppf[0..5] == b"PPF30", "not a PPF30 patch");
    ensure!(ppf[5] == 0x02, "unsupported PPF encoding {:#x}", ppf[5]);
    let mut pos = 0x3C;
    let mut applied = 0;
    while pos < ppf.len() {
        if pos + 9 > ppf.len() {
            bail!("truncated PPF record header at {pos}");
        }
        let offset = u64::from_le_bytes(ppf[pos..pos + 8].try_into().unwrap()) as usize;
        let len = ppf[pos + 8] as usize;
        pos += 9;
        if pos + len > ppf.len() {
            bail!("truncated PPF record body at {pos}");
        }
        let dst = image
            .get_mut(offset..offset + len)
            .ok_or_else(|| anyhow::anyhow!("PPF record at {offset} past end of image"))?;
        dst.copy_from_slice(&ppf[pos..pos + len]);
        pos += len;
        applied += 1;
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_finds_only_changed_runs() {
        let orig = vec![0u8; 16];
        let mut patched = orig.clone();
        patched[3] = 1;
        patched[4] = 2;
        patched[10] = 9;
        let runs = diff_runs(&orig, &patched);
        assert_eq!(
            runs,
            vec![
                PatchRun {
                    offset: 3,
                    bytes: vec![1, 2]
                },
                PatchRun {
                    offset: 10,
                    bytes: vec![9]
                },
            ]
        );
    }

    #[test]
    fn long_run_splits_at_255() {
        let orig = vec![0u8; 600];
        let patched = vec![0xABu8; 600];
        let runs = diff_runs(&orig, &patched);
        assert_eq!(runs.len(), 3, "600 changed bytes -> 255 + 255 + 90");
        assert_eq!(runs[0].bytes.len(), 255);
        assert_eq!(runs[1].bytes.len(), 255);
        assert_eq!(runs[2].bytes.len(), 90);
        assert_eq!(runs[1].offset, 255);
    }

    #[test]
    fn write_then_apply_round_trips() {
        let orig: Vec<u8> = (0..1000u32).map(|i| (i * 13) as u8).collect();
        let mut patched = orig.clone();
        for i in [7usize, 8, 9, 400, 401, 999] {
            patched[i] = patched[i].wrapping_add(0x55);
        }
        let runs = diff_runs(&orig, &patched);
        let ppf = write_ppf3("test patch", &runs);
        assert_eq!(&ppf[0..5], b"PPF30");
        assert_eq!(ppf[5], 0x02);

        let mut rebuilt = orig.clone();
        let n = apply_ppf3(&mut rebuilt, &ppf).unwrap();
        assert_eq!(n, runs.len());
        assert_eq!(rebuilt, patched, "PPF apply reproduces the patched image");
    }

    #[test]
    fn empty_diff_yields_no_records() {
        let img = vec![1u8, 2, 3, 4];
        let runs = diff_runs(&img, &img);
        assert!(runs.is_empty());
        let ppf = write_ppf3("noop", &runs);
        assert_eq!(ppf.len(), 0x3C, "header only, no records");
    }
}
