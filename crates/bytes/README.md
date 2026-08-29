# legaia-bytes

Checked little-endian byte readers shared across the Legaia parser crates.

Every format parser in the workspace reads unaligned little-endian scalars out
of raw disc buffers. Historically each crate — and often each module — carried
its own private `read_u32_le` / `read_u16_le` helper of the shape
`buf.get(at..at + N)?.try_into().unwrap()`. This crate hoists that single idiom
into one leaf dependency (no dependencies of its own) so the bounds check
travels with the read and the `.try_into().unwrap()` array copy lives in exactly
one place.

## Scope

Free functions returning `Option<T>` — [`None`] when the requested window falls
outside the buffer, matching the dominant `.get(range)?` convention callers
already relied on. Safe to point at untrusted disc bytes; nothing panics on a
short or malformed buffer.

| Function | Reads |
|---|---|
| `u8_at`  | one byte |
| `u16_le` / `i16_le` | little-endian 16-bit |
| `u24_le` | little-endian unsigned 24-bit, widened to `u32` |
| `u32_le` / `i32_le` | little-endian 32-bit |

C-string / VA-resolved readers stay crate-local: their semantics vary per
format (max-length handling, `ExeMap` VA translation), so they are intentionally
out of scope here.

## Pipeline

Leaf crate with no dependencies of its own. Depended on by `legaia-asset` and
`legaia-engine-core` - the two crates that read the widest range of raw disc
buffers - wherever a raw little-endian read out of one is needed. Other
parser crates still carry their own private helpers; each is a candidate to
fold in here.
