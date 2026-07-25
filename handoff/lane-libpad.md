# Lane: libpad vs SsAPI - `FUN_8006E2B4` / `FUN_8006CE30`

## Verdict

**libpad**, grade `disassembly`. `FUN_8006E2B4` is `PadInitDirect(buf0, buf1)`
and `FUN_8006CE30` is a three-argument `PadSetAct(socket, table, len)`. The
whole `0x801CE628` cluster is libpad's two-port driver-context array, not an
SsAPI sequence-worker table. Consequently `DAT_800915DA` / `DAT_800915DB` are
port 0's actuator bytes and `FUN_80018DB0` is a **rumble** cadence that plays
no sound at all.

Full write-up with the evidence:
[`re-settled-threads.md`](../docs/reference/re-settled-threads.md#fun_80018db0-is-a-rumble-cadence-not-an-audio-one).
Corrected function cluster:
[`audio.md`](../docs/subsystems/audio.md#not-ssapi-the-0x801ce628-cluster-is-libpad).

## What was changed

| File | Change |
|---|---|
| `docs/subsystems/audio.md` | Six libpad rows removed from the SsApi table; the "Seq-worker callback table" subsection replaced by "Not SsAPI: the `0x801CE628` cluster is libpad"; three stale libspu rows and the `_DAT_801CE564`/`_574` globals row corrected. |
| `docs/reference/functions/audio.md` | `80018DB0`, `80018F94` and `8001D230` rows rewritten. All three are kept on the audio page as explicit corrections rather than deleted. |
| `docs/reference/open-rev-eng-threads.md` | Rumble thread resolved and moved out; "Retail's footstep SFX cue id" closed resolved-negative (no cue exists to pin). Both detail sections removed. |
| `docs/reference/re-settled-threads.md` | New Audio row + `###` section carrying the instruction-level evidence and the falsified-label post-mortem. |
| `crates/engine-audio/src/footstep.rs` | Doc comments only. The port and its wiring are untouched - the arithmetic is a faithful mirror of `FUN_80018DB0`; only the labels were wrong. |

## Verification method worth reusing

The `FUN_8006CE30` and `FUN_8001D230` windows were disassembled **directly from
`extracted/SCUS_942.54`** with capstone at file offset `0x800 + va -
0x80010000`, not read off the dump, so the argument count and the `0x22` /
`0x40` strides do not depend on any dump's printed base. That is what makes
the dropped-`param_1` call a settled fact rather than a second guess: Ghidra
renders `FUN_8006CE30` as two arguments, the bytes show three
(`a0` → `jalr` resolver, `a1` → `s0` → `a1`, `a2` → `s1` → `a2`).

## Left open / out of scope

Same-cluster mislabels found but **not** touched, because they sit outside this
lane's file scope. Each is the same error class (an audio label on a
pad / card / CD function) and each is cheap to fix with the addresses below.

- `docs/reference/functions/runtime-libs.md` - the `8002035C` row calls
  `FUN_8006EF18` an "SPU voice-state init". `FUN_8006EF18` is
  `StopCARD` (B0 `0x4C`) plus its card-family siblings, so that row is the
  teardown of the memory-card init, not of SPU state.
- `docs/subsystems/renderer.md` and `docs/subsystems/boot.md` both cite
  `FUN_8005C034` in a `PutDrawEnv` / `PutDispEnv` / `DrawSync` context.
  `FUN_8005C034` is the `CdControl` retry wrapper over `FUN_8005CF80` (the
  corpus already identifies `8005CF80` as `CdControl` in
  `docs/tooling/playthrough-coverage.md`). One of the two readings is wrong.
- `docs/formats/sfx-table.md` describes `FUN_80018DB0` as a field *audio*
  cadence in its narrative section.
- `scripts/ci/port-catalog-ignore.toml` - the `80018f94` entry describes the
  function as libspu voice-state glue. It is `PadGetState` / `PadInfoMode` /
  `PadSetAct` / `PadSetActAlign` glue.

Nothing here changes engine behaviour; all four are label corrections.

## If this is ever re-opened

The counter-hypothesis was checked and it does have a basis - the band really
does hold libspu code, and the vtable-over-stride-`0xF0`-records shape really
does look like a sequence-worker table. What kills it is not the shape but
three concrete checks, each of which any future re-read should redo first:

1. the resolved context's `+0x30` field holds `0x800840F8`, which
   `FUN_8001822C` decodes as buttons;
2. `FUN_8006CB3C`'s `term = 4` branch implements `PadInfoMode`'s id-table
   contract (`offs < 0` → length at `ctx+0xE3`, else bounds-checked
   `((u16 *)ctx[0])[offs]`), which has no sequencer analogue;
3. the context array holds exactly 2 records.
