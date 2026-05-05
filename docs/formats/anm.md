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

## Connection to other systems

The [field/event script VM](../subsystems/script-vm.md) opcode `0x34` sub-op 3 plays a 3D animation by indexing into an ANM container and handing the entry to `func_0x800252EC`. The implication is that the ANM bytecode is interpreted by code reachable from the field/town overlay path; tracing `func_0x800252EC` further would surface the per-record opcode dispatcher.
