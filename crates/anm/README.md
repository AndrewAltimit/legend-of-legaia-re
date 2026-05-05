# legaia-anm

Legaia ANM (asset type `0x06`) — animation pack container.

## Format (in RAM, post-load)

```text
u32 count                        // number of animation records
u32 byte_offset[count]           // each is a byte offset relative to
                                 //   payload base (i.e. relative to
                                 //   the count word)
...record bytes, packed back-to-back...
```

The byte offsets in the table are *relative to the payload base* — same
as the count `u32` itself. Record `i` lives at
`payload[byte_offset[i] .. byte_offset[i+1]]`, with the last record
extending to the payload end.

## Wrapper preamble (only when extracted from a RAM dump)

When the dispatcher (`FUN_8001f05c` case 6) loads ANM data, the
malloc'd buffer at `DAT_8007b7c8` carries a 16-byte allocator preamble
before the payload:

```text
+0x00  back_ptr        (RAM ptr)
+0x04  forward_ptr     (RAM ptr to next allocation)
+0x08  forward_ptr_2   (RAM ptr — sometimes 0)
+0x0C  expanded_size   (u32 — total allocated bytes)
+0x10  -- payload starts here --
```

`parse` takes the *payload* (no preamble). Use `peel_preamble` to strip
the wrapper from a RAM-extracted blob first.

## Per-record content

Each record begins with an 8-byte common header observed across two
independent captures (title screen, town):

```text
u16 a       // varies (e.g. 0x0A, 0x06, 0x02)
u16 b       // varies (e.g. 0x1E, 0x14, 0x28) — likely frame count
u16 marker1 // = 0x080C in every record observed
u16 marker2 // = 0x0002 in every record observed
...payload bytes...
```

Per-record bytecode interpretation is **not** in this crate yet — the
animation system is overlay-resident. The only static reader of ANM
containers is `FUN_80024cfc` (`play_anm_by_id`).

## CLI

```bash
anm info    <file>                  # count, header markers, record sizes
anm extract <file> <out_dir>        # split records to disk
anm json    <file>                  # JSON dump
```

## See also

- [`docs/formats/anm.md`](../../docs/formats/anm.md)
