# Falsified RE readings - do not re-walk

Hypotheses about Legaia's runtime that were disproved, kept with their
reasoning intact. The reasoning is the deliverable: each of these is a
*plausible* reading of the bytes, and knowing why it is wrong is worth more
than the row it occupies.

Rows here are terminal. If new evidence reopens one, move it back to
[`open-rev-eng-threads.md`](open-rev-eng-threads.md) rather than editing the
verdict in place - the falsification trail is what makes the row useful.

Two falsification classes recur often enough to name up front. **VA aliasing**:
a bare virtual address is not an identity, because slot-A and slot-B overlays
host different code at the same VA, so a dump labelled by address can be a
different function entirely. **Ghidra's collapsed switch**: a jump table's arms
can render as bare `break`s or as fake `FUN_x` calls, inventing opcode
semantics that the raw table does not have. Both have produced multiple rows
below.

## World map / kingdom bundles

| Thread | Verdict | Why |
|---|---|---|
| Slot-4 → cluster-A converter site | falsified | There is no slot-4 → cluster-A converter. The cluster-A pool (`DAT_8007C018`) is filled exclusively by `FUN_80026B4C`, reached only from `FUN_8001f05c` **case `0x02`** (TMD pack) and **case `0x09`** (bare TMD). Slot-4's type byte is **`0x05`**, whose `FUN_8001f05c` case merely allocates the MOVE buffer `_DAT_8007B888` and never calls `FUN_80026B4C`. So slot-4 bytes never become cluster-A TMDs; the `DAT_8007C018` kingdom entries are the scene's own type-`0x02` field-file TMD pack(s), installed by the single `FUN_80020224` descriptor-walk. |
| World-map outline / coastline reading | falsified | Visual inspection plus the slot-4 record-semantic work refuted the "world-map overlay outlines / coastline wireframe" interpretation. Bodies are most likely small object-local 3D meshes; treat any future "kingdom border lines" claim with suspicion. |

## Battle / arts / level-up

| Thread | Verdict | Why |
|---|---|---|
| Move-VM op `0x2F` extension dispatcher - per-overlay copies? | falsified (one copy, field overlay 0897 only) | The **capture-derived** `_801d362c` dumps are identical to each other (0897 observed under world-map / dialog / cutscene scenario labels); the `0897` **static** dump is a strict *subset* of them, not a byte-identical twin (Ghidra could not follow the JT flow). Substance is unchanged: every other mapped slot-A overlay + the title overlay carries unrelated bytes at the fixed call VA and no JT at `0x801CE868`, so op `0x2F` is executable only while 0897 is resident and battle-side move records cannot use it. See [move-vm-overlay-ext.md](../subsystems/move-vm-overlay-ext.md#overlay-residency---one-copy-in-the-field-overlay-only). |
| "`FUN_801F3894` spirit/magic damage roll" (state-`0x3D` chain caller) | falsified (VA-aliased dump) | The `overlay_0897_801f3894` dump is `FUN_801DD0AC` byte-for-byte under a double VA shift, so the already-ported damage kernel surfaces at a fake entry VA. The real state-`0x3D` callee `FUN_801F3990` is a cast **audio-cue dispatcher**; spirit damage is state `0x3E`'s inline formula. **Corollary, widened: `801Exxxx` dumps are suspect too, not just the `0x801F` band** - `801f0348` and `801e23ec` are settled casualties, the latter's aliased reading having dropped all three initiative modifier terms; `0x801F1ED4`/`0x801F45A4` unverified. See [battle-formulas.md](../subsystems/battle-formulas.md#initiative-key-seeding-fun_801da780). |
| Navmesh / per-scene navigation data | falsified | `0x80108EA4..0x80109550` is per-scene GPU primitive scratch, not a 24-byte stride navmesh. Pointer hunts find zero RAM cells pointing into the window. Real per-scene region / collision / event-trigger data lives in the field-file preamble (a count + `u16` offset table + records - **not** the field-pack schema slots, which are a global-constant template; see [field-pack](../formats/field-pack.md)); the collision grid is the `+0x4000` MAP region; the encounter-record path lives at `actor[+0x94]`. |
| Op-`0x4E` sub-ops 4..8 "absolute jump" / "rand -> next PC" readings | falsified (all sub-ops 0..9 are the 7-byte compare-and-skip) | [details ↓](#op-0x4e-sub-op-family---every-sub-op-09-is-a-compare) |
| `801d58f0` / `801d63b0` as single shared port blockers | falsified (VA-aliasing artifact) | The two addresses host different code in different overlays (byte-verified: 80/228/124/308/1 B and 208/1036 B across 0897/baka/cutscene/debug-menu/fishing/slot/dance) - the port-catalog's bare-VA keying aggregated their refs into phantom top blockers. Tracked per-overlay via `overlay_<label>_<addr>` identities; catalog ignore category `va_aliased_overlay_local`. |

### Op-0x4E sub-op family - every sub-op 0..9 is a compare

*Status:* falsified ("absolute jump" 5..8 and "rand -> next PC" 4 were Ghidra's collapsed switch)

The raw 12-entry jump table at `0x801CEE30` (field overlay, PROT 0897 file `+0x618`) routes
**every** sub-op 0..9 to a value loader that joins the shared 7-byte compare-and-skip
continuation at `0x801E0B40`:

| sub | loader | state value |
|---|---|---|
| 0 / 1 | `0x801E0A40` / `0x801E0A70` | char-record HP / MP `(cur, max)` pair - the only scaled form (`max * arg >> 8`) |
| 2 | `0x801E0AC0` | char level byte `+0x130` |
| 3 | `0x801E0AEC` | party gold `_DAT_8008459C` |
| 4 | `0x801E0AFC` | **BIOS `Rand() & 0xFF`** - a random-chance branch |
| 5..8 | `0x801E0B0C` | **slot table `0x801C6460[sub - 5]`** (s16; the read side of the `4C CA/CB/CC` slot writes) |
| 9 | `0x801E0B34` | coin bank `_DAT_800845A4` |

Sub-ops 10/11 keep the 9-byte u32 gold/coin form; 12..15 fall through (PC += 7). The decompiled
bare-`break` arms for 2..9 were the collapsed switch - each raw loader ends `j 0x801e0b40` /
`j 0x801e0b3c` with the operand pointer staged in the delay slot (the same class of trap as the
label-call idiom). Disassembler + executing VM corrected: `field_disasm::decode_subops` (single
0..=9 compare arm), `engine-vm` `field/step/flow.rs` + `FieldHost::op4e_char_level` /
`slot_table_read`. cave01's `P2[12]` spawn gate is the live sub-5 exemplar.

## Field / locomotion

| Thread | Verdict | Why |
|---|---|---|
| "~270 undumped field-overlay functions" (recomp dispatch-entry seed list) | falsified (not a function inventory gap) | [details ↓](#270-undumped-field-overlay-functions-recomp-dispatch-entry-seeds) |

### 270 undumped field-overlay functions (recomp dispatch-entry seeds)

*Status:* falsified - the list is not a function inventory, and the inventory gap it implied does not exist.

A PSXRecomp runtime capture of the slot-A overlay window during a boot-to-town play
session yielded ~312 "call targets" in the `0x801CC000+0x29000` band, ~270 of them
absent from `ghidra/scripts/funcs/` + [`functions.md`](functions.md) - read at the
time as a large undumped-function backlog for PROT 0897. Triaging every address
against the disc overlay images and the captures' own resident bytes falsifies the
premise on three independent axes:

- **They are dispatch entries, not call targets.** The recomp's capture seeds record
  every PC where its dispatcher entered interpretation: indirect-call targets, but
  also **return sites** (the instruction after a `jal`+delay-slot), **interrupt-resume
  PCs** (arbitrary mid-loop addresses, weighted by hot loops), and `jr`-table case
  labels. Against the resident image, only ~1/4 of the entries classify as
  call-shaped at all; the rest sit mid-function or mid-loop.
- **The PC tables span overlay generations; only the byte snapshot is coherent.** The
  capture accumulates PCs across the whole session (title → FMV → menus → field), so
  a "field window" list mixes title-overlay, cutscene-overlay (0970) and menu-era
  PCs with field-era ones. Smoking guns: one source capture's resident bytes match
  the disc 0897 image at only ~16% (title-era, different occupant); dozens of listed
  PCs land inside 0897's **data head** (debug strings + pointer tables - impossible
  as 0897 code); and two entries the list marked as already-known resolve to the
  cutscene overlay's STR dispatch `FUN_801CEA3C` and the actor-VM jump *table*
  `0x801CED70` - a different overlay's function and a data address.
- **No image claims them as functions.** Sweeping all mapped slot-A overlay images +
  the slot-B field library for prologues / static `jal` targets at the listed
  addresses yields only two coincidental hits (both in the never-resident
  slot-machine image) and a handful of `j`-target labels.

The durable lessons: seed lists from a recomp's interpreter dispatcher need
**per-hit resident-image resolution** (e.g. a mode-gated `dirty_exec_hot` window)
before any identity claim, and a "new function" claim needs a prologue or a
static-call witness in the image that was actually resident. The real undumped-code
question for 0897 is better served by the [port-catalog dashboard](../tooling/port-catalog.md)
than by this list.

## Related pages

- [`open-rev-eng-threads.md`](open-rev-eng-threads.md) - the live hunts.
- [`re-settled-threads.md`](re-settled-threads.md) - the answered questions, each with an evidence grade.
- [`docs/tooling/ghidra.md` § decompiler artifacts](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims) - the seven C-rendering artifacts that produced several of the readings above.
- [`docs/tooling/call-target-integrity.md`](../tooling/call-target-integrity.md) - why a decoded `jal` target is a property of the bytes, not the load base.
