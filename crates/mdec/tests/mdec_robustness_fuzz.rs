//! Robustness fuzz for the MDEC video path: hammer the STR sector parser,
//! timing analyzer, and frame decoder with truncations and random bytes, and
//! assert none panic. STR frames come straight off the user's disc, so a bad
//! dump must degrade to an error, never a crash. Deterministic + disc-free.
use std::panic::{AssertUnwindSafe, catch_unwind};

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

fn corpus() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut rng = Rng(0xdead_beef_1234_5678);
    for n in 0..64usize {
        out.push(vec![0u8; n]);
        out.push(rng.bytes(n));
    }
    for _ in 0..4000 {
        let n = (rng.next_u8() as usize) * 8 + (rng.next_u8() as usize);
        out.push(rng.bytes(n));
    }
    out
}

fn run<F: Fn(&[u8])>(name: &str, f: F) -> usize {
    let mut panics = 0;
    for input in corpus() {
        if catch_unwind(AssertUnwindSafe(|| f(&input))).is_err() {
            panics += 1;
            if panics <= 3 {
                eprintln!(
                    "PANIC in {name} len={} head={:02X?}",
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
fn mdec_never_panics_on_malformed_input() {
    let mut total = 0;
    total += run("mdec::parse_video_sector", |b| {
        let _ = legaia_mdec::str_sector::parse_video_sector(b);
    });
    total += run("mdec::analyze_str_timing", |b| {
        let _ = legaia_mdec::str_sector::analyze_str_timing(b);
    });
    // decode_frame across a few dimensions (must be multiple of 16).
    total += run("mdec::decode_frame", |b| {
        for (w, h) in [(16u32, 16u32), (320, 240), (32, 48)] {
            let _ = legaia_mdec::MdecDecoder::new(w, h).decode_frame(b);
        }
    });
    assert_eq!(total, 0, "{total} mdec inputs panicked");
}
