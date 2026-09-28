//! Disc-gated oracle for the two-frame pose sampler
//! (`PlayerAnmBundle::sample_bone`, the port of `FUN_8001BE80`).
//!
//! Over every player-ANM bundle the detector finds on the disc it checks:
//!
//! - at every **integer** cursor (`frame * 16`, low nibble zero) the sampler
//!   returns exactly the single-entry decode `bone_transform`, for both
//!   values of the clamp bit - the blend must never move a keyframe;
//! - a record whose blend gate is clear returns that same decode at every
//!   sub-frame cursor too;
//! - a gated record's sub-frame samples stay inside the 12-bit domains.
//!
//! It prints the census the doc cites (records per gate value, and how many
//! half-frame samples take the Euler-flip retry). Skips when
//! `LEGAIA_DISC_BIN` is unset.

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_asset::player_anm::{self, EULER_FLIP_THRESHOLD, lerp_angle_12_journaled};

fn prot_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()?
        .to_path_buf();
    let p = repo.join("extracted").join("PROT");
    p.is_dir().then_some(p)
}

#[test]
fn sampler_matches_the_decode_at_every_keyframe_on_the_disc() {
    let Some(dir) = prot_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT missing");
        return;
    };
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "BIN"))
        .collect();
    paths.sort();

    let mut seen_bundles = BTreeSet::new();
    let (mut bundles, mut records, mut gated, mut keyframes) = (0usize, 0usize, 0usize, 0usize);
    let (mut half_samples, mut flips) = (0usize, 0usize);
    let mut gated_entries = BTreeSet::new();
    let mut divisors = std::collections::BTreeMap::new();
    // (records, gated) for record indices 0..=8 and 9+.
    let mut head_tail = [(0usize, 0usize); 2];
    for path in &paths {
        let bytes = std::fs::read(path).unwrap();
        for desc in [3usize, 5, 6, 7] {
            for bundle in player_anm::find_in_entry(&bytes, desc) {
                // The same section parses under several descriptor counts.
                if !seen_bundles.insert(bundle.decoded.clone()) {
                    continue;
                }
                bundles += 1;
                for r in 0..bundle.record_count as usize {
                    let Ok(rec) = bundle.record(r) else { continue };
                    records += 1;
                    let (bones, frames) = (rec.bone_count as usize, rec.frame_count as usize);
                    let band = usize::from(r >= 9);
                    head_tail[band].0 += 1;
                    head_tail[band].1 += usize::from(rec.blends());
                    if rec.blends() {
                        gated += 1;
                        *divisors.entry(rec.flag & 0xFF).or_insert(0usize) += 1;
                        gated_entries
                            .insert(path.file_name().unwrap().to_string_lossy().to_string());
                    }
                    for f in 0..frames {
                        for b in 0..bones {
                            let d = bundle.bone_transform(r, f, b).unwrap();
                            for hold in [false, true] {
                                let c = (f * 16) as i16;
                                assert_eq!(
                                    bundle.sample_bone(r, c, hold, b),
                                    Some(d),
                                    "{} rec {r} frame {f} bone {b} hold {hold}",
                                    path.display()
                                );
                            }
                            keyframes += 1;
                            for frac in 1..16i16 {
                                let s = bundle
                                    .sample_bone(r, (f * 16) as i16 + frac, false, b)
                                    .unwrap();
                                if !rec.blends() {
                                    assert_eq!(s, d);
                                    continue;
                                }
                                for a in [s.r_x, s.r_y, s.r_z] {
                                    assert!((0..0x1000).contains(&a));
                                }
                                if frac == 8 {
                                    half_samples += 1;
                                    let n = bundle.bone_transform(r, (f + 1) % frames, b).unwrap();
                                    let sum: i32 = [(n.r_x, d.r_x), (n.r_y, d.r_y), (n.r_z, d.r_z)]
                                        .iter()
                                        .map(|&(a, b)| lerp_angle_12_journaled(a, b, 8).magnitude)
                                        .sum();
                                    if sum > EULER_FLIP_THRESHOLD {
                                        flips += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    eprintln!(
        "[ok] {bundles} distinct bundles, {records} records ({gated} blend-gated, in {} PROT entries), \
         {keyframes} (record, frame, bone) keyframes exact; {flips} of {half_samples} gated half-frame \
         samples take the Euler-flip retry",
        gated_entries.len()
    );
    eprintln!("[ok] gated entries: {gated_entries:?}");
    eprintln!(
        "[ok] gated by record index: {} of {} in 0..=8, {} of {} in 9+",
        head_tail[0].1, head_tail[0].0, head_tail[1].1, head_tail[1].0
    );
    eprintln!("[ok] gated records by step divisor (flag low byte): {divisors:?}");
    assert!(records > 0, "no ANM bundles found - the census is vacuous");
}
