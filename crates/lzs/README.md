# legaia-lzs

Legend of Legaia LZS decompressor.

Reverse-engineered from `FUN_8001a55c` in `SCUS_942.54` (see
`ghidra/scripts/funcs/8001a55c.txt`).
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
# Decompress a raw stream whose output size you already know. There is no
# length prefix on the wire, so --size is required; --skip starts the stream
# partway into the file.
lzs-decode raw extracted/PROT/0863_battle_data.BIN --size 32768 -o out.bin
lzs-decode raw extracted/PROT/0863_battle_data.BIN --size 32768 --skip 32 -o out.bin

# Parse an `.lzs` container and write one file per section
lzs-decode container extracted/PROT/<entry>.BIN sections/

# Is this file an LZS container? (heuristic - see the caveat above)
lzs-decode probe extracted/PROT/<entry>.BIN

# Walk a directory and list everything that looks like a valid container
lzs-decode scan extracted/PROT

# Decode every container in a directory and cluster the results by
# (total decoded size, first 24 decoded bytes) to group like with like.
# --tsv writes one summary row per file for offline analysis.
lzs-decode audit extracted/PROT --tsv audit.tsv

# Brute-search for an embedded stream whose DECOMPRESSED output contains a
# needle. Tries every byte offset as a stream start, so it finds
# non-container streams at arbitrary offsets that `scan` misses - e.g. the
# in-battle party palette buried inside a scene bundle.
lzs-decode find extracted/PROT --needle 409d709079be16b6 --max-out 8192
```

## See also

- [`docs/formats/lzs.md`](../../docs/formats/lzs.md) - full algorithm and
  container layout.
