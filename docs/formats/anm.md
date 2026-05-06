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

### Walker-candidate scan

[`ghidra/scripts/find_anm_tick_walker.py`](../../ghidra/scripts/find_anm_tick_walker.py) walks every function in a program and reports which ones load from at least two of `+0x4C` (anm_pc), `+0x56` (anm_state), `+0x68` (anm_timer). Functions that hit all three are the strongest walker candidates. Results across the captured corpus:

| Program | Hits-3 candidates |
|---|---|
| `SCUS_942.54` | `FUN_80021DF4` (per-frame actor tick), `FUN_80023070` (move VM), `FUN_80024CFC` (the writer), `FUN_80020DE0`, `FUN_800204F8`, `FUN_8001ADA4`, `FUN_8003A1E4`, `FUN_80047430`, `FUN_8004998C` |
| `overlay_0897_xxx_dat.bin` | `FUN_801C8D00`, `FUN_801C8FDC` (small standalone walkers, not inlined into the field VM) |
| `overlay_0897.bin.0` | `FUN_801D7518`, `FUN_801D77F4`, `FUN_801DE840` (field VM reads the slot for actor lookups) |
| `overlay_dialog_mc4.bin` | same trio as 0897 (dialog overlay shares the actor frame chain) |
| `overlay_menu.bin` | `FUN_801D33D8` |

`FUN_80021DF4` is the canonical per-frame actor tick (already documented as the move-VM driver). Memory + scan together suggest the ANM bytecode interpreter is reached via the move VM's per-actor dispatch — opcode `0x05` in [move-vm.md](../subsystems/move-vm.md) sets `actor[+0x56]` to the value `0x0B` that `FUN_80024CFC` also writes, suggesting ANM playback is just one of several "animation source" modes the move VM multiplexes. The `0x801C8D00` / `0x801C8FDC` pair in the 0897 town overlay are smaller and more likely candidates for the per-record walker proper.

## Connection to other systems

The [field/event script VM](../subsystems/script-vm.md) opcode `0x34` sub-op 3 plays a 3D animation by indexing into an ANM container and handing the entry to `func_0x800252EC`. That sibling path likely walks the same `actor[+0x4C]` slot via the same per-frame ANM tick.
