# Legaia LZS compression

Reverse-engineered byte-for-byte from `FUN_8001A55C`. Implementation: `crates/lzs/src/lib.rs`.

A standard LZSS variant with three Legaia-specific choices:
- **4096-byte sliding ring buffer**, initialised to zero.
- **Initial write position 0xFEE** (3054).
- **Control byte: 8 bits, LSB-first.**

## Pseudocode

```rust
let mut window = [0u8; 4096];
let mut window_pos = 0xFEE;
let mut control = 0u32;

while !done {
    if (control & 0x100) == 0 {
        control = (input[src] as u32) | 0xFF00;
        src += 1;
    }
    if (control & 1) != 0 {
        // LITERAL: copy 1 byte
        let v = input[src]; src += 1;
        out.push(v);
        window[window_pos] = v;
        window_pos = (window_pos + 1) & 0xFFF;
    } else {
        // BACK-REF: 2 bytes encode (12-bit absolute window position, 4-bit length-3)
        let b0 = input[src] as u32;
        let b1 = input[src + 1] as u32;
        src += 2;
        let base = b0 | ((b1 & 0xF0) << 4);
        let len = (b1 & 0x0F) + 3;
        for n in 0..len {
            let v = window[(base + n as u32) as usize & 0xFFF];
            out.push(v);
            window[window_pos] = v;
            window_pos = (window_pos + 1) & 0xFFF;
        }
    }
    control >>= 1;
}
```

The `0xFF00` mask above is the trick that lets the control register tell the decoder when to refill: every shift right pulls a `1` bit into bit 8, and after 8 shifts bit 8 reaches the test position and triggers a refill from `input[src]`.

## Container format

`crates/lzs::parse_container` handles the multi-section `player.lzs`-style wrapper used by some PROT entries — a length-prefixed array of independently-compressed sections concatenated together.

## "Decompresses without error" is not a validity signal

The 4096-byte ring buffer initialises to zeros, so most random inputs decode without error to a zero-padded output of plausible length. Always magic-check the *decoded* output before treating an LZS-decode result as a hit. The decoder offers a `tracked` mode that returns end-of-stream offsets and overrun bits so callers can apply strict gates.

## Where LZS is consumed

The asset-type dispatcher (`FUN_8001F05C`) calls the LZS path when its `copy_only` flag is zero. See [asset type dispatcher](asset-type.md). Standalone-shaped LZS containers (with the descriptor-pair walker in [`asset-descriptor.md`](asset-descriptor.md)) are also recognised by `crates/lzs`.
