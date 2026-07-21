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
| Charm battle softlock = unbounded reroll in `FUN_801E7320` | falsified (cannot spin from any reachable state) | The reroll loops are unbounded in isolation, but every reachable caller state has an exit: the scheduler `FUN_801DABA4` never seeds a dead actor (predicate `+0x14C != 0 && !(+0x16E & 0x4)`), the acting `0x380` monster is itself an in-band self-pick exit (`0x801E73E8` clears `+0x1DE`), and a band with zero living members means the previous `0x5A` already fired the wipe. The real defect is downstream in the `0x5A` victory arm's roster indexing ([battle.md](../subsystems/battle.md#enemy-ally-charm-at-the-end-of-action-gate-the-charm-battle-softlock)). Lesson: an unbounded loop hangs only under a reachable all-invalid state - check the predicates feeding it first. |

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

## Audio / sound driver

| Thread | Verdict | Why |
|---|---|---|
| `FUN_80068D94` as "`SsSepOpen` / SEP loader" (with `FUN_80068B98` as "`SsSeqOpen`") | falsified (it is the VAB-open head) | The plausible part: it validates a magic, reads a count at `+0x12`, `SsSpuMalloc`s, and patches a pointer table - the shape of a SEP/track loader, with the magic read as 'VAP'. The disassembly refutes it: the compare is `0x564142` against `word >> 8` plus low byte `0x70` - `pBAV`, the **VAB** magic - and `+0x12` is `ps`. The "per-track pointer table" is the ProgAtr table receiving the program → packed-tone-page rank map ([`vab.md`](../formats/vab.md#program-slots-vs-packed-tone-pages)); the mislabel hid that map, and with it the engine's tone collapse on sparse banks. Correct roles: [`audio.md`](../subsystems/audio.md#ssapi-seq-management-layer-above-libspu). |

## Field / locomotion

| Thread | Verdict | Why |
|---|---|---|
| "~270 undumped field-overlay functions" (recomp dispatch-entry seed list) | falsified (not a function inventory gap) | [details ↓](#270-undumped-field-overlay-functions-recomp-dispatch-entry-seeds) |
| Field-VM op `4C` nE sub-3 "syncs the resolved actor's position to the active camera" | falsified (copy direction inverted) | Plausible because the handler tail (`0x801E3178..0x801E31AC`) really does refresh the camera-scroll globals - but that tail is a player-ctx-only side path. The op body (`0x801E3108`) copies the operand-resolved actor's `+0x14/16/18` position and `+0x26` facing **into the executing ctx** - it is the seat primitive of every mid-visit crowd swap (dolk2 `P2[11]`'s eight `CC <crowd> E3 <day>` pairs). Reading the tail as the op's purpose inverted the semantics. See [script-vm.md](../subsystems/script-vm.md#mid-visit-npc-re-arrangement-beats-dolk2-market-swap--garmel-boss-staging). |
| Extraction-0874 §2 F-variant pixels are written by a pause-menu-path uploader | falsified (no menu image transfer exists) | Plausible because six of six pause-menu captures held the variant while the casino prize shop (same mode `0x17`) held disc bytes. But a GPU-op trace (every DMA2 kick chain-walked + GP0 PIO stores hooked) shows the whole pause walk issues **zero** image transfers, and a 49-state census finds plain field saves carrying the variant with no menu in their lineage - the correlation was session history. The real writer is `FUN_80021DF4`'s dispatch-4 VRAM wrap-scroll arm ([details](open-rev-eng-threads.md#extraction-0874-2-playerlzs-f-variant-pixels---a-parked-vram-wrap-scroll-phase-not-a-menu-writer)). |
| Prologue gold grade = per-node `+0x74`/`+0x78` depth-cue crush | falsified (grade is a palette-space collapse; the nodes carry no `IR0`) | Plausible because `FUN_8002735C` really does load per-node DPCS far colour + `IR0`, and the motion/move VMs carry op `0x0C` writers of those fields - but the opening never uses them: a live recomp capture reads node `+0x78` (`IR0`) = **0 on every node at every beat**, and the `opdeene` MAN motion section has no op `0x0C`. The real mechanism is a load-time CLUT/TMD palette collapse `L=max(r,g,b) -> (L, max(L-1,0), L>>1)` ([cutscene.md](../subsystems/cutscene.md#full-scene-sepia-grade-the-gold-prologue-look)); the far-field crush is that law seen through dark authored gouraud. |

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

### `FUN_801F12D0` read from the `overlay_0897` dump

**The claim that doesn't survive:** that the readef/summon applier's slot
sequencing can be read out of `ghidra/scripts/funcs/overlay_0897_801f12d0.txt`.

`FUN_801F12D0` has dumps under several overlay labels because `0x801F12D0` falls
inside more than one overlay's load window. The `overlay_0897` one is a
**mid-function fragment**, and it is a fragment in the way that actually proves it:
it opens at `801f12d0 lw v1,-0x6c84(v0)` with no `addiu sp,sp,-N` anywhere in the
window, yet closes restoring `s0`-`s3` and `ra` from a frame it never established.
Callee-saved reads with no matching save, plus a missing prologue in the
**disassembly**, is the fragment test.

Its 47 instructions contain none of the slot-streaming logic - no `+0x277`
base-slot read, no bit-7 file test, no `base+2` / `base+3` staging arms. A reader
who takes it for the whole function concludes the applier does something else
entirely, and the `jal 0x801daba4` in its tail is close enough to the real control
flow to make that conclusion look plausible.

**Read instead:** `overlay_muscle_dome_801f12d0.txt` - 330 instructions, proper
prologue, carrying the bit-7 test at `801f1644` and both staging arms.

**Generalises to:** any VA that several overlays map. The instruction count in the
dump header is the cheap first filter - a 47-instruction "function" that restores
four callee-saved registers is not a function. The corpus-wide picture is in
[`dump-corpus-integrity.md`](../tooling/dump-corpus-integrity.md).

## Related pages

- [`open-rev-eng-threads.md`](open-rev-eng-threads.md) - the live hunts.
- [`re-settled-threads.md`](re-settled-threads.md) - the answered questions, each with an evidence grade.
- [`docs/tooling/ghidra.md` § decompiler artifacts](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims) - the seven C-rendering artifacts that produced several of the readings above.
- [`docs/tooling/call-target-integrity.md`](../tooling/call-target-integrity.md) - why a decoded `jal` target is a property of the bytes, not the load base.
