//! The cast bed's intro boost.

use super::load_disc;
use legaia_disc_patch::disc::DiscPatcher;

/// The whole-body speed profile of a rebuilt art stream, measured off the
/// readef slot itself: `speed[j]` is the mean per-part translation delta
/// between keyframes `j` and `j + 1`.
///
/// An ME entry decodes to `[u8 parts][u8 frames]` then `frames * parts`
/// nine-byte part records, six 12-bit fields each (low bytes at
/// `[0,1,3,4,6,7]`, high nibbles packed into `[2,5,8]` - the layout
/// `FUN_8004998C` unpacks). Only the three translations are read here, and
/// they are unpacked locally rather than through the crate's own decoder so
/// the assertion does not measure the patcher with the patcher's tools.
pub(super) fn stream_speed(readef: &[u8], slot: usize, entry_index: usize) -> Vec<f64> {
    use legaia_party_swap::winpose::READEF_SLOT;
    let off = legaia_actor_formats::battle_char_assembly::art_me_slot(slot, false) * READEF_SLOT;
    let ar = legaia_actor_formats::me_archive::parse(&readef[off..off + READEF_SLOT])
        .expect("ME archive");
    let body = ar.entry(entry_index).expect("art entry");
    let (parts, frames) = (body[0] as usize, body[1] as usize);
    let sx12 = |v: u16| -> i32 {
        if v & 0x800 != 0 {
            (v | 0xF000) as i16 as i32
        } else {
            v as i32
        }
    };
    let pose = |f: usize, p: usize| -> [i32; 3] {
        let b = &body[2 + (f * parts + p) * 9..][..9];
        [
            sx12(b[0] as u16 | ((b[2] as u16 & 0x0F) << 8)),
            sx12(b[1] as u16 | ((b[2] as u16 & 0xF0) << 4)),
            sx12(b[3] as u16 | ((b[5] as u16 & 0x0F) << 8)),
        ]
    };
    (1..frames)
        .map(|f| {
            let sum: f64 = (0..parts)
                .map(|p| {
                    let (a, b) = (pose(f - 1, p), pose(f, p));
                    let d: Vec<f64> = (0..3).map(|k| (b[k] - a[k]) as f64).collect();
                    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
                })
                .sum();
            sum / parts.max(1) as f64
        })
        .collect()
}

/// The Plasma Strike bed intro remaster: the front shift opens the
/// music ~0.19 s into the stream (retail: silence to +1.31 s), touches
/// ONLY the 70-sector intro span of `XA20.XA` channel 2, converges back
/// onto the retail bytes before the span ends (so the +14.8 s blast and
/// the walk sync are untouched by construction), and guards against
/// double application.
#[test]
fn cast_bed_intro_boost_is_audible_scoped_and_guarded() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const FILE: &str = "XA/XA20.XA";
    const CHAN: u8 = 2;
    const SPAN_LEN: usize = 70;
    let mut p = DiscPatcher::open(original).expect("open");
    let (before, coding) = p.read_xa_channel_payload(FILE, CHAN).expect("read");
    assert_ne!(coding & 0x03, 0, "bed is stereo");

    assert!(
        legaia_delilas_party::delilas_xa_voice::boost_cast_bed_intro(&mut p).expect("remaster"),
        "first apply remasters"
    );
    let (after, _) = p.read_xa_channel_payload(FILE, CHAN).expect("re-read");
    assert_eq!(before.len(), after.len());
    // Scope: nothing after the span changed.
    assert_eq!(
        &before[SPAN_LEN * 2304..],
        &after[SPAN_LEN * 2304..],
        "post-span stream untouched"
    );
    assert_ne!(
        &before[..SPAN_LEN * 2304],
        &after[..SPAN_LEN * 2304],
        "span rewritten"
    );
    // Convergence: the written span's tail re-encodes to the retail
    // bytes (bit-identical input after the +3.3 s bridge), proving the
    // blast region could not have moved.
    assert_eq!(
        &before[66 * 2304..SPAN_LEN * 2304],
        &after[66 * 2304..SPAN_LEN * 2304],
        "span tail converges onto retail bytes"
    );

    // The advance: the opening hit now lands in +0.15..0.6 s, where
    // retail carries digital silence.
    let opts = legaia_xa::DecodeOptions {
        channels: legaia_xa::Channels::Stereo,
        sample_rate: 37800,
        ..Default::default()
    };
    let rms = |payload: &[u8], a: usize, b: usize| -> f64 {
        let need = SPAN_LEN * 2304;
        let (pcm, _) = legaia_xa::decode(&payload[..need], opts).expect("decode");
        let mut acc = 0.0f64;
        for f in a..b {
            let v = pcm[2 * f] as f64;
            acc += v * v;
        }
        (acc / (b - a) as f64).sqrt()
    };
    let (a, b) = (5670usize, 22680usize); // stereo frames 0.15..0.6 s
    let (r0, r1) = (rms(&before, a, b), rms(&after, a, b));
    assert!(r0 < 200.0, "retail opening is silent ({r0:.0})");
    assert!(r1 > 8000.0, "shifted opening hit is audible ({r1:.0})");

    // Double-application guard.
    assert!(
        !legaia_delilas_party::delilas_xa_voice::boost_cast_bed_intro(&mut p).expect("re-apply"),
        "second apply is a no-op"
    );
    let (again, _) = p.read_xa_channel_payload(FILE, CHAN).expect("read 3");
    assert_eq!(after, again, "guarded bytes stable");
}
