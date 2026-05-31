# legaia-lzs

Legend of Legaia LZS decompressor.

Reverse-engineered from `FUN_8001a55c` in `SCUS_942.54` (see
[`ghidra/scripts/funcs/8001a55c.txt`](../../ghidra/scripts/funcs/8001a55c.txt)).
The algorithm is a sliding-window LZSS variant:

- 4096-byte ring buffer initialised to zero, write position starts at `0xFEE`.
- LSB-first 8-bit control byte; the high bit of the in-register control word
  (`0x100`) is a "byte exhausted, fetch the next one" sentinel.
- Control bit = 1 → emit one literal byte from the source.
- Control bit = 0 → read two bytes `(b0, b1)`:
    * absolute window position = `b0 | ((b1 & 0xF0) << 4)` (12 bits).
    * length = `(b1 & 0x0F) + 3`.
    * copy `length` bytes out of the ring buffer; each emitted byte is
      also stored at the current write position (which advances mod 4096).
- The decompressed size is supplied externally - there's no length prefix
  or end-of-stream marker.

`.lzs` files are containers: a small `u32` header table at the start gives
`(decompressed_size, byte_offset_to_stream)` for each section.
`decompress_container` parses that.

## What it provides

- `decompress(input, expected_output_size) -> Vec<u8>`
- `decompress_tracked(input, expected_output_size) -> (Vec<u8>, consumed)`
  - also returns input bytes consumed; useful for validating containers.
- `decompress_container(input) -> Vec<Vec<u8>>` - full `.lzs` container.
- `compress(input) -> Vec<u8>` - the inverse, for re-packing edited assets
  (e.g. the disc patcher). The retail game ships only a decoder, so this is an
  LZSS matcher (with one-step lazy matching) whose output the retail decoder
  accepts byte-for-byte, **not** a bit-exact clone of Sony's packer. Its
  correctness criterion is `decompress(compress(x)) == x`; it packs tightly
  enough that a re-packed asset fits its original footprint even where that
  footprint has no compressed slack (e.g. a scene MAN), which is what makes an
  in-place same-size edit possible. See the
  [randomizer / disc patcher](../../docs/tooling/randomizer.md).

### Why "decompresses without error" is not a validity signal

The 4096-byte ring buffer initialises to zeros, so most random byte streams
decode to plausibly-sized zero-padded output. Always magic-check the
*decoded output* before claiming you've found an LZS container.

## CLI

```bash
lzs-decode raw       <input> <output> --size <expected_size>
lzs-decode container <input> <out_dir>
lzs-decode probe     <input>             # heuristic: looks like an LZS?
lzs-decode scan      <dir>               # walk a directory
lzs-decode audit     <input>             # strict-real LZS detection
```

## See also

- [`docs/formats/lzs.md`](../../docs/formats/lzs.md) - full algorithm and
  container layout.
