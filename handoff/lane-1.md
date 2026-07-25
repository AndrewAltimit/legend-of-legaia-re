# Lane 1 handoff - disc-denominated SCUS dump worklist

Lane 1 closed the `SCUS_942.54` code-gap worklist that
`scripts/ci/disc-coverage.py` emits. Coverage moved **84.0% → 94.4%**
(303518 → 341010 of 361126 code bytes). All eight runs the report opened with
are closed, plus the eight it promoted afterwards.

Everything below is a finding that belongs in a file Lane 1 does not own.

## 1. `docs/subsystems/audio.md` line 204 - a false claim, correct conclusion

The "Correction (label != role)" paragraph reads:

> `FUN_8006352C` / `FUN_8006320C` were tagged elsewhere as "fixed-point div"
> pitch kernels - they carry **no division** and are per-channel note/expression
> handlers.

**"they carry no division" is wrong.** `FUN_8006320C` carries a `div` / `mfhi`
pair at `0x8006329C..0x800632C4`, and `FUN_8006352C` carries the matching one at
`0x800635BC..0x800635E4`. Both are a **modulo** of the slide's remaining-tick
counter (`+0xA0`) by its signed per-tick step (`+0x4C`) - the sub-tick divider
that lets a slide move one volume unit every `N` ticks instead of `N` units every
tick.

The paragraph's *conclusion* survives: neither is a pitch kernel, and the
note→pitch math really is confined to `FUN_80066E50` / `FUN_8006C6E4`. Only the
reason needs replacing. Suggested wording: *"neither is a pitch kernel - the one
`div` each carries is a modulo of the slide tick counter, not a fixed-point
pitch divide"*.

While there, the same page's per-frame call graph (line 202) can be sharpened.
It currently says `FUN_80062F98` (per-slot fan-out) → `FUN_8006320C` /
`FUN_8006352C` → … with "track-end / vab-release in `FUN_80063AA8`". The
dispatch is by flag bit, and the full map is now in
`docs/reference/functions/audio.md` § "SsAPI per-frame calc tier". Two
corrections worth folding back:

- `FUN_8006320C` / `FUN_8006352C` are the **volume-slide ticks** (ascending /
  descending), not note/expression handlers. `FUN_8006206C`
  (`_SsSetSlideVolume`) is their installer and writes exactly their field set
  (`+0x48` / `+0x4A` / `+0x4C` / `+0x9C` / `+0xA0`) - that pairing is what
  identifies them.
- `FUN_80063AA8` is the **track-end / loop-repeat** handler, and the "vab
  release" reading is not what the disassembly does: on the last repeat it
  chains to another `(slot, channel)` named by `+0x22` / `+0x23` through
  `FUN_80064090` (unless `+0x22 == 0xFF`), and kills notes via `FUN_800684CC`.

## 2. `docs/reference/re-settled-threads.md` - two rows can be re-graded

Line 1598 lists `0x8002149C` and `0x80059E10` among addresses whose evidence
grade rests on something weaker than disassembly. Both now have dumps carrying a
full disassembly section:

- `0x8002149C` - 688 bytes / 172 instructions, force-disassembled (Ghidra had
  never analysed it). It is a **leaf** (no `addiu sp,sp,-N`), sits between
  `FUN_80021248` and `FUN_8002174C`, and reads the frame-delta scratch pair
  `0x1F800393` / `0x1F80037D` plus the camera-angle triple `DAT_8007B790` - the
  same inputs as `FUN_80021248`'s camera normalisation. Role not pinned; the
  bytes are now readable. Row added to `functions/game-modes.md`.
- `0x80059E10` - 644 bytes / 161 instructions. A libgpu **VRAM-transfer entry**:
  it clamps the caller's `RECT` extents at `+0x4` / `+0x6` against the
  framebuffer limits `0x80078D58` / `0x80078D5A` (the same globals the rectangle
  validator `FUN_80058170` bounds-checks) and calls `FUN_8005AA30` first. Row
  added to `functions/runtime-libs.md`.

Same line's `0x8004DC68` is **not** covered by this lane - it is still an
un-dumped gap edge.

## 3. `docs/reference/functions/battle.md` (sibling-owned) - one row to add

`FUN_800508DC` (732 bytes / 183 instructions) was in the worklist and is now
dumped with full disassembly. It is cited in
`docs/tooling/playthrough-coverage.md:695` as *"voice/anim-cue select keyed to
the 0x414"* but has no row in `functions/battle.md`. Lane 1 did not read it -
flagging only that the dump now exists at `ghidra/scripts/funcs/800508dc.txt`
where it previously carried no `size=` header.

The other battle-band addresses in the worklist (`8004AD80`, `8004C140`,
`8004C650`, `8004C7B4`) were already cited in `subsystems/battle-action.md` /
`subsystems/battle.md` / `formats/battle-data-pack.md`, so they were already in
the tracked set; only their **dumps** were missing. No new rows needed.

`8003D764` (already in `functions/script-vms.md`) and `8001B964` / `8001BE80`
(already in `functions/renderer.md`) are the same case.

## 4. `docs/subsystems/save-screen.md` - the missing entry path

That page documents the save-slot select + write flow `FUN_801DC6B4` but not how
a field session reaches it. It is `FUN_80024190` (new row + detail section in
`functions/game-modes.md`): an 11-state actor SM, spawn descriptor `0x800706BC`,
spawned by the field/world fade SM `FUN_801EE5D4`. It pages the menu overlay in
(`FUN_8003EBE4(4, 0)`), runs **either** `FUN_801DC6B4` (save) or `FUN_801DD35C`
(load) selected by `actor[+0x5C]`, then pages the field overlay (`slot 2`) and
the slot-B pair (`FUN_80025BA0`) back and sets mode 3.

Note the near-miss this corrects: `functions/game-modes.md` already says the
mode-23 CARD actor is `_DAT_8007B8E0`, spawned from descriptor `0x800706D4`.
`FUN_80024190` also writes `_DAT_8007B8E0`, which makes it look like that
actor's `+0x0C` handler. It is not - descriptor `0x800706D4 + 8` holds
`0x801E36A0` (read from `extracted/SCUS_942.54` at file offset `0x60EDC`), and
`0x80024190` appears exactly once in the image's data, at `0x800706C4`, i.e.
descriptor `0x800706BC`.

## 5. Remaining worklist (for the next lane)

After this lane the largest SCUS code gaps are all under ~630 bytes and sit
almost entirely in the statically-linked PsyQ band:

```
0x8006DB54..0x8006DDC8   628 B    0x8004DA00..0x8004DC68   616 B
0x800198E0..0x80019B28   584 B    0x8006DE30..0x8006E06C   572 B
0x800480D8..0x80048310   568 B    0x80059068..0x80059280   536 B
0x80020C14..0x80020DE0   460 B    0x800697E0..0x800699AC   460 B
```

`ghidra/scripts/dump_scus_gaps.py` takes them directly: append to `RANGES` and
run it against `SCUS_942.54`. Un-analysed sub-runs inside a range are reported
as "un-attributed" and go in `FORCE_RANGES`, which disassembles them and creates
one function per `jr ra` + delay-slot unit. **Check the bytes are MIPS first** -
forcing data produces convincing garbage.

## 6. `docs/tooling/ghidra.md` - script-catalogue row to add

`ghidra/scripts/dump_scus_gaps.py` is new and outside Lane 1's doc scope, so it
is not in the catalogue yet. Suggested row for the **Per-function dumps** table:

> | `dump_scus_gaps.py` | Dumper for the disc-denominated **code gap** worklist
> `scripts/ci/disc-coverage.py` emits. Takes address `RANGES` rather than entry
> points: walks the listing per range, dumps every function entry inside, and
> reports the bytes no function covers. Those go in `FORCE_RANGES`, which
> force-disassembles the run and creates one function per `jr ra` + delay-slot
> unit - the shape a run of separately-emitted library leaves has, where
> `force_disasm_dump.py`'s one-entry-per-address model needs the entries known in
> advance. `in_program()`-guarded, so it is safe to run against any program. |

## 7. Baseline

`scripts/ci/disc-coverage-baseline.json` is **not** touched by this lane, per
the wave brief. It needs re-ratcheting at integration; the current measured
value is `SCUS_942.54` code `94.4%` (341010/361126).
