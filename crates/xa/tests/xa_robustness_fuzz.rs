//! Robustness fuzz for the XA-ADPCM decoder: hammer `decode` with truncations
//! and random bytes in mono and stereo, asserting it never panics. XA audio
//! comes straight off the user's disc, so a bad dump must degrade to an error
//! (or skipped groups), never a crash. Deterministic + disc-free.
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

#[test]
fn xa_decode_never_panics_on_malformed_input() {
    let mut rng = Rng(0xa1b2_c3d4_e5f6_0718);
    let mut panics = 0usize;
    let mut inputs = Vec::new();
    for n in 0..300usize {
        inputs.push(vec![0u8; n]);
        inputs.push(rng.bytes(n));
    }
    for _ in 0..4000 {
        let n = (rng.next_u8() as usize) * 16 + (rng.next_u8() as usize);
        inputs.push(rng.bytes(n));
    }
    for input in inputs {
        for ch in [legaia_xa::Channels::Mono, legaia_xa::Channels::Stereo] {
            let opts = legaia_xa::DecodeOptions {
                channels: ch,
                ..Default::default()
            };
            if catch_unwind(AssertUnwindSafe(|| {
                let _ = legaia_xa::decode(&input, opts);
            }))
            .is_err()
            {
                panics += 1;
                if panics <= 3 {
                    eprintln!(
                        "PANIC in xa::decode len={} head={:02X?}",
                        input.len(),
                        &input[..input.len().min(16)]
                    );
                }
            }
        }
    }
    assert_eq!(panics, 0, "{panics} xa inputs panicked");
}
