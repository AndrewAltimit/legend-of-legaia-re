# ANM animation container

Asset type `0x06` from the [asset-type dispatcher](asset-type.md). Implementation: `crates/anm`.

## Layout

```
u16 count
u16 byte_offsets[count]    // each is a byte offset into the buffer
records[]                  // per-record bodies; offsets[i+1] - offsets[i] = record size
```

Each record begins with a 16-bit marker byte that's been observed to consistently equal `0x080C` — this is the gate for the per-record bytecode interpreter that lives in an overlay (i.e., the overlay that drives the animation timeline; not yet captured).

## What's parsed today

- Container shape (count + offsets + record boundaries) — confirmed across captured RAM blobs.
- Per-record bytecode — partial; only the leading marker is reliably interpreted. Per-record op bodies are overlay-resident.

## Per-record bytecode dispatcher

`FUN_80024CFC` (`play_anm_by_id(id, actor, ?)` in SCUS) is the public entry point — but it doesn't *interpret* the bytecode. It just:

1. Calls `FUN_80020DE0` (actor allocator).
2. Reads the per-record offset from the ANM payload at `_DAT_8007B7C8 + (id*4) + 4`.
3. Stores `(anm_base + record_offset)` in `actor[+0x4C]` (the per-actor "anm pc" slot).
4. Writes `0xB` to `actor[+0x56]` (animation state byte) and `100` to `actor[+0x68]` (frame counter).

The actual per-frame interpretation runs in the per-frame actor tick — that dispatcher is overlay-resident and not yet traced. Until then `crates/anm` ships `record_bytecode_histogram` / `pack_bytecode_histogram` for byte-frequency surveys (run via `anm histogram <PATH>` to spot likely opcode bytes without the dispatcher).

## Connection to other systems

The [field/event script VM](../subsystems/script-vm.md) opcode `0x34` sub-op 3 plays a 3D animation by indexing into an ANM container and handing the entry to `func_0x800252EC`. That sibling path likely walks the same `actor[+0x4C]` slot via the same per-frame ANM tick.
