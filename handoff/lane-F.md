# Lane F - the burst boundary, closed; two battle-band rows off the worklist

Two assignments. The first was the stated boundary in `engine-vm::battle_burst`:
the two "spawn parameter tables" whose addresses the port carried and whose bytes
it did not. That boundary turned out to rest on three premises, and **all three
were false** - which is the substance of this lane. The second was the remaining
battle-band worklist rows.

## 1. The burst tables

### The premise that dissolved it: they are not tables

`0x801F5DA4` and `0x801F5D0C` are **move-VM stager records** in the shared
move-buffer format `[i16 model_sel][u16 flags][move-VM bytecode]` - the format
`docs/subsystems/move-vm.md` already documents under "Move-buffer record
sources", and the same shape `legaia_asset::summon_overlay` and
`legaia_asset::scene_event_scripts::move_stager_records` already parse for the
summon and per-scene stager tables.

The chain that gets you there is entirely in `funcs/80021b04.txt`, which was
already dumped. `FUN_80050ED4` forwards its third argument untouched to
`FUN_80021B04`, and that function dereferences it exactly twice: `lh` of word 0
as the four-way seat selector, and `sw` of the pointer itself into `actor[+0x48]`
- the move VM's buffer base. So the "table" is a move program by construction,
and its layout is defined by `FUN_80023070`, not by anything in the burst.

`BurstRecord::parse` now slices an arm's record out of a supplied `0898` image
and walks its extent by driving the **ported** move-VM dispatcher until HALT, so
the opcode-size table is not restated. No bytes are committed; the six
image-gated assertions in `crates/engine-vm/tests/battle_burst_real_records.rs`
are all structural (extent, terminator, opcode sequence, which single word
differs).

What the records are: `model_sel = -1`, `flags = 0`, 65 u16 words each, opening
on `0x39 RENDER_BANK_SET` / `0x15` / `0x23` (render-mode-2 child spawn) / `0x0C`,
then a strictly alternating `WAIT_SET` / `0x24` sprite-add strip, then HALT. The
`0x24` deltas walk a texture atlas in fixed steps with one row wrap - a
frame-per-wait sprite-sheet cycle.

**The two arms' records are identical except for one halfword**: operand 8 of the
`0x23`, which lands in the child's `+0xB2`. What `+0xB2` means under render mode
`2` is *open* - the ported actor tick names `+0xB0`/`+0xB2` for the mode-`5` SFX
emitter arm, which is a different mode, so I did not carry that reading across.
That is the one live question this lane leaves.

### The second premise: `FUN_80050ED4` is not undecoded

`ghidra/scripts/funcs/80050ed4.txt` exists - 23 instructions. It scans the
0x60-slot pointer pool at `DAT_801C90F0`, forwards the same four arguments to
`FUN_80021B04` (sign-extending the low halfword of the fourth via the
`sll 16` / `sra 16` pair straddling the `jal`), stores the returned pointer in
the first null slot, returns it, or returns `0` when all 96 are taken. The port
catalog already carries it as `subsumed_glue`, so it is **not** a worklist row
and I did not tag it - `// REF:` only. Its decompiled C drops all four arguments,
which is presumably how it read as opaque.

The one behavioural consequence I did model: a full pool returns `0`, so
`BurstHost::spawn` now returns `Option<u32>`. Retail would fault on the two
post-spawn stores; the port lets a host represent exhaustion without inventing
that fault, and a test pins that the RNG stream is consumed in full either way
(the `mfhi` chains sit between the `jal`s with no branch on the return).

### The third premise: the first argument is not a "record"

It is an **actor**, and the caller is the move VM. `FUN_801F30C4` is opcode
`0x17`, the battle-side twin of the field escape `0x2F` -
`docs/subsystems/move-vm.md` already said so. That reframes every field the port
had named by shape:

| burst writes | destination | what it is |
|---|---|---|
| scratch `+0x12` | `param_2[1]` → child `+0x96` | the child's **heading**, masked to 12 bits by the seater; the index move-VM op `0x03` rotates by |
| `sh $a1, 0x3e($s0)` | child `+0x3E` | middle of the `+0x3C..+0x40` triple |
| `sh $v0, 0x18($s0)` after `addiu $s0, 0x80` | child `+0x98` | middle of the `+0x96..+0x9A` triple - written *after* the seater's clear, so it survives |

### How the burst is reached, and a correction for `level-up.md` (not my scope)

Each record is preceded in `0898`'s tail by an **18-byte trigger** move program:
`WAIT_SET 0 / 0x17 <mode> / WAIT_SET 0 / HALT`, one alignment word before the
record. The trigger's operand matches the arm whose record follows -
`0x801F5D90` carries mode `0` and precedes `0x801F5DA4`; `0x801F5CF8` carries
mode `1` and precedes `0x801F5D0C`. Both verified by parsing them out of the
image (`each_trigger_record_fires_the_arm_it_precedes`).

`docs/subsystems/level-up.md:267` lists exactly those two addresses as "Binary
animation tables passed to particle spawner `FUN_80050ED4`". **Both halves are
wrong**: they are move programs, not tables, and they do not reach the spawner -
the `0x17` inside them calls `FUN_801F30C4`, which does. The consistent `-0x14`
skew from the real record addresses is the tell. That page is outside this lane's
file scope; the corrected reading is written up at
`docs/reference/functions/battle.md#801f30c4` for whoever owns it.

`docs/subsystems/move-vm.md`'s op-`0x17` paragraph is also now understatement -
it says the escape exists; it could say what it does. Also out of scope.

### Corrections to the port itself

Three, all from re-reading the disassembly rather than from the handoff:

1. **The scale argument is per block, not uniform.** Blocks `0` and `1` load the
   parent's `+0x72` with `lhu` and pass `>> 1` in the `jal`'s delay slot. Block
   `2` loads it with `lh` and passes it **unshifted** - its delay slot carries
   `move $a1, $s5` instead. Both arms agree, so it is the block that decides. The
   old port halved unconditionally. `SpawnBlock::scale_halved` /
   `child_scale`; the test probes `0x8000`, where `lhu >> 1` and `lh` differ in
   sign as well as magnitude.
2. **Only the diagonal arm masks the LUT index.** The cardinal arm is a bare
   `sll $s1, $s2, 0xb`; `andi 0xfff` appears only at `0x801F32A8`. Inside the
   four-iteration bound the two agree, which is exactly why it needed modelling
   rather than assuming - `lut_index` now reproduces the asymmetry and a test
   pins it at `iteration = 4`, where it becomes visible.
3. **The loop bodies are 260 instructions, not 258, and twelve differ, not ten.**
   Three of the twelve are the loop-latch shape. Diff script:
   normalise arm-1 branch targets by the arm offset and compare instruction by
   instruction.

The **fourteen** reciprocal (magic, shift, divisor) triples are now verified
inside the test suite rather than in prose - reproduced in retail's own form
(`hi(x*magic) >> shift` minus the sign word, plus the `0x88888889`
magic-with-add variant) and checked against `x / d` over a dense band and the
32-bit boundaries. The prior note's constants were all correct; what was missing
was that the check lived outside the repo. Note `0x2AAAAAAB` appears at **four**
different shifts in this one function (`/6` bare, `/48`, `/96`, `/192`) - the
sharpest possible illustration of why the shift cannot be dropped.

## 2. Worklist rows

Regenerated from the worktree (with `ghidra/scripts/funcs` symlinked, which is
what makes a worktree run report a real `dumped` count). **61 → 59**; ports
789 → 791. `--live-audit` reports **0 undisclosed inert ports**, and none of the
12 stale-`NOT WIRED` rows are mine.

The two rows I took are the only battle-band ones on that list. The rest split
`runtime-libs.md` / `audio.md` (the `0x80057xxx..0x80064xxx` band, audio lane),
fishing (`801d56e4`, `801d7030`, `801d765c`), field (`801d4a60`), and
game-modes / asset-loader / save-screen (`8002149c`, `8002174c`, `80021934`,
`80024190`).

### `80055B4C` → `engine-vm::battle_stream_slot::StreamSlotSm::arm`

Already implemented and already exact - it was tagged `// REF:` rather than
`// PORT:`, which is why it stayed on the worklist. Promoted, with the disclosure
and three tests. Two facts worth having: both stores are `sb`, so arming slot
`0xFF` wraps the request byte to the idle value and **disarms** (retail has no
guard; no caller reaches it), and `arm` is the exact inverse of the same module's
`decode_request` over the whole armable range - now a property test rather than
an adjacency.

### `801CE844` → `engine-vm::gameover_banner`

Game-over overlay init, PROT `0902`, reached only from mode 18, which nothing
statically writes - a dev harness. **Read it out of the `0902` image**: the same
VA sits inside `0898`'s footprint and the dump taken there is `NOFUNC` plus a
garbage window, which is this band's usual trap.

Three phases; two are a deliberate non-port (GPU/heap reset + `gameover.pak`
stream, then a chunk-walk installing assets - host emission with no arithmetic of
its own, so transcribing it would be a fake port). The third is a clean
renderer-free kernel and is what is ported: nine fixed slots on a line, one child
actor per non-blank slot, all nine sharing **one** move record whose `model_sel`
the loop rewrites per letter (`model_sel = glyph - 0x3F`).

Two behaviours live in delay slots and are invisible in the C:

- the pen advance runs on **every** slot including the skipped blank, which is
  what keeps the label's two words evenly spaced instead of butted together;
- the stagger accumulator runs only in the spawn arm, so the wait timers count
  **letters**, not slots.

Both are separate tests, written from the delay-slot placement. The layout
constants are self-justifying: nine slots at `0x1C2` from `-0x708` put the centre
slot exactly on the origin, and that symmetry is asserted rather than described.
The label bytes are disc data and are a parameter, not a constant.

## 3. One correction outside the burst

`docs/subsystems/battle.md`'s ground-tile paragraph said "each cell samples one
sub-tile with a per-cell random corner mirror" over "two distinct variants
duplicated across the row". Lane 3 falsified that from `FUN_801D02C0` but could
not fix it (the page was outside its scope; it is inside mine). Fixed: the emit
loop runs 2×2 per visible cell advancing the sub-tile row pointer by `0x10`, so
the tiling is **deterministic** and there is no RNG in the routine at all. The
random corner mirror is real but belongs to the particle scatter
`FUN_801E0080`.

## 4. A port defect found on the way: `SpawnSubmode`

`move_vm::spawn::SpawnSubmode` documented itself as "the four-way branch in
`FUN_80021B04`". Retail is **two independent two-way branches**: `bltz` on the
sign at `0x80021BD8` picks the OBJECT-table rebuild, and `sltiu (word - 0x4000),
2` at `0x80021CC0` picks the keyframe/tween arms - whose *else* branch is the
render-scratch clear at `0x80021D3C` that also seeds `+0x96` from `rot[1]`.

A negative init word is negative **and** outside `{0x4000, 0x4001}`, so it takes
both. The doc said `Negative` "skips the type-keyed slot clear". It does not -
and that is not academic here: `model_sel = -1` is what both burst records use,
and the `+0x96` write inside that clear is where the burst's whole heading
computation lands.

Fixed as documentation plus two additive predicates
(`clears_render_scratch`, `rebuilds_object_table`) that name the two branches
separately. **No signature or variant changed**, deliberately - `engine-core`'s
`MoveSpawnHost` impl consumes this enum and I am not in that crate. Whoever owns
`engine-core::actor_alloc_host` should check its `apply_move_spawn_state` applies
the render-scratch clear on the `Negative` arm; if it keys off the four-way
reading it is missing the `+0x96` seed for every transform-node spawn.

## Files

- `crates/engine-vm/src/battle_burst.rs` - rewritten; parser, per-block scale,
  LUT asymmetry, reciprocal verification, renamed fields.
- `crates/engine-vm/tests/battle_burst_real_records.rs` - new, image-gated.
- `crates/engine-vm/src/gameover_banner.rs` - new.
- `crates/engine-vm/src/battle_stream_slot.rs` - `arm` promoted to a port.
- `crates/engine-vm/src/move_vm/spawn.rs`, `move_vm/host.rs` - corrections.
- `docs/reference/functions/battle.md`, `docs/subsystems/battle-action.md`,
  `docs/subsystems/battle.md`.

`cargo test -p legaia-engine-vm --release`: 1686 unit + 6 image-gated, all pass.
`fmt`, `clippy -D warnings`, `check-doc-density`, `check-md-links`,
`check-port-tags` all clean.
