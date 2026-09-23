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
| A kingdom scene reaches `FieldSceneAnim::ocean_only` | falsified (it is the arm for a bundle whose slot-5 CLUT-walk table fails to parse) | Plausible: the arm is named for the ocean and lives on the kingdom path, so a ladder into `map01` looks like the way to enter it. All three retail kingdoms ship a slot-5 table, and all three install the CLUT walker; only a modified or damaged disc reaches the fallback. |
| Slot-4 → cluster-A converter site | falsified | There is no slot-4 → cluster-A converter. The cluster-A pool (`DAT_8007C018`) is filled exclusively by `FUN_80026B4C`, reached only from `FUN_8001f05c` **case `0x02`** (TMD pack) and **case `0x09`** (bare TMD). Slot-4's type byte is **`0x05`**, whose `FUN_8001f05c` case merely allocates the MOVE buffer `_DAT_8007B888` and never calls `FUN_80026B4C`. So slot-4 bytes never become cluster-A TMDs; the `DAT_8007C018` kingdom entries are the scene's own type-`0x02` field-file TMD pack(s), installed by the single `FUN_80020224` descriptor-walk. |
| World-map outline / coastline reading | falsified | Visual inspection plus the slot-4 record-semantic work refuted the "world-map overlay outlines / coastline wireframe" interpretation. The replacement guess - "small object-local 3D meshes" - is falsified too: slot 4 is the scene's animation bank (below). Treat any future "kingdom border lines" claim with suspicion. |
| Walk-view decoration layer = walk-bit (`0x1000`) cells plus a `FLAG_MESH_DRAWN` test | falsified (the gate is `0x2000` alone) | Plausible: every tree / prop cell carries both bits, and the one family the walk-bit sweep wrongly admitted (record 408, a wall down every river) happened to lack flag `0x2`, so a flag test "fixed" it. But the resident kernel (`FUN_801F69D8`, PROT 0901, `andi 0x2000` at `0x801F6ECC`) tests only the draw bit, never `+0x12 & 0x2` - and the big enterable mountains sit on cells with `0x2000` and **no** `0x1000` (the mesh is the ground there), so the walk-bit sweep drew every tree and dropped every mountain while looking complete ([world-map.md](../subsystems/world-map.md#placing-the-continent-terrain-engine-port)). |
| `FUN_80024E08` performs the `0xF0` model-id split | falsified (the split is in its callers) | Plausible: the resolver is the routine that turns a model id into a pool pointer, so the id space's one discontinuity looks like its business. It does no splitting. Two callers do - `0x800393B8`, and `0x8003A2DC` inside the placement installer `FUN_8003A1E4` - each picking `DAT_8007B824` or the scene base `DAT_8007B6F8` before the call. A port tag naming the resolver for the split describes a routine that never sees the fork. |
| `FUN_80026B4C` is the TMD pointer table's only writer | falsified (PROT 0976 writes it directly) | Plausible: the registrar is the only routine that *installs* an entry, and every install path does go through it, so "only writer" and "only installer" predict the same thing everywhere an install happens. `0x801CF1E0` in PROT 0976 stores a zero into a slot without it. A liveness rule that enumerates the registrar's calls therefore has a hole exactly where a slot was cleared rather than filled. |

### Slot 4 is a GTE vertex pool walked by an unpinned "cluster-A command stream"

*Falsified by disassembly, with a live state agreeing.*

The reading: each 8-byte slot-4 record is a vertex `(i16 x, y, z, attr)` in
an object-local pool, indexed by a command stream nobody could find, drawn
in place by the world-map render library; `attr` render-unused.

Why it looked right: `FUN_80044C14`, a real per-kind primitive handler, does
load two words into the GTE vertex registers exactly that way, and a capture
showed the slot-4 window read in place with GTE-shaped return addresses.

What is true: there is no stream. `_DAT_8007B888` (the type-`0x05` buffer)
has six references image-wide, none in a render path; the entry's field
boundaries fall on **nibbles** (three 12-bit translations, three 8-bit
angles); `attr` is the Y / Z rotation, read every frame; and the capture's
return addresses sit inside the animated-mesh renderer `FUN_8001B964`, not a
prim handler. The `attr`-is-unread register-width argument swept a function
that never sees these bytes. See
[`re-settled-threads.md`](re-settled-threads.md#kingdom-slot-4---per-record-semantic).

### The world-map slot-4 landmark meshes are the consumer of the lit prim handlers

*Falsified twice over.*

The renderer's kinds 8..11 are the only handlers with an `NCC*` light op, and
the presumed consumer was the kingdom bundles' slot-4 "landmark meshes". Slot
4 is the scene's animation bank, not a mesh library; and exec breakpoints on
all four handlers over two kingdom overworlds, a field scene and a battle
return zero while the overworld's real handlers (PROT 0901's eight
replacements) fire in the same run. A kingdom overworld never enters the SCUS
prim-dispatch family at all. See
[`re-settled-threads.md`](re-settled-threads.md#battle--arts--level-up).

## Battle / arts / level-up

| Thread | Verdict | Why |
|---|---|---|
| The in-battle steal is a command, or rolls on the hit | falsified (it rolls on the kill, once per strike chain) | Plausible: the steal table is per-monster like the drop table, and "steal" reads as an action a character takes. The roll lives in `FUN_8004AD80`'s knockdown-end arm, and the latch it sets before looking at the killer means an action's first kill spends that action's attempt ([settled](re-settled-threads.md#battle--arts--level-up)). |
| The steal latch `ctx[+0x27]` is set once per battle and has one writer | falsified (the action SM clears it at every strike chain's exit) | Plausible: a byte scan for a store to `+0x27` finds exactly one `sb`. The clear is `sb zero, 0x16(s5)` at `0x801E3A84` with `s5 = ctx + 0x11`, in the delay slot of the `0x1E -> 0x1F` arm - a displacement no `0x27(reg)` scan matches. A write-watch caught it going `1 -> 0` at the chain's end. |
| Hub arms 4 / 5 / 6 draw the ROUND banner when a dome leg opens | falsified (course card + title art; the banner is arm `0x15` only) | Plausible: the envelope has the banner's shape and timing, and the port drew the ROUND card there for as long as nobody read which emitter each arm calls. Arms 4 / 5 call `FUN_801D042C`, arm 6 only drains the backdrop and starts the load. |
| `FUN_801DD4B0` is the physical-damage wrapper and `FUN_801DD6B4` the spell one | falsified (the other way round) | Plausible: the two wrappers are near-identical and sit next to each other. The stat each loads decides it: `+0x168` (INT) in `801DD4B0`, `+0x158` (ATK) in `801DD6B4`. |
| `FUN_801E93C8` re-arms the arts gauge / `FUN_80046870` is a charge gauge | falsified (an anim-rate restore; a cooldown top-up) | Plausible: both run around the art commit, where a gauge would be re-armed. Neither touches the AP gauge's fields. |
| `ctx[+0x28B]` has four writers, none in the battle overlay | falsified (five; the fifth is the tick's own clear in PROT 0898) | Plausible: the four raises in `FUN_8004AD80` were found by a SCUS scan; the clear at `0x801E263C` is a `sb zero` inside the overlay's banner tick, which that scan never read. |
| Which pass draws the ringside still is unsettled on the retail side | falsified (`FUN_801D00F8`, two `POLY_FT4` packets at texture pages `0x106` / `0x109`, measured live) | Plausible: the still's loader was ported before its consumer was traced, and the port's disclosure kept saying so a wave after the contest hub's emitter had been measured. The row is blocked on two port-side pieces, not three. |
| Spine flag `0x142` is set by six records | falsified (two are story writers; four are menu rows) | Plausible: all six decode as clean `51 42` / `61 42` arms in shipped carriers, and a flag-writer census has no way to tell a beat from a menu row - both are the same opcode with the same operand. Four of them are rows of a **developer flag-setting menu**: rikuroa `P1[10..12]` is a nine-flag Set ladder with its mirrored Clear ladder, and dolk2 `P1[1]` / dolk `P1[26]` sit in the same shape. The story writers are rikuroa `P2[50]` and dolk2 `P1[0]`. |
| The cast-arm countdown drains by the `scratch[0x37D] * scratch[0x393]` product | falsified (the multiplier is per arm) | Plausible: one arm really does drain by that product, and a rule read off one arm and checked against a second that agrees looks like the band's rule. Each arm carries its own multiplier of the frame byte at `0x1F800393` - the product, twice it, or once it - so two arms measured as "constant 4" and "constant 8" were `1x` and `2x` a byte that happened to read 4. A per-arm constant and a per-arm multiplier of a varying byte are indistinguishable in a single fight. |
| PROT 0950's arm 6 is frame-gated like its siblings | falsified (it has no gate; it ends on countdown expiry) | Plausible: twelve neighbouring arms in the same band do gate on a frame count, and a run that stopped two ticks short of expiry looks exactly like a gate firing. The arm tests nothing - the countdown simply runs out. The image's real fault is a different arm walking the actor table past a one-monster seat fill. |
| PROT 0905 (Vera) leaves the HP restore to the band's shared heal fold | falsified (it writes HP itself, as a second owner) | Plausible: the band has one heal fold and every other healing module routes through it, so a module that also has the amount looks like a caller rather than a writer. Vera stores the restored HP in its own tick as well, so a port that runs both applies the heal twice per frame. A single-owner posture is the fix; an `amount = 0` on the fold's side is that posture working, not a missing value. |
| PROT 0952 carries no spawn record because its spawn sites load `$a2` from a saved register | falsified (the sites are not 0952's) | Plausible: a pointer arriving in a saved register genuinely does defeat the `lui`/`addiu` recovery, and the image genuinely has no record the parser can bound - so "the parser cannot see it" and "there is nothing to see" predict the same output. Both sites sit in 0952's **inherited tail** and their pairs resolve to `0x801F8348` / `0x801F836C` - two of PROT 0951's records, past 0952's own image end. 0952 has no band because it has no records, not because its records are hidden. |
| The slot-B spawn record's `+0x02` is a flags word | falsified (`reserved`) | Plausible: `[i16 model_sel][u16 ?]` is the shape of a record header, and a second halfword next to a selector reads as flags by default. It is zero in every one of the band's 1027 records and nothing on the disc reads it. |
| Both gates of the `0x801E6218` block jump to `0x801E6814` | falsified (they are conditional branches) | Plausible: the block's two exits do reach the same tail, so a reading that calls them jumps gets the control flow's shape right. They are `beqz 0x801E6158` and `bnez 0x801E6168`, and it is their **not-taken** edges that are the tail's only entry - which matters because the second gate reads the byte `0x801E6210` / `0x801E6214` increments. The LATCHED conclusion the block was cited for survives intact. |
| `FUN_801D65F8` is a positional SFX helper | falsified (it is a VRAM blit) | Plausible: it takes a small packed argument and is called from the Baka Fighter round SM, where a positional cue would fit. It blits a `6 x 0x18` VRAM `RECT` from `(0x340 + (byte0 >> 2), 0x80 + byte1)` to `(0x340, 0x86)` through `MoveImage` - the same error class as `FUN_80058490`: an id-shaped argument into a graphics routine. |
| No dome round raises the magic bit, so magic is *not* forbidden on the Master course | falsified **twice** - the original claim was right and this page's refutation of it was wrong | Every fact in the refutation holds: the `0x200` bit's SCUS writers do key on the first enemy monster id, the dome ladder does top out at monster `0xAA`, and `FUN_801D0088` does write only the low byte. They are not the whole writer set. `FUN_801CEA6C` **seeds the word itself** - `0x101` / `0x111` / `0x321` over a zero at `0x801CEBA0` / `0x801CEBB4` / `0x801CEBC8` on story flags `0x536` / `0x537` / `0x538`, last match winning - so retail bars Item on every seeded course and crosses out the Ra-Seru chip on the top one. **Enumerating one bit's writers is not enumerating the word's.** |
| A cast is an AP spend, so the dome pennant's AP budget pays for one | falsified (a cast costs MP) | Plausible: every other dome command is an AP spend, and the pennant geometry is linear in AP cost, so a magic command looked like one more row of the same table. The cast path `0x801D1408..0x801D1528` writes `+0x1DF[0]`, `+0x1DE = 2`, `+0x1E7 = 9` and phase `0x46` and touches no AP field; the cost is the spell table's `+3` byte discounted by the record's `+0xF4` bits. Any budget argument built on the AP reading is about the wrong currency. |
| A slot-B tick body that sweeps the whole actor row is a never-kill clamp, and only `0x801F85A8` / `0x801F8D64` sweep one | falsified (three more sweep it, and they can kill) | Plausible: the two known whole-row bodies both clamp, so "whole row" and "clamped" looked like one property of one shape. Three **tick** bodies - `0x801F726C`, `0x801F69EC` and `0x801F69D8` - sweep the whole row with the kill-capable shape A instead. Pairing the two properties made a kill-capable body read as safe. |
| Past its `sltiu` bound, a capture-class tick body returns `Done` | falsified (it returns **Busy**) | Plausible: an out-of-range phase index looks like a terminal state, and treating it as `Done` is what an anti-softlock port would want. The saved register is seeded `1`, and 0949's `beqz` at `0x801F6AA4` lands *past* the `move s7,zero` that would clear it, so retail leaves the body busy. The port keeps `Done` deliberately, as a disclosed divergence, on all eleven bodies. |
| PROT 0955's four bodies are the band's only stat-block writers | falsified (eight images write the block) | Plausible: 0955 is the module whose whole choreography is buffs, so it looked like the band's designated stat writer. Also writing `+0x150..+0x16D`: 0940 (`0x801F78B8`), 0942 (`0x801F7D34`), 0943 (`0x801F69D8`), 0945 (`0x801F69F8`), 0954 (`0x801F6A58`), and 0925 / 0956 at `+0x16C` only. |
| `0x801E6218` is an unported multi-cast sweep | falsified (it is latched, and already ported) | Plausible: the address appears in a documented sentence naming it as remaining work, which is exactly the shape a worklist row has. It is a latched arm the port implements as `battle_action::done`, and a five-form reference sweep over 84 images finds nothing that reaches the address at all. The doc sentence was stale, not the port. |
| The eleven player-Seru arms off `0x801CF4EC` are data stagers | falsified (they are tick bodies) | Plausible: the cast-module page's per-entry verdicts really do say "data" for PROT 0903..0913 - but those verdicts describe each module's **stager**, and the `0x801CF4EC` table's arms are a different routine in the same image. Measured, they are 3396..7260-byte `ctx+0x279` phase machines with damage wrappers, and none of the eleven is ported. |
| The battle AI's companion pick reads the acting actor | falsified (it reads seat 0 and writes seat 1) | Plausible: every other per-actor routine in the file takes the actor it acts for. `FUN_801EED1C`'s character-id-4 arm reads `actor_table[0]` and writes `actor_table[1]`, so a table built around the acting actor describes the wrong seat's stats. |
| Selectors `0x10..=0x83` of the battle applier are a stat-up / status-clear / queue-end / item family | falsified (116 of them are the epilogue) | Plausible: a 132-slot jump table with decoded arms at the bottom reads as a large opcode space with an undecoded upper half. The table at `0x80014FA0` has 15 distinct targets, and 116 slots point at the shared epilogue `0x800421A8`. The only body above `0x0E` is slot `0x82` (`0x800421A0`), a brightness ramp already ported. |
| A learned skill is inserted at the head of the displayed list | falsified (both list writers insert in order) | Plausible: a head insert is the cheap implementation and would explain "newest first" in a menu. The party-slot learned-Arts list (`+0x74D`, selector `0x0B`) and the displayed-skill list (`+0x186`, arm `0x80041FB4`) both walk to an ascending position first. |
| `FUN_80043264` scans all eight equipment slots | falsified (three) | Plausible: the routine sits beside the equip helpers and the block it indexes is eight slots wide. Its counter starts at `li v1,0x5` and runs while `slti v1,0x8`, so it reads `char +0x19B..+0x19D` - the accessory ("Goods") slots only. |
| `0x801F7D34` is a body of PROT 0943 (`cast_curse`) | falsified (PROT **0942**, `cast_power_up`) | Plausible: the dump is named `overlay_cast_curse_0943_801f7d34`, and a filename is the fastest thing to trust. The bytes are 0942's: an 876-byte four-arm tick for action `0x52` that writes the caster's `+0x156` AGL base as `record[+0x0E] * 3/2`. A dump filename prefix is not evidence of which image a routine belongs to. |
| SCUS's `jal 0x801F7B88` runs only when a battle ends mid-cast | falsified (it runs on ordinary in-battle frames) | Plausible: the call sits in a teardown-shaped neighbourhood, and no corpus state had its gate up, which reads as "a rare end-of-battle path". The arm requires `_DAT_8007BD71 == 0xFF` - battle **running**; `0xFE` is the ending state - and fires per frame while PROT 0920's effect budget is non-zero (123 hits on one victory ladder, every one *before* the end signal). |
| The battle exit (the `0x66` escape teardown / the results sequencer's `+336` frame) is a **white-out** | falsified (a fade to black) | The template is kind `2`, black → white, and the fade actor's tick passes the kind as `FUN_80024EE4`'s second argument, which the emitter folds into the draw-mode packet's ABR bits (`sll a3,a1,0x5; ori a3,a3,0xe` at `0x80024FB0`) - the law the intro styles already obey (`abr == 1` white-out, `abr == 2` to black). Kind 2 is `B - F`, so a rising black → white ramp subtracts more each frame: the scene - result windows included - darkens to black. "White-out" came from reading the ramp's end colour as the screen's end colour. |
| Retail plays a victory **fanfare BGM** when a battle is won | falsified (the battle theme runs through the results; the only jingle is cue `0x50` on a level-up) | The one call in `FUN_8004E568`'s results frame that looked like a music start, `FUN_8003CE08(0x35)` at `0x8004EECC`, is the story-flag setter (`DAT_80085758[idx >> 3] \|= 0x80 >> (idx & 7)`). The frame's only cue is `FUN_8004FCC8(0x50)` behind the level-up test; the hero's "fanfare" is a `monster.snd` voice clip streamed into slot 7. |
| The member who landed the killing blow strikes the victory pose | falsified (the leader poses) | The pose actor is `ctx[+0x13]`; every store to it in the battle overlay is a round-boundary zero or the magic menu's MP-cost scratch, and the three-member `noa_levelup_banner` capture reads `0` with seat 0 posing while Noa levelled. |
| A party melee's whole sound is the `XA27` channel-4 sting (`0x10C`) | falsified (an ordinary swing is the `XA30` grunt; the sting needs `_DAT_8007BD84 != 0`) | `FUN_801EC3E4` selects **one** of the two on `_DAT_8007BD84`: zero takes the grunt at `0x801EEB44` and the re-read at `0x801EEB60` then skips the cue; non-zero branches over the grunt straight into the `0x10C` path (`bne v0,zero,0x801EEB70` at `0x801EEAC8`). The word is zeroed by the battle-start sweep and the round reset and no dumped routine sets it. A second reading - "the grunt fires first and the sting is dropped behind its CD read" - was the decompiled C's flattening of that branch; the two are never both attempted on one strike. |
| Move-VM op `0x2F` extension dispatcher - per-overlay copies? | falsified (one copy, field overlay 0897 only) | The **capture-derived** `_801d362c` dumps are identical to each other (0897 observed under world-map / dialog / cutscene scenario labels); the `0897` **static** dump is a strict *subset* of them, not a byte-identical twin (Ghidra could not follow the JT flow). Substance is unchanged: every other mapped slot-A overlay + the title overlay carries unrelated bytes at the fixed call VA and no JT at `0x801CE868`, so op `0x2F` is executable only while 0897 is resident and battle-side move records cannot use it. See [move-vm-overlay-ext.md](../subsystems/move-vm-overlay-ext.md#overlay-residency---one-copy-in-the-field-overlay-only). |
| "`FUN_801F3894` spirit/magic damage roll" (state-`0x3D` chain caller) | falsified (VA-aliased dump) | The `overlay_0897_801f3894` dump is `FUN_801DD0AC` byte-for-byte under a double VA shift, so the already-ported damage kernel surfaces at a fake entry VA. The real state-`0x3D` callee `FUN_801F3990` is a cast **audio-cue dispatcher**; spirit damage is state `0x3E`'s inline formula. **Corollary, widened: `801Exxxx` dumps are suspect too, not just the `0x801F` band** - `801f0348` and `801e23ec` are settled casualties, the latter's aliased reading having dropped all three initiative modifier terms; `0x801F1ED4` holds (verified from the 0898 image); `0x801F45A4` unverified. See [battle-formulas.md](../subsystems/battle-formulas.md#initiative-key-seeding-fun_801da780). |
| A level-up **refills** the live HP / MP pools (the captures' "settle" phase at `+0x106` / `+0x10A`) | falsified (the settle write is the battle-end resync; both currents stand still) | [details ↓](#a-level-up-is-not-a-heal) |
| A streamed signature-attack cast module cannot run from a party slot because "a party actor has no monster block" | falsified (it has a first-class equivalent) | `FUN_8004AD80` resolves the staged raw anim index down two arms, and the party one (`DAT_801C9360[slot]`) carries the indices PROT 960 stages. The module's only monster-block touch is a hardcoded **seat-0** write unrelated to the caster. [details ↓](#the-cast-module-blocker-was-named-wrong) |
| An art whose attack camera films flat has the wrong **arm**, so the fix is to select a better-choreographed one | falsified (every arm is timed for a ~20-frame swing, and not one is spare) | [details ↓](#the-attack-camera-was-never-an-arm-choice) |
| `FUN_801D5854` case 6's `0x801D5CFC` arm is the per-action **party** framing, gated on `DAT_8007BD71 == 0xFE` as "the in-battle state" | falsified (`0xFE` is the battle-END signal; a running fight takes the `0x801D64C4` arm for everyone) | [details ↓](#the-case-6-party-arm-is-the-battle-over-framing) |
| The Done band (`0x50` / `0x51`) is idle for the camera - keep it on the far framing so the "per-action close-up" does not own half the fight | falsified (retail re-arms case `6` / `8` per category there; the close-up was the wrong arm) | [details ↓](#the-done-band-is-not-idle) |
| Navmesh / per-scene navigation data | falsified | `0x80108EA4..0x80109550` is per-scene GPU primitive scratch, not a 24-byte stride navmesh. Pointer hunts find zero RAM cells pointing into the window. Real per-scene region / collision / event-trigger data lives in the field-file preamble (a count + `u16` offset table + records - **not** the scene texture pack at block `+4` (the former "field-pack schema", which is an `asset::pack` of TIMs); see [field-pack](../formats/field-pack.md)); the collision grid is the `+0x4000` MAP region; the encounter-record path lives at `actor[+0x94]`. |
| Op-`0x4E` sub-ops 4..8 "absolute jump" / "rand -> next PC" readings | falsified (all sub-ops 0..9 are the 7-byte compare-and-skip) | [details ↓](#op-0x4e-sub-op-family---every-sub-op-09-is-a-compare) |
| `801d58f0` / `801d63b0` as single shared port blockers | falsified (VA-aliasing artifact) | The two addresses host different code in different overlays (byte-verified: 80/228/124/308/1 B and 208/1036 B across 0897/baka/cutscene/debug-menu/fishing/slot/dance) - the port-catalog's bare-VA keying aggregated their refs into phantom top blockers. Tracked per-overlay via `overlay_<label>_<addr>` identities; catalog ignore category `va_aliased_overlay_local`. |
| A monster's after-image ghosts (`FUN_80049348`) fire on **any non-idle clip tag** | falsified (the gate is two record bytes, `+0x77` and `+0x87`) | Plausible: the party gate is committed slot `>= 0x11` = "an art is playing", so "ring id = clip tag + 0x10" read as the same idea. But the anim tick stamps `record[+0x77] + 0x10` (`+0x87 == 1` forces `0x11`), and `+0x77` is the attach-key byte, zero on almost every monster entry; no idle entry on the disc qualifies. Gating on the engine's "is a clip staged" proxy ghosted every monster through its approach walk and idle loop (the permanent yellow halo). Lesson: when retail reads a record byte, port the byte, not the state that usually accompanies it ([battle-action.md](../subsystems/battle-action.md#the-after-image-ghost-walk-fun_80049348)). |
| Charm battle softlock = unbounded reroll in `FUN_801E7320` | falsified (cannot spin from any reachable state) | The reroll loops are unbounded in isolation, but every reachable caller state has an exit: the scheduler `FUN_801DABA4` never seeds a dead actor (predicate `+0x14C != 0 && !(+0x16E & 0x4)`), the acting `0x380` monster is itself an in-band self-pick exit (`0x801E73E8` clears `+0x1DE`), and a band with zero living members means the previous `0x5A` already fired the wipe. The real defect is downstream in the `0x5A` victory arm's roster indexing ([battle.md](../subsystems/battle.md#enemy-ally-charm-at-the-end-of-action-gate-the-charm-battle-softlock)). Lesson: an unbounded loop hangs only under a reachable all-invalid state - check the predicates feeding it first. |
| Gaza 2 `0x51` park: clamp asymmetry as a standalone retail generator | falsified (amplifier only; its exhibit was a phased mid-action state) | [details ↓](#gaza-2-0x51-park---the-two-falsified-generators) |
| Gaza 2 `0x51` park: the Final Heal revive lands "at the worst possible moment" (mid-drain) | falsified on the Gaza 2 move set (12/12 revives found the accumulator already drained) | [details ↓](#gaza-2-0x51-park---the-two-falsified-generators) |
| Muscle Dome as a **card battle** with a per-fighter "score out of 108" | falsified (it is a 4-turn battle; the readout is the opponent's HP percentage) | [details ↓](#muscle-dome-was-never-a-card-battle) |
| Muscle Dome awards a **Seru** on a win | falsified (a leg pays nothing; a contest pays casino coins) | [details ↓](#the-dome-victory-caption-is-not-a-prize) |
| `FUN_801DBC30` blits the party panels' name plate | falsified (its page + CLUT resolve to the `etim` red cross-out X) | [details ↓](#fun_801dbc30-is-not-the-battle-name-plate) |
| The retail party HUD carries HP / MP gauge bars | falsified (no bar primitive in either readout's packet run) | [details](../subsystems/battle.md#the-party-status-readout---and-it-has-no-gauge) |
| Screen-element kinds named by what sits at their seat (`0x32`/`0x33` = "the roster panels") | falsified (naming by seat named the wrong record) | [details ↓](#a-kind-named-by-its-seat-can-name-the-wrong-record) |
| The battle message banner is "a gold border over a blue interior" | falsified (border only - no fill primitive under it) | [details ↓](#the-battle-message-banner-has-no-interior-fill) |
| `FUN_801E2524` / `FUN_801E2650` are a full-screen flash / fade ramp | falsified (they are the **Arts announcement banner**) | [details ↓](#the-flash-ramp-is-the-arts-announcement-banner) |
| The battle per-actor draw `FUN_80048A08` runs **35-64x per frame** during a summon | falsified (once per live actor per rendered frame) | [details ↓](#the-summon-draw-runs-35-64-times-a-frame) |
| The slot-B cast band applies damage with **one** shape, seat-0 hardcoded | falsified (true of PROT 0958 / 0959 / 0960 only) | The seat-0 write is real, and reading it as the band's law is the natural generalisation from the three Delilas modules where it is the whole damage path. But the band splits: the capture-class ticks read the caster's own `+0x1DF` through the `0x801CF56C` trampoline and clamp per victim, and two images apply damage to a **row** of seats rather than one. The rule to carry is per-module, not per-band - [cast-module.md](../subsystems/cast-module.md#the-seat-0-hardcode-and-where-it-does-not-hold). |
| PROT 0927 can never kill, so it needs no death path | falsified (it is a stager; its tick is the killer) | 0927's own image is a multi-seat **stager** - it seats the enemy row from `ctx[+1]` and stages clips, and nothing in it subtracts HP, which is what the reading measured. The damage lands in the tick it stages: `0x801F6A84` clamps the subtraction with `sltu`, exactly like every other capture-class tick. Reading a stager's image as the whole spell is the recurring slot-B trap - [cast-module.md](../subsystems/cast-module.md#the-two-aoe-sweeps). |
| `ctx[+0xD]` variant `2` stamps a `0x400` camera **roll** | falsified (it is the translation `TR.y`, not a rotation) | Plausible because variant `1` is a `0x800` yaw and a per-action camera that yaws would naturally also roll. The byte is two independent bits, not an enum: bit 0 adds `0x800` of yaw and bit 1 adds `0x80` of pitch **and** drops `TR.y` by `0x100`. Nothing in the arm writes a Z angle. See [`battle-action.md`](../subsystems/battle-action.md#the-three-movers). |
| The Spirit **halving** flag is a battle-actor `+0x16E` bit | falsified (it is the character record's `+0xF8` bit `0x800`) | `+0x16E` is where every other per-battle affliction bit lives, so a "Spirit is halved" bit reads as belonging there. It does not: the halving is passive `0x2B` (*AP Used Down*), an accessory bit in the persistent per-character ability bitfield at record `+0xF8`, tested at `0x801EF364` in the queue builder and again by the status panel at `0x801D4520`. A per-battle bit could not survive the save, and this one does. |
| A dome direction swing takes its damage from `FUN_801E09F8` -> `FUN_801DD0AC` | falsified (that chain carries a move-power **index**, and the dome's rows are zero) | The chain is real and it is the monster-special damage path, so a dome swing entering it looks like the answer. What travels is an index into the move-power table, and the dome's four direction commands map `0x0C..0x0F` to row `0`, whose power bytes are zero - the chain would deal nothing. The dome resolves its own exchange; see [`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md#the-dd0ac-chain-is-not-a-direction-swings). |
| `^H` is an exception to the element-badge caret bijection (Cort has no badge letter) | falsified (the map is a zero-exception bijection) | The census that produced the exception was over shipped monster **names**, and no shipped name happens to carry `^H`; absence from the corpus read as absence from the encoding. The escape decoder has no special case: badge index = `letter - 'A'` for the whole `0x8B..=0x92` strip, and element -> caret is the fixed permutation `[4, 3, 0, 2, 1, 5, 6, 7]`. Parser `MonsterRecord::plaque_badge`. |
| `battle_gimard_tail_fire_a/_b` are frames of a **party** Seru summon | falsified (the enemy Gimard's Tail Fire) | The acting-actor plaque top-left reads `Gimard`, the pill readout is Vahn at 154/180 after `DAMAGE 16`, and the states' loader-B id is `5` (PROT 0900, the move-FX module), not a stager. A party summon draws no label (and no readout while its seats are hidden); its frames are the `*_summon_mid_cast` states. Reading these two as the player-summon reference put the enemy's chrome on the player's cast. |
| `FUN_801DBF9C` (the `0x29` party trigger) applies the spell's outcome | falsified (it stages the anim stream and the summon sub-route) | No store in it reaches HP, MP or a target; it writes `+0x1E0..+0x1E2` (`9`, `0x12`, `0xFF`) for any id `>= 0x25` and copies an overlay anim-pair list below that. The outcome is the streamed module's - the summon stager's strike for a Seru id. [details](re-settled-threads.md#the-party-cast-trigger-is-a-params-stager) |
| `FUN_801DC0A0(actor, id)` stages the cast clip | falsified (it is the cast-effect driver) | The summon band calls it with `0x12` every frame of `0x33` / `0x34` while the caster's `+0x1D9` reads `9` (`gimard_summon_start`); the clip stage is the SM's own `+0x1DA` store at `0x29` / `0x2A`. |
| "The port never latches the art id, so `actor[+0x1DB]` reads `0x00` all fight" | falsified (it latches `0x01`, then `0x0D`, then `0x0C`) | Those are the rolled arm swings a basic Attack queues, and a retail mid-swing Attack state reads the same band. `0x1A..=0x2D` - the per-art camera's band - is reached by action-constant queue bytes, i.e. an arts chain, so the camera not arming on two arm swings is retail. [details](../subsystems/battle-action.md#three-readings-the-port-already-satisfied) |
| "The port's `0x51` Done-band residency is unbounded" | falsified (bounded at `ctx[+0x6D8] = 0x3C`, as retail) | The 60-70 frames a sample shows is **one** action's countdown plus the HP-bar settle freeze; a multi-action sample sums several of them. Counting frames in a band without splitting them by action reads a per-action budget as a park. [details](../subsystems/battle-action.md#three-readings-the-port-already-satisfied) |
| "The sparring tutorial reopens the command session, so the cursor cannot move" | falsified (the cursor walks `0..5`; a *waiting* prompt box parks the tick) | The session is reopened only on a rejected resolution. What pins the cursor on screen is retail's own `ctx[+0x6B2]` box guard, which the port reproduces. [details](../subsystems/battle-action.md#three-readings-the-port-already-satisfied) |
| An action returns its combatants to their authored formation seats | falsified (retail leaves them on the ground the action ended on) | Two library states of one solo fight read the authored formation 1600 apart; two later ones read the same pair ~300 apart and both far off it, with every actor's `+0x3C`/`+0x40` pair within ~110 units of its live `+0x34`/`+0x38`. The port's walk-home leg is gone; the seat is committed from the live pair at `DoneCleanup`. [details](../subsystems/battle-action.md#where-an-action-leaves-its-combatants) |
| Staged ids `0x10` and `0x1A` **alone** install at dynamic slot `0x11`, every other art-bank id at `0x10` | falsified (`0x10`, `0x1A` and every art constant `>= 0x1B` install at `0x11`; only the base ids `0x11..=0x19` take `0x10`) | [details ↓](#the-dynamic-slot-rewrite-was-never-0x10-and-0x1a-only) |
| PROT 0910 writes no HP, because its tick body carries no damage wrapper | falsified (the wrapper is in its callee) | Plausible: the tick's own extent really does hold no `jal` into the damage family and no `+0x14C` store, and a per-image verdict taken inside one function extent looks like a verdict about the image. `FUN_801F81DC` is the applier - three `jal` sites in the tick, the wrapper call `li a0,0x12` / `li a1,7` / `jal 0x801DD0AC` at `0x801F8874`, and the HP store at `0x801F8910`. A frame is a boundary of the measurement, not of the module. |
| A module's phase stores are its `sb` writes at a literal `0x279` displacement | falsified (most store through a formed pointer) | Plausible: the phase byte is `ctx + 0x279` and a census keyed on the displacement is exact wherever the base is the incoming argument. Most modules form the context pointer once into a saved register and store through that, so the displacement never appears: PROT 0908 reads as **zero** phase stores this way and has six. Counting both forms, the eleven player-Seru bodies run 3..10 stores each. |
| The cast band uses two damage-clamp shapes, and every tick body takes the kill-capable one | falsified (three shapes; seven tick sites take the third) | Plausible: the two shapes differ only in whether the clamp precedes or follows the store, which is the kind of split a census finds by windowing each `jal`. A third caps at `HP - 1` with an *unsigned* compare - neither killing nor healing - and every one of its seven sites is a tick body. Two of them park the roll in a saved register more than a hundred instructions before the apply, so a windowed census misses them. |
| The wrapper's return is the damage a module applies | falsified for PROT 0910 (it is shifted first) | Plausible: every other body in the band stores the wrapper's return through its clamp unchanged, so the wrapper reads as the magnitude. `srl s1,s1,2` at `0x801F8898` rewrites the very register the clamp and the stores use, so the applied figure is `wrapper_return >> 2` - a live capture reads 427 from the wrapper and 106 into the victim. A port that folds the wrapper's value applies four times retail. |
| Nighto's resist roll forks the module's phase | falsified (only the kill roll does) | Plausible: the arm rolls twice and ends in two different phases, so pairing the second roll with the second phase is the economical reading. The `beqz` at `0x801F7E04` tests the **kill** roll `0x801F8534`; the confuse leg's `sb 0xF,0x279(...)` at `0x801F7E28` is unconditional. `0x801F853C` only suppresses the victim writes, so a resisted cast still advances to phase 15 - captured twice as `13 -> 15`. |
| Monster record `+0x20` is a per-monster instant-death immunity byte | falsified (it is a double-width texture-page flag) | Plausible: three summon ticks read it before an instant-death roll and force the resist when it is set, and 37 of 186 records carry it - bosses plus the Evil Fly / Death Wings / Demon Fly family, which is exactly what an immunity list looks like. Its primary reader is the model upload at `0x801F1D0C`, which widens the VRAM rect from `0x20` to `0x40` through `FUN_80055468`. The ticks borrow it as a "big model" proxy; a field's *meaning* is its primary reader's, not its most interesting reader's. |
| PROT 0941's `0x51` Steal deals damage like its band neighbours | falsified (zero damage wrappers) | Plausible: the band's arms are overwhelmingly damage bodies, and an enemy Steal that also hurts is an ordinary design. Its outcome is an inventory consume through `FUN_80042310` - against a party victim by rejection sampling over the 256-slot bag at `0x80085958`, against a monster victim by `rand() % 100` versus `0x80077828 + id*2`, the table the player's Steal rolls on. |
| PROT 0943's MP-pair writer is the routine at `0x801F69D8` | falsified (that VA is the body's head table) | Plausible: `0x801F69D8` is the slot-B load base, six other images really do put a tick body there, and a table of code pointers disassembles. In 0943 the base holds the `0xB5` body's **head table**; the writer is the body at `0x801F6A04`. Head tables are not pinned to the base either - 0943's `0x40` and 0944's `0x53` read `0x801F69F0`, 0950's `0x5A` reads `0x801F6A10`. |
| PROT 0940's `0xAC` arm blanks the caster's `+0x0C` | falsified (it blanks seat 3's action queue) | Plausible: `s0` holds the caster on entry and the stores are a short run of zeros, so reading them against the register's *entry* value is the natural pass. `s0` is reassigned to `0x801C9370` at `0x801F7648`; the blanked bytes are `actor_table[3]`'s `+0x1EF..+0x1F3`. A backward-only scan for the base is what misses a reused register. |
| The arena's pre-test seed of `0x8007BAC0` is zero | falsified (it is `1`) | Plausible: the word is zero before the arena runs and the seeding arms only fire on a set story flag, so the unflagged path reads as "leave it alone". `sw $s2` at `0x801CEB8C` stores `1`, with `$s2` loaded 43 instructions and three `jal`s earlier - far enough back that a local read does not see it. Course 0 with no bans, which is a different thing from no seed; "every seed carries `0x100`" is true of the three flagged seeds only. |
| The dome tally screen draws its four lanes, then the totals | falsified (the HP accumulator sits between lanes 2 and 3) | Plausible: a scoreboard listing its lanes in order and totalling underneath is the shape every reader expects, and the port drew it that way on both hosts. Retail's six rows are `[lane0, lane1, lane2, the HP accumulator 0x801D1AC8, lane3, the running tally 0x80084440]`, at brightnesses `[0, 1, 2, 0, 3, 3]` - four steps, not six. `FUN_801D1184` re-forms the `0x801D` base into a different register between the product and the store, which is what mis-attributed the lanes. |
| The player-Seru wrapper sites pass the caster seat in `a1` | falsified for the player half (`a1` is a baked `7`) | Plausible: the capture half really does load it - `lbu a1,0x13(...)` - and one rule covering both halves is the tidy reading. The player-half sites bake the literal instead: `addiu a1,zero,7` at `0x801F74A8` and `0x801F8880`. The summon always occupies seat 7 on that half, so the constant and the field agree in retail and disagree the moment a port seats a summon anywhere else. |
| PROT 0941's Steal floors its bag draw while the battle context's `+0x11` reads 4 | falsified (the gate is `DAT_8007BD10[1]`) | Plausible: `$s5` is the battle-context pointer in most of the band, so `lbu v1, 1($s5)` reads as a context field - and a context byte gating a bag walk is an ordinary shape. In this module `$s5` is formed at `0x801F77B0` as `0x8007BD10`, the present-party list, so the gate at `0x801F77E8` is `DAT_8007BD10[1] == 4` - **battle seat 1 holding roster character 4**, the split-bag condition. PROT 0941 makes no access at `+0x11` at all; the context pointer there is `*(0x8007BD24)`. Read the register's own formation, not the register's usual meaning. |
| `0x8007B83C` is `FUN_8001E890`'s `== 2` gate | falsified (that word is the game mode) | Plausible: the routine really does compare a word against 2, a game mode of 2 really is the scene-load mode, and a probe that watches the mode word across a load sees it take the value the branch wants at exactly the right moment. The compared word is `gp+0x6AC` = `0x8007B9C4`, the pack's own load-state. `0x8007B83C` is `gp+0x524`. Two words that agree on one route are not the same word, and the whole three-state reading of the arm depends on which one it is. |
| PROT 0943's `0x40` and PROT 0944's `0x53` fault before their first tick | falsified (the body ticks; the fault is downstream) | Plausible: the emulator paused on an unmapped read with no module frame on the stack, in every post-turn state the corpus held, which reads as "the arm never started". The debugger's pause is what hid the PC. Both arms stage clip `0x0B` and SCUS's anim commit indexes the **caster's** spell-entry array with it, so a caster with ten entries reads its own name text as a pointer. Logging the access instead of pausing on it shows both bodies walking all five arms. |
| Battle context `+0x276` is a per-module gate | falsified (it is the side-band applier's stage) | Plausible: the summon modules do test it before their head cue, and a byte a module tests before doing its work is a gate by every normal reading. Its writers are the applier SM and two battle routines; the modules poll it, so it is open by construction when a cast wants it. Feeding a port's copy from a tutorial flag - which nothing on the disc writes it from - silenced the melee sting in exactly the battles the flag was set in. |
| The SPU's slow cast-voice release is an envelope defect | falsified (it is the reverb tail) | Plausible: a one-second decay after key-off is what a wrong ADSR shift sounds like, and the envelope was the unpinned half of the chain. The envelope is tick-exact - shift `0` linear is `-0x4000` per tick, pinned against an independent model for shifts 0, 15 and exponential - and the dry SPU is silent four samples after key-off. The tail was the room: every voice was routed through a retail reverb preset the resampler installs. |
| `0x1F80037D` is a second per-frame byte | falsified (it is the game-speed **rate** scalar) | Plausible: the mode-INIT core reset reloads it in the same breath as `0x1F800393`, and on a machine holding 60 fps both bytes sit at small constants, so a probe cannot tell them apart. `0x80055FBC` writes it the literal 8 and it is not touched again per frame; `0x1F800393` is written every frame by the pacer, which picks 1 to 4 off elapsed time. Elapsed time in this game is their **product**, so a countdown read against either byte alone reads as a constant multiple of the other. |
| `FUN_801DD4B0` is a damage path of its own | falsified (it is `FUN_801DD0AC`'s non-summon arm, term for term) | Plausible: it is a separate entry with its own frame, and two entries that compute damage are two damage paths under any normal reading. Its arithmetic matches the shared kernel's non-summon arm instruction for instruction, which is why driving both against one seat returns 251 against 251 and no capture can separate them. `FUN_801DD6B4` is the one that differs - a physical-stat kernel that bypasses defence. |
| "Move `0x36`" and "move `0x37`" are move-VM sub-opcodes | falsified (they are `actor[+0x1DF]` action ids) | Plausible: the move VM has a dense opcode space and both numbers fall inside it, so a pair of ids cited without their space reads as opcodes. They are entries in the battle action queue, which is a different id space with a different dispatcher - so a reader who went looking for them in the move VM's table found two unrelated arms and nothing to contradict the reading. |
| `_DAT_8007B64A` has no writer | falsified (the field entity tick writes it) | Plausible: an absolute-address sweep over `SCUS_942.54` and every overlay image really does find nothing, and that sweep is the instrument these questions are usually settled with. Every access to the byte is `gp`-relative, a form the word scan cannot see; the `gp`-relative sibling finds 14. `FUN_801DA51C` clears it at `0x801DA69C` and raises `1` at `0x801DA6A8` off system flag `0x19`, and battle latches `3` at `0x801E6D2C` ([settled](re-settled-threads.md#battle--arts--level-up)). |
| The arm at `0x801E3DD8` sets `ctx[7] = 0x3E`, and a spell can reach it | falsified (the arm **is** state `0x3D`, and only an item opens it) | Plausible: the arm's own exit store is the next state, so reading the store as the arm's identity is one off in exactly the direction that hides the entry. The jump table settles it: base `0x801CED44`, the word holding the arm at `0x801CEE38`, index `0x3D`. And the band is not spell-reachable - the Magic category arm sets the predecessor state only for spell ids below `0x65`, which the player Seru block `0x81..0x8B` fails outright, while the Item arm sets it unconditionally. Fifteen injected casts reaching the arm zero times was the bound, not a sampling accident. |
| The battle selectable scans test an action-state byte where `4` means removed or done | falsified (it is a seat index) | Plausible: the scans sit beside the action state machine, they run per seat, and a small constant compared against a per-seat byte reads as a state enum - which is how a `4 = removed / done` state nothing ever writes got documented. `DAT_8007BD10` holds the per-slot roster character id: `FUN_801DA34C` indexes it and subtracts one to reach a character record. `4` is the AI-companion seat, so the term excludes a seat the player does not command ([settled](re-settled-threads.md#battle--arts--level-up)). |
| `FUN_801D0748` is the Muscle Dome's match state machine | falsified (it is the round SM **every** battle runs) | Plausible, and it held for as long as nobody asked the bytes: every capture that had caught it was a dome one, five differently-prefixed dumps of it all carry the dome label, and its arms *are* the arms a dome round needs. They are the arms every round needs. The routine has exactly one `jal` disc-wide - `0x80047014` in the SCUS battle frame driver `FUN_80046A20`, with no test in front of it - so every battle frame steps it, and three non-dome battle states enter it hundreds of times over 700 vsyncs each. The generalisation: a routine named from the contexts its captures came from is named after its callers ([settled](re-settled-threads.md#battle--arts--level-up)). |
| No draw site exists for the dome panel still at VRAM `(384, 0)` | falsified (the emitter never materialises `384`) | Plausible, and the census behind it was correct: 33 sites disc-wide form `0x180`, none paired with `y = 0`. The inference from it was not. A textured primitive addresses VRAM through the packed `tpage` halfword, where the x coordinate is a **page index** - `384 / 64 = 6` - so the emitter's constants are `0x106` and `0x109`, and no search for the pixel column could have hit it. It is `FUN_801D00F8` in the contest hub PROT 0977, an image that is not even resident when the upload runs ([settled](re-settled-threads.md#battle--arts--level-up)). |
| `0x801F90DC` is retail-unreachable, so its port owes no host | falsified (five slot-B images reference it) | Plausible: the address sat in a group of wiring rows that really were retail-unreachable, and a group verdict is cheaper to write than five. Two `lui` materialisations and three branches reach it across PROT `0913` / `0927` / `0928` / `0934` / `0951`. What actually blocks the port is image **ownership** - which module the code belongs to - not reachability, and those are different blockers with different work behind them. |
| The Arts banner selector `ctx[+0x28B]` is raised in the battle overlay, and an overlay sweep finding no writer means there is none | falsified (all four writers are in SCUS `FUN_8004AD80`) | Plausible: the banner's reader `FUN_801E2524` and everything around it are overlay code, so the overlay is where a raiser was looked for. Every `sb ...,0x28b` on the disc is at `0x8004ADDC` / `0x8004B774` / `0x8004B80C` / `0x8004B87C`. "Unfound in X" is a statement about where the search ran; it aged into "no raiser" because the scope was not written next to it. |
| The three `ctx[+0x28B]` raises are alternatives - one per starter kind | falsified (they run in sequence; the side-array pick overwrites the seat flag's write) | Plausible: three stores of three constants in three arms of one routine read as a selection. `0x8004B774` stores `3` and **falls through** to `0x8004B7D8`, which reads the queue-builder side array `0x801F6990` at `0x8004B804` and stores over it at `0x8004B80C`. `battle-action.md` had read the branches as exclusive. |
| `FUN_801D84C0` builds the four battle party-name panel labels, measuring each with `FUN_8003CBF8` | falsified (it builds the four **battle-result messages**; `FUN_8003CBF8(buf, 0xC1, 1)` locates the name escape) | Plausible: the four buffers are text, one per display slot, and the routine pairs with a panel opener. Resolving the pool strings its two arms copy and append (`0x801F4C38..0x801F4CC4`) gives a victory line with spoils, a defeat line and the two escape outcomes; the `0xC1` call returns an offset the roster arm then patches with a participant id, not a width. Every patch reads the **first** seat, so the per-seat caption ids were wrong too, and the port's victory line named a team of one. |
| `FUN_801D32BC` is a turn-order choice - retail's cursor order over initiative | falsified (it is the command window's **member cursor**) | Plausible: it steps a seat index over living actors, which is what a turn order does. Its six call sites are the round reset `0x801D8910` and `FUN_801D388C`'s cases `0x10`, `0x11`, `0x21` and a tail pair - command input. Initiative is the execution order, and the port's command order is already retail's slot scan; what the port lacks is the backward step. |
| The six `baka_fighter_chrome` NOT WIRED anchors are the duel's digit strips | falsified (they are the keyframe-editor band and sprite passes) | Plausible: the digit strips were the visible gap and the cluster sits in the same image. The anchors are `anim_slot_install` / `delete` over `DAT_801DBF44`, `impact_effect_pair` (`FUN_801D4DF8`), `sprite_blit` (`FUN_801D65F8`, a `MoveImage`), `mirrored_sprite_pass` (`FUN_801D49E8`) and `editor_tick` (`FUN_801D4FC8`); what blocks them is missing engine state - an eight-slot key array, per-action keyframe TRS, a sprite-actor pool. |
| A clobber of the resident PROT 0874 pack trips `FUN_8001E890`'s checksum | falsified (the sum is over a VRAM read-back, not RAM) | Plausible: the routine re-sums the container and reloads on a mismatch, so corrupting the container looks like the way to fire it. Four XOR'd words of the resident copy left the sum byte-identical (`0x7BF74962`): the words summed come back from VRAM through `StoreImage` (`0x8005842C`). Only VRAM at `(0x180 + 0x40i, 0)` or the boot sum at `gp+0x6B8` can break it. |

### The scripted boost profile is "the international release's" profile for every fight

The battle loader `FUN_80054CB0` boosts an enemy's ATK / UDF / LDF / INT as it
installs the record, choosing between two profiles on `ctx[+0x287]`. A live
Gaza capture reproduced the flag-set profile (`ATK x5/4, UDF/LDF x2, INT
x9/8`) byte-for-byte, and the curated bestiary matched it for 120+ enemies,
so the reading became "the gate-set profile is what the US/PAL build uses"
and `battle_stats()` returned it as *the* in-battle block. Both premises hold
and the conclusion is wrong: `ctx[+0x287]` is the **scripted-fight flag**
(a formation row's non-zero header byte), Gaza is a scripted fight, and the
bestiary was authored from boss-profile numbers. Every random-encounter save
state carries `+0x287 == 0` and the *other* profile (`ATK x1, UDF/LDF x7/4,
INT x5/4`) - a world-map Gobu Gobu with record `17/15/14/10` fights as
`17/25/24/12`. The site's enemy table and `enemies.toml` both showed random
natives with boss-fight defence. `MonsterRecord::battle_stats_random` is the
flag-clear profile; see [battle.md](../subsystems/battle.md#monster-record-source-layout).

### `FUN_801F3D3C` installs a queued-magic follow-up routine

The pair `FUN_801F3C34` / `FUN_801F3D3C` was read as a "queued-magic
follow-up" latch: the installer picks a record out of `0x801F6870` by
`[actor class][level band]`, stores its byte `0` as a follow-up id and its
word `1` as a **routine pointer** at `0x800775B4`, and the reader stays
silent while a follow-up is pending. The table index is not a class - it is
the summon record's **element** byte (`(*0x801C9358)[+0x1D]`), the same
byte the affinity scale reads; the word is a **banner string** pointer (the
table's strings name the stat and the percent, and the reader's own install
value `0x801CFA20` is the text "No effect."); and byte `0` is the **percent**
the damage finisher's per-element switch shaves off the target on every hit.
The "seven-entry jump table the dump does not cover" is the per-element
base-vs-record compare inside the same function. Full mechanism:
[battle-formulas.md](../subsystems/battle-formulas.md#seru-magic-side-effects---the-element-debuffs-fun_801f3d3c--the-finisher-switch).

### The dynamic-slot rewrite was never "`0x10` and `0x1A` only"

The decompiled C of `FUN_8004AD80` shows two `0x11` assignments and the
reading took them for the whole set, so `resolve_staged_anim` installed every
art constant at slot `0x10`. The slot register `s2` is written in **delay
slots**, which the C folds away: `_li s2,0x10` under the `0x1A` test at
`0x8004B720` is the default, `0x8004B76C` (the `0x1A` arm) and `0x8004BB58`
(the `0x10` test) set `0x11`, and the art-constant arm - entered for every
staged id `>= 0x1B` at `0x8004BB5C` - sets `0x11` in the delay slot of its
name-width call (`jal 0x80035f04 ; _li s2,0x11` at `0x8004BBBC..0x8004BBC0`)
before the install at `0x8004BC4C`. A live Tri-Somersault capture agrees:
`+0x1D9` reads `0x11` under `0x27`, `0x1F` and `0x2B`, and `0x10` under the
`0x19` starter. The narrow reading survived for as long as no art constant was
ever staged through the port's attack band; the first one that was landed on
the wrong slot. The `+0x1D9 == +0x1DA` equality checks compare slot numbers,
so the slot an id lands on is what decides whether the SM sees its clip as
committed.

### A level-up is not a heal

The reading came from a *multi-level* capture triplet whose third frame writes
the live current-pool cells right after a level-up, and it was written into
`capture_observations::char_level_up` as "the level-up refill". The port
encoded it in `LevelUpTracker::apply_to_record`, which made every level-up a
free full heal.

Two independent checks falsify it.

- **Disassembly.** `FUN_801E9504` stores to exactly eleven addresses: the
  record window's `hp_max` / `mp_max`, its six battle stats, the displayed-level
  byte and its actor-table mirror, and two globals. No live-window cell is
  among them, and the routine's only `jal` is the BIOS `rand` - so nothing it
  calls writes one either. Identical in both dumps of the routine.
- **Capture.** The **single-level** `noa_levelup_*` triplet - the corpus's own
  arithmetic oracle - reads Noa at `164/182` HP and `16/16` MP going into the
  fight and `164/221` / `16/21` once the L2 → L3 level-up has settled in the
  field. Both maxima move by the growth amount; neither current moves at all.

So the `+0x106` / `+0x10A` write the multi-level captures see is the
battle-end resync of the live pools, not a grant.

**Generalises to:** a write that lands *near* an event is not a write *by* it.
A frame-window capture cannot separate the two on its own - only the store set
of the routine can.

### The flash ramp is the Arts announcement banner

`FUN_801E2650` scales a percent into grey, replicates it into RGB, picks GP0
`0x2C` or `0x2E`, and emits quads whose extent is driven by a level byte. Read
on the arithmetic alone that is a flash, and it was documented as one - a
"full-screen flash / fade overlay" walked by a "brightness level".

The quads are **textured**, and the texels settle it. Every arm writes texpage
`0x27` = `(448, 0)` under CBA `0x7703`; decoding that page at 4bpp through that
sub-palette shows the emitter's three 24-tall rows are the words `SUPER`,
`HYPER`, and `MIRACLE` + `NEW`, sitting directly above the already-documented
`DAMAGE` / `HIT` / `TOTAL` labels on the same sheet. The second quad's texel
rect is fixed for every position and reads `ARTS!!`. So the four `ctx[+0x28B]`
values compose `NEW ARTS!!` / `HYPER ARTS!!` / `MIRACLE ARTS!!` /
`SUPER ARTS!!`, each as two halves sliding in from opposite screen sides to a
per-banner seam. `ctx[+0x28C]` is that slide's clock, and the four "layers" are
a ghost trail behind the moving word - not a brightness ramp.

The lesson generalises: a routine that emits textured primitives is not
characterised until its texels are decoded. Percent-scaled grey with
`0x2C`/`0x2E` describes a flash and a banner equally well, and only the atlas
distinguishes them. Full geometry:
[`battle-action.md`](../subsystems/battle-action.md#arts-announcement-banner-fun_801e2524--fun_801e2650);
sheet layout: [`effect.md`](../formats/effect.md#the-battle-value-readouts-glyph-sheet-lives-here-too).

### The battle message banner has no interior fill

Two live frames carrying the banner - `rim_elm_gimard_seru_capture_after` (the
mid-battle Seru "captured!" line) and `noa_levelup_banner` - draw the class-0
9-slice border sprites and the glyph run and **nothing else**. No textured
fill, no flat quad, no semi-transparent rect anywhere inside the frame rect.
The scene shows through.

What made "a gold border over a blue interior" the natural reading is that the
framed-window widget records (`0x03` / `0x04` / `0x44`) carry a 32x32
blue-marbled patch at texels `(128, 0)` as their own sprite rect, and the
framed *menu* windows do fill with it - so the art exists, and the battle
banner simply does not use it. Geometry:
[`battle.md`](../subsystems/battle.md#the-full-width-message-banner).

### A kind named by its seat can name the wrong record

Before the `+0x0E` kind byte was resolved as a table index, the open thread
listed the values it could not decode with the surface each one *sat under*:
`0x0303` "full-width message rows", `0x0404` "framed windows", `0x2B2B` "the
status bar", `0x32`/`0x33` "the roster panels". Four of those survive the
decode. The panel one does not: the three roster-panel placement records
(6, 78, 79) carry kind `0x07`, and `0x33`/`0x34`/`0x35` are the sibling kinds
that add the level / status marker on top of the same panel chain.

The reading was not careless - it was correct about *what is on screen* and
wrong about *which row draws it*, because the two kinds converge: `0x33`'s
chain hops `+0x0E` and then walks into `0x08` → `0x09`, the same panel plate
`0x07`'s chain ends on. A seat-based name cannot separate two records that
draw the same pixels, and no amount of further capture would have; only the
index arithmetic (`0x800732A4 + kind * 0x0C`, `FUN_8002C69C` at
`0x8002C7A0`) does. Resolution:
[`re-settled-threads.md`](re-settled-threads.md#the-chrome-kind-byte-is-an-index-into-the-widget-class-table).

### `FUN_801DBB8C` is not the party readout's registration

The battle overlay's `FUN_801DBB8C` registers one retained SCUS text actor
through `FUN_8003541C` and stashes the handle at `_DAT_801F4E0C`, and it sits
among the party-panel build and teardown leaves. Reading it as the readout's
own registration - the actor the arts input then parks at `y = 230` - was
the natural next step, and a brief carried it as a fact.

Its one caller says otherwise. `FUN_801D0748` calls it at `0x801D1660`, on the
ring's `0x28 -> 0x50` arm, immediately after `FUN_801D388C(9)` has built the
arts-entry screen - and the arguments it passes are `(0, 0xC, 0, -146, 36,
138, 144, 3)`: a 138x144 box parked one screen to the left, which is the arts
list window the Triangle page slides in. The party readouts are placement
records 7 and 6 / 78 / 79, opened by the sub-draw script through
`FUN_801D8DE8` like every other chrome element.

**Lesson:** a registration is identified by the rect it registers and the
transition that calls it, not by the leaves it is compiled beside.

### The item window shows the pill

Retail's item-use *action* shows the full-width pill (`captures/tetsu_idle`),
and the ring the window opens from shows it too, so "pill while the item
window is up" read as the obvious interpolation. The sub-draw step the
`0x28 -> 0x3C` arm runs (`FUN_801D388C(5)`, `0x801D13F0`) says the opposite:
`06/0 4E/0 4F/0 07/1` - the roster panels come **back up** and the bar parks.
The window's target step (`0x64`, step `0x18`) is where the bar returns,
re-pointed at the member the cursor names. The magic window (step 7) has the
same shape.

**Lesson:** a menu state's surfaces are a table row, not a neighbour's; read
the step, not the frames either side of it.

### The magic chip's gate reads the weapon byte

`FUN_80053CB8` writes `ctx[+0x25F + member]` after an `lbu` at `+0x760` off
`0x80084140 + (char_id - 1) * 0x414`, and `0x80084140 + 0x760` is the live
record's `+0x198` - the equipment byte the save-record table names
`weapon_id`. So the first reading was "the element chip is live when a weapon
is equipped". Twenty-nine states say no: `player_steal_skeleton_pre` has the
gate at `1` with `+0x198 = 0` and `+0x199 = 1`. The store at `0x80054270` is
the **second** arm; the first (`0x800541E0..0x80054218`) reads `+0x761` -
`+0x199`, the Ra-Seru slot - and every state's gate equals that byte's
non-zero test. The `+0x760` arm is reached only for `char_id == 2` - the
`beq v0,a3` at `0x800541E4` on `DAT_8007BD10[member]` - so it is Noa's byte,
not the weapon rule.

**Lesson:** one `lbu` in a two-arm predicate is not the predicate; check the
reading against a state whose bytes disagree with it.

### `FUN_801DBC30` is not the battle name plate

The blit at `FUN_801DBC30` sits in the battle overlay next to the party-name
panel's open and teardown leaves, takes an `(x, y)`, and lays down one
`0x40 x 0x10` textured quad. Reading it as those panels' name plate is almost
irresistible: a fixed-size strip, drawn at a caller-supplied seat, in the one
function group that builds the name buffers. The port acted on it - the HUD
drew a filled rect at the quad's geometry, and the panels' 8-pixel text inset
was explained by the quad's `x-8` bias.

The quad's own words falsify it. Two constants say where the pixels come
from: `tpage 7` resolves to VRAM page `(448, 0)` and CLUT `0x7704` to
`(64, 476)`. That is not the system-UI sheet the battle chrome samples - it
is the `etim` effect page, and the texel span `(0, 96)`-`(63, 111)` decodes
out of a battle VRAM dump as the **red cross-out X**, the mark retail lays
over a command chip the actor cannot pick. The same rect is already pinned,
under that name, for the Muscle Dome's forbidden Item chip.

Walking the real display list settles what the chrome is instead: the name
plates are 3-slice runs off the resident system-UI sheet's page `(896, 256)`,
and the party readout draws no bar at all.

**Lesson:** a primitive builder is identified by the page and palette it
samples, not by the neighbourhood it is compiled into. Both constants were
sitting in the decode the whole time; nobody resolved them to a VRAM
coordinate, so the function kept the name its neighbours gave it.

See [battle.md](../subsystems/battle.md#battle-screen-chrome-packet-pinned).

### The dome victory caption is not a prize

`FUN_801D8DE8` case `0x59` composes a victory line out of a per-character
label from the table at `0x801F4DFC` plus a spell name from the shared
spell-name table at `ctx[+0x269] + 0x80` - the player Seru-magic block. Read
on its own that is a very convincing award message, and the port acted on it:
a won dome leg credited a Seru capture against the registry.

Two things falsify it.

The table is **shared**. `0x801F4DFC` is the battle-family per-character label
table, byte-identical across the battle-action, magic-capture, magic-level-up
and dome overlays, and the composer that reads it is the ordinary cast-caption
builder reached by *any* cast in *any* battle. Its presence in the dome
overlay is residency, not a dome feature - the same trap the `0x801F4D34` /
`0x801F4B8C` sibling tables sit next to.

And the arena grants nothing of the kind. The whole reward path in PROT 0977
is `FUN_801D0F60`: it settles the score tally and, once per save on the
Master-course final fight, hands over item `0xCD`. The tally is then paid by
the *shared* minigame-exit routine `FUN_80026018` into the casino coin bank
`0x800845A4`, saturating at 9,999,999. Item and coins are the only two things
that move. There is no `record_capture` analogue anywhere in the overlay.

**Lesson:** a caption that names a reward is not a reward. Before crediting
anything a message mentions, find the *writer* of the thing being credited -
and check whether the table the message reads is resident in ten overlays.

See [minigame-muscle-dome.md](../subsystems/minigame-muscle-dome.md#contest-settlement--the-one-shot-prize).

### Muscle Dome was never a card battle

Three claims fell together, and each is instructive about a different reading habit.

**"A hand of four cards."** `FUN_801d388c` case `9` builds four slots in a `do { } while (< 4)` loop, which reads like a deal. The four slots are the four **d-pad directions**, always the same command ids `0xC..=0xF`, each carrying that fighter's own AP cost. Nothing is drawn, discarded or reshuffled; the arena is an ordinary battle whose command string is bounded by AP instead of by a fixed length. The retail presentation was already captured as the standard battle command cluster - the "card" reading survived the capture because the code's own loop shape kept suggesting it.

**"A score of `hp * 0x6C / max`."** The compiler renders `× 100` as a shift-add chain: `sll 1` (2x), `addu` (3x), `sll 3` (24x), `addu` (25x), `sll 2` (**100x**), at `0x801d0f38..0x801d0f4c`. Stopping at the fourth instruction yields 25, and folding the wrong pair yields `0x6C` (108). The lesson generalises past this arm: **a multiplier read off a shift-add chain is only correct if you consume the whole chain**, and the check is free - Ghidra's own C prints `* 100`, and a second dump of the same code at a different load base (`overlay_0896_801f04b0.txt`) reproduces it.

**"Rendered in phase `0x6e`, per fighter."** The computation lives in the phase-`0x14` arm; `0x6e` only re-stamps the two globals `0x14` already wrote. And the record it reads is `DAT_801c937c` - actor-table index 3, the first **enemy** slot - so there is one number on screen, the opponent's, not one per fighter. The whole match SM contains exactly two ratio computations and both are that one `× 100`.

What the arm actually draws is the `Turns Left / HP Left` strip, whose format string is on the disc at PROT 0898 file offset `0x0`: `4 - ctx[+0x28a]` (the shared battle turn counter, bumped by `FUN_801e295c` case `0xff`) and the first enemy's HP percentage.

**"…and four turns is the whole dome leg."** That last step is itself wrong, and it is the subtler trap.
The arm is gated on `*(u8*)0x8007BD0C == 0xB6`, and `0x8007BD0C` is the four-slot **monster-id formation cell**, not a battle-type byte.
The gate therefore names a *monster*, and the dome stages its own opponents into that same cell out of a 29-round table topping out at id `0xAA`, so no dome round can ever reach it.
The strip belongs to monster `0xB6` - Koru, whose four-turn timed kill the curated boss table records independently.
The general lesson: **a byte compared against a small constant is not a mode tag until you have found its writer**; this one had exactly one writer in the arena overlay and it writes a monster id.
See [minigame-muscle-dome.md](../subsystems/minigame-muscle-dome.md#the-four-turn-strip-belongs-to-koru-not-the-dome).

A separate widget must not be folded into this one: `FUN_801d8de8` is the **shared battle status plate** (dumped under ten overlays), drawing each fighter's own HP/MP `cur`/`max` numerals from `+0x172`/`+0x14e` and `+0x174`/`+0x152`. It computes no percentage and is not dome-specific.

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

### Gaza 2 0x51 park - the two falsified generators

*Status:* both first-pass "ordinary play" generators of the `0x51` HP-readout
desync are falsified; what remains open is in
[re-settled-threads.md](re-settled-threads.md#endless-camera-orbit---the-0x19-attack-approach-park).

**Clamp asymmetry as a standalone generator.** The two overkill clamps in
`FUN_801EC3E4` (accumulator vs displayed bar at `0x801EDB70`, live HP vs
itself at `0x801EEA10`) can only disagree when the bar already lags live HP
**at action start** - and the previous party-targeted action's own `0x51`
settle wait guarantees it does not. From a synced start the arithmetic is
forced: credits exceeding the starting bar also exceed starting HP, so both
sides floor together (a kill, consistent). The "live HP 266 / bar 0 / zero
accumulator in plain capture" exhibit that anchored the generator reading is
per-strike **phased crediting** - paired stores `0x801EDB40`/`0x801EDB58`
credit the action total and the accumulator per strike while live HP commits
once at `0x801EEA10` - a transient that closed with a death commit ~90 vsyncs
later. Lesson: a per-frame watchpoint cannot distinguish an absorbing desync
from the inside of a healthy multi-strike resolution; only survival past the
action's commit and settle wait counts.

**The Final Heal revive "at the worst possible moment".** The assigning seed
(`0x800410BC`) is real and the discard arithmetic stands, but the timing
premise - the killing hit credits the whole bar so the readout is mid-drop at
state `0x50` - does not survive measurement. Credits land per strike *early*
in the resolution; `0x50` arrives after remaining targets resolve and effects
tear down; the quarter-step drain empties any accumulator within ~35 rendered
frames. A three-capture campaign (`autorun_gaza2_acc_discard.lua`,
Lost-Grail-armed party, no harness HP/readout/accumulator writes, ~84k
vsyncs) drove twelve retail `FUN_801E6968` revives across cast-path,
kernel-path, single-target and party-wide kills: every assign hit
`+0x10 == 0`, margins 143-280 vsyncs.

### The cast-module blocker was named wrong

The reading was that a streamed signature-attack module "stages the caster's
monster-block entries by raw index, and a party actor has no monster block".
The second half is false. `FUN_8004AD80` resolves `actor+0x1DA` down two
arms - monster seats through `DAT_801C9348[slot-3]+0x4C`, **party seats
through `DAT_801C9360[slot]`** - and the indices PROT 960 stages resolve on
both. Its one monster-block access is a hardcoded **seat-0** write to a
single clip's root-motion field, which has nothing to do with who is casting.

Three real blockers were behind it, none of them the stated one, and the
first is worse than a hang:

- That seat-0 read walks past `magic_count` into words the loader never
  fixed up, so a seat-0 monster with `magic_count <= 13` - Che Delilas has
  12 - turns the pointer into a bare offset and the following store lands a
  halfword in PSX **kernel RAM**. Retail never trips it because the module
  is only ever reached with Lu (16 entries) in seat 0.
- Battle state `0x70` re-enters the module every frame and advances only on
  a zero return; there is no timer and no bail-out. One of its four phase
  gates needs a clip of at least 23 keyframes, which Gala's party index
  `0x0D` fails at 17 of 19 equippable section-2 ids.
- The damage call and both HP writes are hardcoded to actor slot 0, not the
  chosen target.

**Generalises:** a structural-sounding impossibility ("X has no Y") reads as
settled and stops the search, so the *actual* failures never get enumerated -
here three of them, one a silent memory corruption that would have shipped as
"the game crashes later, sometimes". Full account in
[`randomizer.md`](../tooling/randomizer.md#casting-a-sibling-signature-attack-from-a-party-slot).

### The attack camera was never an arm choice

The reading: `FUN_801D71B8` dispatches the per-art attack camera through three
per-character jump tables, the arms visibly differ in how much choreography
they carry - one commits a single framing for the whole swing while others
change shot two or three times - and 37 of the tables' 54 slots point at a bare
return. So an art that films flat is dispatching to a poor arm, and the fix is
to point its slot at a richer one.

Two facts, both read off the tables and the arm bodies, falsify it.

- **Every arm is timed, not merely shaped.** A shot change is an `slti`
  immediate against the animation cursor `actor[+0x22C][+0x68]`, which counts
  sixteenths of a keyframe. The whole arm span carries thirteen of those tests,
  and their immediates are `64` / `97` / `112` / `144` / `160` / `176` / `192`
  / `224` / `240` / `272` - keyframe 4 through **17**, sized for the ~20-frame
  swings retail authors. Aim any arm at a 46-100 frame clip and it finishes its
  entire choreography inside the wind-up, then holds one shot for the rest of
  the move. That length assumption is common to all of them, so no arm escapes
  it and choosing between them cannot.
- **Not one arm is spare.** The 37 dead slots all point at a single shared
  epilogue (`0x801D828C`), which is a return, not an arm. The 17 live slots
  reach 13 distinct arms, no arm is live in more than one character's table,
  and every one of the 13 is already some art's camera. A retarget can
  therefore only alias an arm another art still uses - and a re-time then
  follows the alias into that art and mistunes it too.

What the arms do admit is re-timing in place: scale an arm's immediates and its
shots land at the same beats of the longer swing. Where a host art's own arm
carries no cursor test at all there is nothing to scale, and the move is a
**swap** of two slots inside one character's table rather than a retarget - a
swap leaves the set of live arms unchanged and leaves the borrowed arm
reachable from exactly one slot, which is what makes re-timing it safe. Full
account in [`randomizer.md`](../tooling/randomizer.md#delilas-party-swap).

**Generalises:** "pick a better one" presumes the candidates differ along the
axis the defect lies on. These differ in shot *count* and agree on clip
*length*, so the choice axis was orthogonal to the problem. And a table that is
two-thirds dead slots reads as spare capacity while the things those slots
would point at are fully subscribed - emptiness in the index is not slack in
what it indexes.

### The case-6 party arm is the battle-over framing

The reading: `FUN_801D5854` case 6 forks at `0x801D5CF4` on `DAT_8007BD71 ==
0xFE` and at `0x801D5CFC` on `ctx[+0x13] < 3`, so "in-battle flow state and a
party seat" takes the `0x801D5CFC` arm - eye `prescale(0x500)` straight behind
the actor at `-5 × actor[+0x3E]`, a per-character script over
`actor[+0x1DB]` - and everything else the `0x801D64C4` arm. Read as the
per-action party framing, with `0xFE` glossed as the battle-flow SM's
"in-battle" value, and the port shipped it that way with the flag hard-wired
`true`.

Why it was plausible: the fork *is* keyed on a party seat, the arm *does*
read the acting actor's anim id through a per-character dispatch, and `0xFE`
next to a `0xFF` reads naturally as one live state beside another.

Why it is wrong: `DAT_8007BD71` is the byte the rest of the repo already
names the **battle-end signal**. Its writers are the action SM's `0x5A` wipe
scans (`0x801E65D8` party wipe, `0x801E6674` monster wipe, beside the cause
in `_DAT_8007BD2C`), the `0x66` escape teardown (`0x801E5A94`, right after
`ctx[7] = 0x67`), and the capture-effect module (`0x801F7318`); SCUS
`0x80056014` zeroes it at battle init, and the effect-VM walker
(`FUN_801E0088`) runs only while it reads `0xFF`. Twelve battle save states -
five Begin/Run prompts, the arts-input close-up, the tutorial open, two
mid-strike frames and three `ctx[7] == 0x19` approach parks - all read `0xFF`.
So during a fight the `0x801D5CFC` arm is unreachable for anyone, and the anim
band its script keys on (`0x11..=0x18`) is the win-pose band: it is the
end-of-battle framing. The three `0x19` parks with Gaza acting confirm the
other arm byte-exact (`TR (0, 0x500, prescale(ctx[+0x6D0]))`, yaw
`ctx[+0x6DA] − actor[+0x46]`, focus the negated `+0x34/+0x38` pair).

What the wrong arm did to the port: a party member's strike put the eye
2048 projection units behind the actor - inside whichever combatant stood
there, since the target had closed to melee range - with `ndc.y` past `-2`
for the other actor. The fix is one boolean with the right name
(`ActionFraming::battle_over`, `false` while a fight runs), not a camera-model
change. Corrected reading in
[battle.md](../subsystems/battle.md#battle-camera-exact).

### The Done band is not idle

The reading: `FUN_801E295C`'s Done band (`0x50` cleanup, `0x51` fade-down)
is where a port fight rests, and a measured auto-resolved fight spent about
half its frames there; classifying it as "an action is executing" left both
hosts in the per-action close-up for the whole fight, so the port treated
the band as idle and put the far framing (case 9) on it.

Why it was plausible: the band *is* long relative to the port's short action
band, the far framing *is* where the formation and the idle orbit live, and
the per-action framing at the time really was a close-up.

Why it is wrong: that close-up was the battle-over arm applied to a running
fight (see [above](#the-case-6-party-arm-is-the-battle-over-framing)). With
the in-fight arm, case 6 sits at `prescale(ctx[+0x6D0])` - `4915` for a
`0xC00` depth - with both combatants in frame, while the far framing over a
formation collapsed by a melee sits at its `0x800` floor, `3276`, closer than
the arm it was standing in for; every torso close-up in the port's Done band
was the far framing. Retail's own `0x50` / `0x51` arms fork on the category
(`0x801E5E90..0x801E5EF4`, `0x801E5FC0..0x801E6018`): Run → orbit, Attack →
case 8, party slot over a dead target → case 8, else case 6, re-armed every
pass; `zora_glare_petrify_post` (`ctx[7] == 0x51`) reads case 6's pose on the
caster and `evil_medallion_rage_battle` (`ctx[7] == 0x0A`, between actions)
reads the far framing. Corrected reading in
[battle.md](../subsystems/battle.md#battle-camera-exact) and
[re-settled-threads.md](re-settled-threads.md#the-done-band-framing-and-the-two-orbit-writers).

### The battle-intro banner is raised from a top-seated `0x0303` placement record

*Falsified by capture.*

The reading: the intro banner is one of the placement records that park at
`(16, -24)` and live at `(16, 14)` with kind `0x0303` - records 67, 69..75, 89,
101, 102 - with the runtime overwriting the disc width with the measured enemy
name and sliding the element down from the park seat.

Why it was plausible: every *other* battle-HUD element does come from that
table through `FUN_801D8DE8`, which forwards the record field for field and
then glides it park-to-live; those records are exactly the right shape; and one
of them, record 68, really does get its width overwritten at runtime (disc
`w = 0`, spawned at the measured name width). So the shape existed, the
overwrite existed, and the slide existed - just not on this path.

What is true instead: `FUN_801D9D3C` places the intro labels itself with
immediates and never reads the table; the width is a call argument, not a
table write; and there is no slide, the labels appearing and vanishing at one
seat. See
[`battle.md`](../subsystems/battle.md#the-battle-intro-enemy-name-banner) and
[`re-settled-threads.md`](re-settled-threads.md#the-battle-intro-enemy-name-banner).

### `0x801CFA48` is a mid-function citation aliased to another overlay

*Falsified by disassembly.*

The reading (`world-map.md`, per-actor render dispatcher): the `0x2000` arm's
target `FUN_801CFA48` is a mid-function address inside `FUN_801CF88C` in the
menu / battle-action dumps, VA-aliased to some other overlay's routine, so no
clean dump exists to port from.

Why it looked right: slot-A overlays share base `0x801CE818`, and
`overlay_0897` dumps do print bodies at nearby VAs that are label artifacts.

What is true: `0x801CFA48` opens with `addiu sp,sp,-0x70` in
`overlay_battle_action_0898.bin` and in no other extracted overlay image, and
the routine's 12-word signature occurs in exactly one image on the disc. It is
the lightning effect-ribbon emitter (`THERNDER1` in PROT 0973's dev harness),
resident only with the battle overlay. The neighbouring name `FUN_801CFB94`
is the artifact - a branch label inside this routine colliding with a real
entry in PROT 0970. See
[`battle-action.md`](../subsystems/battle-action.md#overlay-local-prng-fun_801d0290).

### `_DAT_8007BD84` is a mode word the melee kernel branches on

*Falsified by disassembly.*

It is an **effect-instance handle**. Its only non-zero writer disc-wide is
PROT 0940's Cort "Mystic Shield" stager at `0x801F7678`, storing a
`FUN_80021B04` return; `FUN_8004CE2C` dereferences it at `+0x10` / `+0x56` /
`+0x72` and clears it when it fires cue `0x10D`. The two SCUS writers only
zero it. Callers that read it as a flag are testing that handle for null. The
companion reading - "the grunt's `s7` latch always passes" - is false too:
`s7` must equal `actor[+0x1F3]`, and only one of fourteen reaching definitions
loads it. See
[`re-settled-threads.md`](re-settled-threads.md#what-a-normal-party-attack-sounds-like).

### The slot cabinet is in neither the art pack nor any prim a traced slot function emits

*Falsified by a second read of the container.*

Both halves of the sentence were true and pointed the wrong way. PROT 1200 has
**three** descriptors, and the first read enumerated only descriptor 0 (the
TIM list); descriptor 1 is a 2160-byte untextured TMD that *is* the cabinet,
spawned as an ordinary actor by the slot init and drawn by the shared TMD
renderer - which is exactly why no slot function emits a large untextured quad.
See [`minigame-slot-machine.md`](../subsystems/minigame-slot-machine.md).

### A phase-gated effect draw is the candidate for the arena's object-1 dust decal

*Falsified by disassembly.*

No effect path touches it. Object 1 is ordinary backdrop geometry that the
SCUS battle loader `FUN_800513F0` trims from both backdrop actors' part lists
when `_DAT_8007B64B` is zero; the mist-free arena capture is the default, not
a phase gate. See
[`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md).

### `ctx[+0x26]` is a boss phase counter and `ctx[+0xD]` is a dead store

*Falsified by disassembly.*

The "boss phase counter bumped by the Cort form-change arm" reading came from
the one increment site (`0x801E6D3C`); the byte's unload reader `0x801E61B4`
passes it as a UI element id, and the only assignment anywhere is `0x65`, the
level-up banner. The "no port field and no reader" claim about `ctx[+0xD]`
missed three readers inside `FUN_801D5854` - it is the per-action camera angle
variant. Both in [`re-settled-threads.md`](re-settled-threads.md#two-battle-context-bytes-read-wrong).

### `0x801E3A20..0x801E3A64` is the Miracle continuation

*Falsified by disassembly.*

`s5` is `ctx + 0x11`, so `0x5(s5)` is `ctx[+0x16]` and the guard chain
(`ctx[+0x13] < 3`, record `+0xF4 & 0x2000`, counter zero) names the War God
Icon: this is the Attack x2 second pass, and the sole writer of `ctx[+0x16]`.

### The Miracle marker is armed by an input recognizer (`FUN_801E91E8`'s caller)

*Falsified by disassembly.*

No such writer exists. `+0x25F` has one `sb` in the corpus, in `FUN_80053CB8` at
battle-actor seeding, from the Ra-Seru equipment byte. `FUN_801E91E8`'s caller
stages a token into `ctx[+0x269]`, which still has no engine consumer.

### `DAT_8007BD10` is a per-slot control-mode byte

*Falsified by disassembly.*

It is the slot -> roster character id table; three routines index character
records with it as `0x80084140 + (byte - 1) * 0x414` (`0x801EF344`,
`0x80053CEC`, `0x801E39CC`). The "`== 4` = an AI-driven member" reading was right
in effect and wrong about the mechanism.

### The spell record's `+0x01` effect-class byte is undecoded

*Falsified by a read of this repo's own source (`inference`), with the bytes
confirmed against the disc.*

`legaia_asset::spell_names::SpellEntry::sub_class` has read `+1` for as long as
it has read `+0`, and `World::spell_table_sub_class` served it to the battle
host. The gap was a *consumer* gap - nothing turned the byte into a module.
The companion claim that the player Seru-magic block all shares `cat = 0x32 /
sub = 0` is false by `SCUS_942.54`: `0x83` Vera is `0x00 / 0x03` and `0x89` Orb
is `0x01 / 0x04`; only the nine enemy-side spells are `0x32 / 0x00`. Grade
`disassembly` for the bytes.

### The slot-B module band shares a library tail

*Falsified by disassembly and byte comparison.*

The reading: PROT 0958 and 0959 hold the same words past file `~+0x2A00`,
which read as a library object linked into several modules.

What is true: every one of the 64 module images ends in a byte-identical,
same-file-offset run of *another* extracted image, ending exactly at the
shorter image's length - and nine of them end in **PROT 0899's** bytes, a
slot-A overlay at a different base. That settles direction: mastering residue,
not linked code. The routine four 12288-byte images carry at printed VA
`0x801F9458` is the menu overlay's at `0x801D1298`. Seven worklist rows were
catalogued against an image that only holds the residue. See
[`cast-module.md`](../subsystems/cast-module.md#a-module-image-ends-in-another-images-bytes).

### The slot-B band: four readings the whole-band dump overturned

*Falsified by disassembly.*

| Reading | What is true |
|---|---|
| "No hard-coded `jal` into the capture-class band was found" | The sweep looked for a table keyed `id - 0x81`; the capture-class dispatcher `FUN_801F2160` keys on the spell record's `+1` sub-id and jumps through `0x801CF56C` |
| "0957's trampoline `0x801F9BA8` is reached by nothing" | It is arm 22 of `0x801CF56C`, at `0x801F233C` |
| "A slot-B module is two functions and has no internal `jal`" | 196 framed functions across the 64 images, 25 of them with internal calls; the two-function shape was the first thirteen |
| "PROT 0965 is a shifted sibling of 0967" | A pre-correction over-read; 0965 is the Doomsday module |

Function extents in the band come from frame matching, not from counting
prologues against `jr ra` words - a frameless leaf, an early `jr ra`, or a
`jr ra` word in a data tail breaks the count. See
[`cast-module.md`](../subsystems/cast-module.md).

### The summon draw runs 35-64 times a frame

*Falsified by capture; the original was a capture too.*

The reading: during a player Seru-magic summon the battle per-actor draw
`FUN_80048A08` fires 35-64 times per frame, which made it read as a per-part
driver walking the summon's mesh groups.

Why it looked right: the figure came from a real exec-breakpoint run, and a
summon *does* have many mesh groups, so a per-group call rate is exactly what a
part-driven draw would look like. It is also the number that retired the
move-VM / `FUN_801F7088` hypothesis, and that conclusion still stands.

Re-measured on the same catalogued state
(`scripts/pcsx-redux/autorun_enemy_move_render_path.lua`,
`gimard_burning_attack`, 400 vsyncs) the draw never exceeds **2** per rendered
frame - Vahn solo against one monster. The same probe reads 6 in a 3-vs-3 and 2
in a 1-vs-1: one call per **live actor**, not per part. The group walk is
inside the call. Any budget or scheduling argument built on the larger figure
is off by more than an order of magnitude; see
[`effect-vm.md`](../subsystems/effect-vm.md).

## Audio / sound driver

| Thread | Verdict | Why |
|---|---|---|
| The port sustains about half again retail's sounding voices, so its release tail runs long or it keys fresh slots | falsified (both explanations, and the statistic) | Plausible: the comparand had already survived two corrections (envelope level rather than phase word; the track a save actually holds), so the figure that was left looked like the real residual. Two instrument defects were stacked under it. The windows were never aligned - the retail capture pairs with engine frame `3111`, not frame 0 - and the emulator steps its envelope on an audio-paced thread, so a per-vsync capture's envelope statistics move with the host's speed: two captures of one state disagree on `env_level` for six of ten voice-frames. |
| The native BGM director's `stop` leaves the resume gate open | falsified (it was writing one of two pause representations) | Plausible: a resume that does not resume looks like a gate nobody closed, and the page-side director was the newer code. The native body was the defective one: two representations of "paused" exist and it wrote one, so the half it left unwritten is what the resume consulted. |
| The blocking bank for the Seru cast voice is `XA27` / `XA28` / `XA29` | falsified (seventeen other files) | Plausible: those three are exactly the clip slots the dispatcher's `1 / 3 / 5` remap produces, so reading the remap without reading which ids reach it names a real but unused corner of the table. The cue id is hardcoded in each cast module's own image - 62 of 64 - and the ids that occur resolve to `XA7`, `XA9..15`, `XA18..20`, `XA22`, `XA23`, `XA25` and `XA34`. |
| The XA cue-duration table has `0x40` entries | falsified (`0x110`) | Plausible: `0x40` covers every arts-shout id, which is the only family the table had been traced from, and the reader's one comparison is against `0x100` rather than against a length. Measured off the executable the table runs to index `0x10F`, with interior zero runs (`0x37..0x40`, `0x78..0x88`, `0xE8..0x10A`) that make a short read look like the end. The reader bounds nothing above `0x100`, so a truncated table silently drops every cast cue. |
| The sequencer's pause freezes sounding notes | falsified (it keys them off) | Plausible: a pause that resumes where it stopped is a freeze in every other sequencer, and the play cursor really is preserved. `FUN_800628F0` mode `0` raises slot flag `0x2`; the per-tick `FUN_80062F98` sees it and calls `FUN_800638D8`, which releases every voice the channel owns through `FUN_800684CC` before clearing the flag. Resume restarts from the cursor into silence, not into held notes. |
| `FUN_80058490` is a sound-driver lane, so the table feeding it is a cue list | falsified (it is `MoveImage`) | Plausible: the routine is called with a small id from a compact table in a battle context, which is exactly the shape of a sound-cue dispatch, and the table's values are in a plausible cue range. It moves a VRAM rect to `(0xE0, 0x1DC)` - CLUT row `y = 476` - so `0x801F6418` holds VRAM **x** coordinates. Anything reading those bytes as cue ids is reading a palette column. |
| `FUN_80068D94` as "`SsSepOpen` / SEP loader" (with `FUN_80068B98` as "`SsSeqOpen`") | falsified (it is the VAB-open head) | The plausible part: it validates a magic, reads a count at `+0x12`, `SsSpuMalloc`s, and patches a pointer table - the shape of a SEP/track loader, with the magic read as 'VAP'. The disassembly refutes it: the compare is `0x564142` against `word >> 8` plus low byte `0x70` - `pBAV`, the **VAB** magic - and `+0x12` is `ps`. The "per-track pointer table" is the ProgAtr table receiving the program → packed-tone-page rank map ([`vab.md`](../formats/vab.md#program-slots-vs-packed-tone-pages)); the mislabel hid that map, and with it the engine's tone collapse on sparse banks. Correct roles: [`audio.md`](../subsystems/audio.md#ssapi-seq-management-layer-above-libspu). |
| The entry that matches "`[u32 format == 2][u16 spu_addr[256]]`, every address `>= 0x8000`" is `monster.snd` | falsified (it is `summon.dat`; `monster.snd` is a multi-bank VAB two entries away) | [details ↓](#the-256-slot-spu-address-run-that-was-really-a-clut) |
| Op-`0x35` sub-op 9 is a **queue**, triggered by the next scene entry | falsified (it is a start behind an asset-load barrier) | [details ↓](#op-0x35-sub-op-9-was-never-a-queue) |
| `_DAT_8007B910` is the live screen brightness | falsified (it is the live **audio level**) | The reading fit the behaviour: the cell ramps down during a summon and back up when the action ends, and a summon does visibly dim the screen. But no reader supports it - all 26 dumped read sites end in a volume setter, none in a draw primitive. The dim rides the separate accumulator `_DAT_8007B440` (`FUN_801ED308` → the wipe emitter `FUN_8003479C`); the two ramp together, which is what made one look like the other. Answer: [`re-settled-threads.md`](re-settled-threads.md#_dat_8007b910-is-the-live-audio-level-not-screen-brightness). |
| `FUN_800684CC` keys a voice off by VAB id | falsified (it keys off by owner halfword) | Plausible: a routine that takes an id and stops voices reads as a bank teardown, and `SsVabClose` is the libsnd call with that shape. The argument is the owner halfword `seq \| track << 8` that key-on stamps at `0x8006661C`, so it stops the voices one sequence's track owns. A port that passed a VAB id stopped whatever happened to share the number. |
| The scene-VAB parse offset is why the audio oracle never converges | falsified (those scenes carry no bank at all) | Plausible: the parse really was being handed offset `0` where no entry on the disc begins with the magic, so every one of those calls failed - and a failing parse is a very good explanation for an oracle that converges on none of its candidates. The nineteen scenes the oracle qualifies carry no VAB entries, so the figure is unchanged after the fix. Two real defects can sit in the same subsystem without being the same defect. |
| `FUN_801EA9B0` cycles the BGM index | falsified (it plays the row; cycling is `FUN_801E9F64`) | Plausible: the dev menu's `BGM CALL` row shows a number that moves, the routine is the row's action dispatcher, and one routine per row is the tidy model. The arm at `0x801EACBC..0x801EAD20` reads the cursor `_DAT_801F2E90`, indexes the 10-byte sound-test rows at `0x801F2E94` and installs the row's global id - it is the fourth disc-wide writer of `_DAT_8007BAC8`. Stepping the cursor is the input half, `FUN_801E9F64`. Reading this arm as the cycler also mis-attributed a BGM writer to "world-map region change". |
| A mednafen save reports nearly every voice non-`Off` because nothing keys them off wholesale | falsified (there is no `Off` phase; the predicate was talking) | Plausible, and it explained the measurement exactly: every snapshot reported 20 to 24 of its 24 voices live, so a rule asking an engine frame's mask to be a superset of that looked unsatisfiable by any faithful playback, and `0 converged` looked like a statement about the comparand. `ADSR.Phase` has no `Off` member - it is `0` Attack through `3` Release, and a key-off parks a voice in Release - so the reader was counting every voice the state had ever keyed. Against `ADSR.EnvLevel != 0` retail holds 4 to 9 audible voices against the engine's 4 to 8, and the comparison is ordinary ([settled](re-settled-threads.md#audio)). |
| The dev sound-test table's row `i` carries global BGM id `2000 + i` | falsified (one id is missing from the run) | Plausible: the 71 rows open at `2000` and the sound-test track numbering is `2000 + i` everywhere else, so the table reads as that numbering written down. The rows carry `2000..=2043` and then `2045..=2071`: sound-test track 44 has no row at all, so above the gap the cursor and the id it installs differ by one. A reader that derives an id from the cursor names the wrong track for 27 of the 71 rows. |
| Retail's per-voice reverb mask is `0xC081`, routing voices 0, 7, 14 and 15 | falsified (that word is `SPUCNT`) | Plausible: the field was named `reverb_mode`, the value was stable across every frame of the capture, and `0xC081` read convincingly as a sparse voice mask. The PCSX-Redux SPU-ports blob is the hardware window `0x1F801C00..0x1F801DFF` **verbatim**, so the offset being read, `0x1AA`, is `0x1F801DAA` = `SPUCNT`: enable, unmute, reverb-master, CD audio. The real `EON` two words earlier reads `0x00FFFFFF` - all 24 voices - on every frame of the same capture. The trace field had been carrying three different quantities under one name, which is what let a register comparison look like a routing comparison ([settled](re-settled-threads.md#audio)). |
| The engine routes no voices through the reverb tank | falsified (both shipped hosts do; the **oracles** did not) | Plausible: the engine side of the trace really did report `EON = 0`, on every frame, from the engine's own code. It was not the engine the port ships. Both hosts build their SPU through `StreamResampler::new` -> `set_retail_reverb`; the trace and PCM oracles built a bare `Spu` instead, so the instrument was measuring an engine nobody runs. Generalises: an oracle that constructs its own subject has to construct the shipped one. |
| The engine runs about half retail's concurrent voices | falsified (two different pieces of music) | Plausible: mean 4.83 against 9.78 over the same scene name, twice, is the shape of a real deficit. A save's *scene* does not decide what it is playing - `_DAT_8007BAC8` does - and the retail `town01` capture held `2000`, the overworld track, because that save walked into town from the world map, while the engine trace played the `2016` the scene's own prescript selects. Same track, comparable stretch: 9.67 / 18 against 9.78 / 19 ([settled](re-settled-threads.md#audio)). |
| Retail doubles about one note in six across two voice slots | falsified (slots-per-note is the arrangement's, not a side's) | Plausible, and it was the residual left after the track pairing was fixed once: retail `1.158` against the port's `1.002` looked like an allocator difference with a mechanism behind it. On a track-aligned pairing the ratio **reverses** - retail `1.002`, the port `1.037` to `1.061` - so the statistic belongs to the piece, not to the renderer. It also kills the caveat that the doubling might be town SFX in the retail window: a second capture in the same town, on the same walk, reports `1.002`. |
| The `nilboa` duel states read BGM `4096` | falsified (they read `2009`; the `4096` states are the ending ones) | Plausible: the states were opened looking for an anomaly and one was found, in a corpus where most states carry `2000..=2068`. The misread is worth keeping because the value is real and its owner is not: `4096` = `0x1000` is the **park sentinel** both streaming slots share, and it appears where a save has no field track owed - the endings ([settled](re-settled-threads.md#audio)). |

### Op-`0x35` sub-op 9 was never a queue

**Falsified:** that field-VM op `0x35` sub-op 9 stashes a BGM track for some
later trigger, and that scene entry is that trigger.

The word "Queue" sat in the sub-op table with no body behind it, and it is a
reasonable guess: the op appears next to the pause / resume / stop control
words, it is never the op a scene's *entry* script uses, and its arm does
begin by comparing two globals - which reads as "is a slot free yet?".

The arm at `0x801E0224` refutes it. The comparison is
`*0x8007BAB8` (the index the resolver produced) against `*0x8007BA9C` (the
index actually loaded); the mismatch branch goes to `0x801DEE4C`, which is
`move s8,s4` - the dispatcher's restore-PC idiom, so the script re-runs this
same instruction next frame. That is a **wait on the asynchronous asset
load**. When it clears, `sw v0,-0x4538(a1)` writes `*0x8007BAC8 = id`, the
identical store sub-op 1 makes. Sub-op 11 (`_DAT_8007BA9C = -1`) is the
barrier's arming half.

So sub-op 9 is sub-op 1 plus a wait, and it is what a **cutscene** changes
music with mid-scene. Two things kept the wrong reading alive: a scene-corpus
BGM sweep only runs prescripts, which emit sub-op 1 exclusively, and the
sweep's recording director folded its start and queue hooks into one list -
so a deferred track and a playing one produced identical output. The audible
symptom is narrow and easy to attribute elsewhere: the cutscene plays silent,
and its score starts over the *next* scene the player walks into.

Full arm + the port's routing: [`script-vm.md`](../subsystems/script-vm.md#sub-op-9-is-a-start-not-a-queue).

### The 256-slot SPU-address run that was really a CLUT

**Falsified:** that a `[u32 mode == 2]` header followed by 256 `u16`s all
`>= 0x8000` identifies a packed monster sound bank, and that the entry matching
it is `h:\mpack\monster.snd`.

Why it was convincing: `0x8000` is exactly the boundary an SPU sample address
clears once the reserved low region is skipped, so "256 halfwords, every one
`>= 0x8000`" reads as a fully-populated 256-slot address table, and a leading
`2` reads as a format word. One PROT entry matched, and its CDNAME label named
the sound cluster.

What it actually matched is `summon.dat` (extraction 893,
[`summon-readef.md`](../formats/summon-readef.md)), whose header word is a mode
`2` and whose next `0x200` bytes are a **BGR555 CLUT with the STP bit forced on
every non-zero entry** - which sets bit 15 of all 256 halfwords for a reason that
has nothing to do with addresses. The tell the predicate cannot see: the values
**repeat** (`0x8000 0x8000 0x8000 0x8000 0x8001 0x8001 …`), and SPU sample
addresses are strictly increasing. A monotonicity check would have rejected it;
a threshold check could not.

`monster.snd` is extraction **891**, and the loader says so outright:
`FUN_8003E104` does `li v0,0x37d` (raw TOC `0x37D` = extraction 891) beside the
`h:\mpack\monster.snd` path string. Entry 891 is a 206-bank multi-VAB archive, so
the monster SE bank is a **multi-bank VAB**, not a bespoke address table - and the
`vab_multi_bank` class that had been described as "the `level_up` cluster's"
archive was reading the same CDNAME `+2` shift off an extraction filename
([`cdname.md`](../formats/cdname.md#numbering-space)). `see
ghidra/scripts/funcs/8003e104.txt`.

**Generalises to:** a byte-histogram or threshold predicate over a fixed-size run
identifies a *shape*, never a format. Where the shape encodes an ordering
(addresses, offsets, LBAs), assert the ordering - it is the cheapest thing that
separates the format from its look-alikes. The `monster_sound_bank` class is kept
and pinned at zero matches so the shape stays named rather than being
re-derived by accident.

### bse.dat: three readings the gp+0x678 trace overturned

*Falsified by disassembly.*

| Reading | Why it looked right | What is true |
|---|---|---|
| `bse.dat` record `+4` is a `u32 v` taking only 0 and 2 | Reading `+4..+7` as one word; `+5..+7` are zero in every retail row | `+4` is a `u8` category (the VAB-slot selector) plus three bytes no reader touches; "0 and 2" described the authored defaults, and the cue router rewrites the byte per cue |
| `bse.dat` is loaded once at sound-init and held for the session | `FUN_8001FA88` has the shape of a sound-init routine and also loads the per-set `.dpk` | Its one caller on the whole disc is battle init `FUN_800513F0`; the bank reloads per battle, and the buffer slot `0x8007B8D0` is repointed at every field load |
| The `bse_bank` detector's "`u32` at `+4` under `0x100`" gate is a value bound | It reads as a range check on the trailing field | A category byte can never exceed `0xFF`, so the bound could only fail on a non-zero trailer - it is a zero-trailer test |

The consumer was "untraced" only because every reader forms `0x8007B990` with
`lui` + `lw`, a pair the five-form address scan does not accept. Details:
[`re-settled-threads.md`](re-settled-threads.md#bsedat-record-columns-and-the-gp0x678-consumers).

### `bse.dat` carries a second record family with a resident consumer

*Falsified by disc bytes.*

The bytes really do repeat on a fixed stride inside the loaded buffer, and a
matching run in a second entry looked like a shared footer. Both are the same
builder's fill inside `VagAtr` tone rows belonging to a *different file* left
in the sector (888's tail equals 886's and 1063's; 1062's equals 1056's). No
family, no consumer to find. The companion reading - that `FUN_8001FA88`'s
**dev** branch loads PROT `0x37A`, tying the `.dpk` name to the `sound_data2`
family - is wrong the same way: it is the retail branch, and `0x37A` is
`bse.dat`. See
[`re-settled-threads.md`](re-settled-threads.md#audio).

## Title / boot / overlays

| Thread | Verdict | Why |
|---|---|---|
| Mode 18 has no hand-off store, so the front-end chain skips it | falsified (`0x801CE8DC` in PROT 0902) | Plausible: a three-form scan for stores to the mode word found one for every other INIT handler and none for this one, which reads as a mode that leaves by some other route. The store is there - `li v0,0x13` at `0x801CE8D4` and the store at `0x801CE8DC`, inside `0x801CE844`. The gap was in the corpus reaching PROT 0902, not in the game. |
| The retail title screen runs under game mode `0x10`, which leads load / new-game -> field | falsified (the title's own mode is the **card** mode `0x17`) | Plausible: `0x10` is a real mode, a real handler stores it, and it really is on the path from boot to the title - so a trace that samples the mode word early sees `0x10` and stops looking. `0x10` is one frame of logo INIT. The chain is six stores, each written by the handler that hands off rather than by any table's `next` field: `0x8001D5B8` -> `0x10`, `0x801CEC94` -> `0x11`, `0x801CF4D4` -> `0x16`, `0x80025974` -> `0x17`, `0x801DFC00` -> `0x02`, `0x80025E50` -> `0x03`. |
| The Muscle Dome is not an OTHER-game mode | falsified for the **hub**, true for a round | Plausible: a dome fight is an ordinary battle under the battle mode, which settles the question for everything a player spends time in. The dome *hub* - the contest screen between fights - is PROT 0977, sub-id 5, game mode 24, the OTHER-game mode. One answer was being applied to two different screens. |
| A mode-change edge clears three `gp` words | falsified (four) | Plausible: three of the four sit adjacent at `gp+0x538` and `gp+0x55C` and read as one cleared block. The edge at `0x800161F4` also clears `0x8007B938`; `gp+0x564` and `gp+0x494` are mode **copies** and are not cleared. |
| Title sub-mode `0x10` selects between a prompt and a menu | falsified (it is one state) | Plausible: the screen visibly changes between a "press start" prompt and a two-row menu, so a sub-mode looked like the selector. Retail's `0x10` is a single state that polls the card, polls Start and runs the cursor; the prompt-vs-menu split is a port-side phase, which is fine as long as nothing reads it back as retail's. |


### The title sub-mode word lives at `0x801DD920`, and `0x02` is a screen a player can see

*Falsified by disassembly, with a cold-boot capture agreeing.*

The reading: `FUN_801DD35C`'s init writes sub-mode `0x02`, so `0x02` is the
cold-boot screen and the word it writes is at `0x801DD920`, the address the
dump prints on that line.

Why it looked right: `0x02` really is a complete two-row menu (rows at y 107
and 120, confirm mask `0x44`, advancing to `0x14`), so it is not dead code in
the "never assembled" sense; and reading a printed line address as a data
address is exactly what a dump invites.

Both halves are wrong. `0x801DD920` is the **instruction** address of the
`sw v0,0x204(a2)` that performs the store, with `a2 = 0x801F0000` from a `lui`
four instructions earlier - the word is `0x801F0204`, which is the same word
the page's own sub-mode switch is over. And the `0x02` store is overwritten
with `0x11` whenever the entry word `_DAT_8007BB00` is non-zero, which the boot
`init.pak` raises unconditionally at `0x801CEB84`; a per-vsync cold-boot poll
never observes `0x02`. See
[`re-settled-threads.md`](re-settled-threads.md#a-cold-boot-always-shows-title-sub-mode-0x10).

### The attract sequence can be armed from any title sub-mode

*Falsified by disassembly.*

The attract countdown at `0x801EF16C` is a state-struct field like any other,
and several arms of the tick touch state-struct words, so "any idle sub-mode
lets the attract fire" is the natural reading of a global timer.

The arming code is in one arm only. The countdown is decremented and its
underflow writes `_DAT_8007B83C = 0x1A` inside the extent
`0x801DDB0C..0x801DDD94`, which is sub-mode `0x10` `AttractIdle` and nothing
else; every other arm reaches the shared epilogue without touching it. The
preceding sub-mode `0x11` spends a *different* accumulator
(`_DAT_8007BAB4`) and hands to `0x10`, which is what makes the two look like
one timer from a capture.

### There is exactly one master-mode-`2` writer, at `0x801DFC00`

*Falsified by disassembly.*

`0x801DFC00` is in `LaunchGame` (`0x06`), the NEW GAME exit, and a single
"leave the title into the field" writer is the shape the mode graph suggests.

There are two. `LaunchFade` (`0x16`) writes the same master mode at
`0x801DFAFC` on the **load** route (`state[-0xEA8] == 1`), and both arms clear
`_DAT_8007BB00` as they go. A sweep that found the new-game store and stopped
missed the CONTINUE path entirely - which is the half a save-file boot takes.

### The title slider `state[-0xEB4]` is clamped to `[0, 0x2C]`

*Falsified by disassembly.*

`0x2C` appears as a bound in both of the slider's arms, and a value bounded
above by `0x2C` with a natural floor of `0` reads as a range.

Neither arm implements that range. The decreasing arm subtracts
`frame_scalar << 3` and then does `slti v0,v0,0x2c` / `beqz`: if the result is
**below** `0x2C` it is forced back **up** to `0x2C`
(`0x801DFC78..0x801DFC98`). The increasing arm adds the same step and forces
anything at or above `0x2D` back **down** to `0x2C`
(`0x801DFCA4..0x801DFCC4`). Both arms converge on the single value `0x2C`
from their own side; there is no `0` floor anywhere, and the seeds the graph
writes are `0x100` (`0x801DD88C`, `0x801DE094`) and `-0x16` (`0x801DECB8`) -
both outside the supposed range. The cell is a settling animation parameter,
not a bounded slider position.


### The title screen is loaded before the mode table is consulted

*Falsified by disc bytes and `main()`'s disassembly.*

The 28 mode-table rows carry no name resembling "title", so the screen was
read as running ahead of the dispatcher from its own pre-mode-dispatch boot
load. Both halves are false. The one pre-loop overlay load in `main()` is
`0x8001612C jal 0x8003ebe4` with `a0 = 0`, which is extraction 0895
(`init.pak`, the publisher logos) reached by mode 16 `READ`; the title overlay
PROT 0899 is loaded by mode 22 `CARD` (`0x800258B4`, `a0 = 4`) and its tick
runs as a spawned actor under mode 23. The name search was the dead end, not
the table. See
[`re-settled-threads.md`](re-settled-threads.md#title-screen-mode-table-prot).

### `FUN_801DD35C` lives in an unindexed PROT.DAT gap between entries 899 and 900

*Falsified by TOC arithmetic and a byte search.*

There is no such gap - an entry's size is the sector span to the next, so the
TOC partitions `PROT.DAT` without one (899 ends at sector 47301 where 900
begins). The tick's 48-byte prologue occurs exactly once on the disc, inside
entry **0899** at file `+0xEB44`. Another coordinate measured under the
superseded entry-size expression.

### `FUN_8003EAE4`'s flags are consumed by an untraced CD driver

*Falsified by disassembly.*

`gp+0x910` and `gp+0x890` have no reader anywhere in SCUS or the 1233 PROT
entries - every access is a store. The real CD-callback sequencer
`FUN_8003D764` reads none of the three cells; it dispatches on `gp+0x928`,
which only `FUN_8003D53C` writes and which `FUN_8003EAE4` merely reads as a
no-op entry gate. See
[`re-settled-threads.md`](re-settled-threads.md#fun_8003eae4-is-a-seek-plus-bookkeeping---and-the-driver-is-not-untraced).

### `0x801CE9C0` is an entry point in no image, so mode 16 is a stripped dev path

*Falsified by disassembly.*

The reading: SCUS `FUN_8002612C` (mode 16) jumps to `0x801CE9C0`, which sat
mid-way through the debug-menu overlay's `FUN_801CE97C` in every dump, so
the mode was read as a retail-stripped path jumping into slot A blind.

What is true: it is VA aliasing at the shared slot-A base. `0x801CE9C0` is a
clean function entry in PROT **0895** (`init.pak`, file `+0x1A8`), the
publisher-logo pass that mode 16 exists to run. See
[`boot.md`](../subsystems/boot.md).

### `FUN_801CE9C0` draws the publisher logos

*Falsified by disassembly.*

It uploads them. Its four `FUN_800198E0` calls are `LoadImage` wrappers over
rects it forms from each TIM's header, and the routine contains no primitive
emit. The quads come from the sprite-descriptor table at `0x801F369C`, emitted
by `FUN_801CFBB8` ([settled](re-settled-threads.md#the-publisher-logo-quads)).

### SCEA unfolds as a 2x2 grid of 32-row strips

*Falsified by disassembly.*

The engine's `STRIP_GRID` `(2, 2)` was fitted to the visible content. Retail's
descriptors 2 and 3 are two 64-row halves of the 256x128 TIM, and PROKION's
halves are 127 rows, not 128. Pixel-equivalent by accident; not what retail
does.

### `0x801D06E0` is a SHARED_TAIL with no `jr ra`

*Falsified by disassembly.*

The worklist row read it as a tail exiting `j 0x801dee50`. In PROT 0895 at the
same base it is a 22-instruction leaf with its own `addiu sp,sp,-0x18` frame,
four `TestEvent` calls and its own `jr ra` at `0x801D0730`. Same cause as the
`0x801CE9C0` entry above: a row filed before its image was mapped.

## Containers / placeholder slots

| Thread | Verdict | Why |
|---|---|---|
| PROT 0967 carries a 2,992-byte un-dumped code run | falsified (92 bytes of one leaf, a 1,757-byte prompt pool and 1,143 bytes of PROT 0966's tail) | Plausible: the shape classifier called the window `plausible_mips`, and the image is a slot-B module outside the dumped band. The window was mostly another module's bytes, read before the inherited-tail cut existed; `disc-coverage.py` had reported no un-dumped code there all along. |
| The PROT 0898 head block carries a `switch` jump table from `0x9B4` | falsified (nine runs of jump-table words below `0xDF8`, not one) | Plausible: one run of in-image VA words after a string pool reads as one table. Two of the nine now have their `jr` consumer (`0x801EA9FC` over 179 arms, `0x801EB558` over five); the rest are still unbound. The count of nine fell in turn - see the twenty-two-table row below. |
| PROT 0970 is the world-map top-view debug image | falsified (it is the STR / MDEC FMV overlay; the top-view image is 0981) | Plausible: both are slot-A images at the same base, both sit in a CDNAME block whose labels inherit forward, and a three-digit entry number is easy to carry across a sentence. 0970's own operands are the STR play loop's - the decode context at `0x801D19A0`, the two slice buffers, the STRv2 VLC table - and 0981's are the world map's. An entry number is only as good as the operands under it. |
| PROT 0974's 10 KB of readable text is a string blob | falsified (an 81-record `0x84`-stride roster) | Plausible: the run is legible text with separators, which is what a string pool looks like from a hex dump, and the text is Japanese in a USA-build image, which suggested a foreign build on top. The loop operands in `FUN_801CED68` walk a fixed stride from `0x801CEF40`, and 34 of the image's 37 SCUS `jal` targets land on this disc's function heads (a genuinely foreign image scores 0 of 42). Shift-JIS stored as LE `u16` is not evidence of anything but an encoding. |
| PROT 0975's `0x801D4138` run is a jump table, or data, or a function interior | falsified (it is PROT 0972's code) | Plausible: three readings in turn, each fitting one property of the run - no prologue, no `jr ra`, no caller. All three are explained at once by the packer: its buffer is indexed by file offset and never cleared, so a short entry ends in the previous image's bytes at the **same file offset**. The 1,760 bytes from `0x5920` are byte-identical to 0972 there. Byte accounting does not cut such a tail the way disc coverage does, which is why the run reads as this image's residue. |
| `FUN_8001E890`'s own frame keeps the registrar away from a foreign block | falsified (the checksum guards the other buffer, on the other arm) | Plausible: the routine does re-sum PROT `0x36C` and compare against the boot-time sum at `gp+0x6B8`, and a checksum in the frame is exactly the guarantee the question wanted. It sums the **VRAM read-back** it is about to decompress from, not the pack the registrar walks; and the register-only arm never reaches it, because `bne v1, v0` at `0x8001E974` jumps past the sum whenever the load word is `1`. The gate's writers are still what keeps the walk safe. |
| A rebuilt PROT 0874 with a different section-0 decoded size can be hand-built to reproduce the wild read | falsified (the container is not constructible that way) | Plausible: the symptom was measured on a rebuilt container, so a rebuild is obviously the instrument - and "make the section bigger" is what a size-sensitivity test looks like. The LZS decode is length-driven by the descriptor and `FUN_8001ED60` sizes the section-0 / section-1 buffers from the container header words at `gp+0x69C` / `gp+0x6C8`, so a header-byte-exact rebuild whose decoded size differs gets a **truncated** pack instead of a differently-sized one. No shipped patcher path changes that size either. The producer to bracket is retail's own unclamped registrar over a battle-load pointer, not a patched disc. |
| A scene bundle's descriptor offsets can fall outside the entry | falsified (all 668 are inside) | Plausible: the parser really did compute offsets past the file end, and a format that stores absolute offsets into a streamed container would produce exactly that. The offsets were being measured against the over-reading entry-size expression; against the corrected extent every descriptor in all 102 tables lands inside its own entry. |
| PROT 0874's `meta[1]` (`0x2CBA0`) is a VDF tail offset past the LZS payload inside the entry | falsified (the entry is `0x19800` bytes; `0x2CBA0` is 78 KB past its end) | Plausible: the battle loader really does walk a flat `[count][offsets]` pack through `FUN_8001FBCC`, and the character pack really does carry a second meta word shaped like an offset. Two things kill it. The loader's constants `0x368..0x36B` are raw TOC `872..875` = extraction `870..873`, so the pack it walks is `vdf` (extraction 872), while the character pack is extraction 874 = raw `0x36C`. And the word indexes nothing: the entry is exactly `0x19800` bytes, and `0x2CBA0` is the **sum of the descriptors' decompressed sizes** (`0xB49C + 0x41E0 + 0x1D524`). See [`character-mesh.md`](../formats/character-mesh.md). |
| The dome panel still is uploaded as 320x64 rows stepped down in `y` | falsified (four 64x256 columns) | Plausible: a 320-wide destination and a y-stepped upload is how the `int.tim` family (`0x4C7` / `0x4C8`) really does load, and that family sits next to this one. The measured upload is **four** `LoadImage` calls of 64x256 at `x = 384 / 448 / 512 / 576` - texture pages 6..9 - keyed on raw TOC `0x36C`. The two families were being described with one geometry. |
| The `scene_v12_table` load path reads the `.MAP`, the `.PCH` and `efect.dat` | falsified (that is the **dev** branch) | Plausible: the three-file read is right there in the routine, and nothing in it is marked dev. The fork is on `_DAT_8007B8C2` at `0x8001F87C`, and retail takes the other leg: one `FUN_8003E800` read of `0x28` sectors from `0x8001F9A4`. `FUN_800608F0`, reached from the dev leg, is the `break 0x103` host trap - which is the tell. |
| Both `record[0] + 0x5C` accesses live in PROT 0900 | falsified (one is PROT 0901) | Plausible: the two hits sit close together in a scan of a window that was believed to be one entry. One of them is past 0900's real end and belongs to 0901 - the entry-size over-read again. A sweep of 518,656 words across 84 images finds no reader of that displacement on a `record[0]` at all. |
| There is no `jal` at `0x801F78D0` in the world-map overlay | falsified (the test read the delay slot) | Plausible: the address was checked directly and holds no call, which is a clean negative. The call is at `0x801F78CC` and `0x801F78D0` is its delay slot; three slot-B images hold one. The conclusion the negative supported happens to survive, which is what makes this the dangerous shape. |
| Pochi-fill slots are stale mastering scratch, and some parse as valid TIMs | falsified (every slot is one 2048-byte sector; 0 of 266 carry a TIM) | [details ↓](#pochi-fill-slots-as-stale-mastering-scratch) |
| The world-map kingdom bundle is PROT `0085` / `0244` / `0391` | falsified (it is `0086` / `0245` / `0392`) | [details ↓](#assets-named-by-the-entry-the-over-read-window-started-in) |
| The battle-form character pack holds seven atlases inside PROT `1204`, the last truncated, with CLUT row 496 skipped | falsified (eight whole atlases in PROT `1205`; 496 is the eighth, not a gap) | [details ↓](#assets-named-by-the-entry-the-over-read-window-started-in) |
| The title TIM ships as three multi-bank duplicates in PROT `0888` / `0889` / `0890` | falsified (one copy, in `0890` at `0x14228`) | [details ↓](#assets-named-by-the-entry-the-over-read-window-started-in) |
| `scene_tmd_stream` entries can hold two or more concatenated sub-streams (the "two-list" shape) | falsified (one stream per entry; 0 of 182 hold a second) | [details ↓](#concatenated-sub-streams-in-a-scene_tmd_stream-entry) |
| The stage backdrop renders as half a bowl because bytes are missing - mirror it to recover them | falsified (the half is authored; 182 of 182, and nothing is unread) | [details ↓](#the-backdrop-shell-is-drawn-once-so-no-completion-exists) |
| ...and therefore nothing completes it, so drawing a second copy is a regression | falsified (retail links **two** backdrop actors; the second carries a per-stage transform) | [details ↓](#the-backdrop-shell-is-drawn-once-so-no-completion-exists) |
| PROT 0968 is a 4 KB module (pointer-table head, 10/11 self-pointers, 2+8 spawn calls) | falsified (its own content is 2600 bytes; the rest is stale buffer) | The entry really is 2 sectors, but only file `0x00..0xA28` is 0968's. The trailing 1496 bytes are 0967's bytes at the *same* file offsets, cut mid-string at the sector boundary, and **nothing in 0968's own window references them** - no `jal`, no `j`, no materialisation. Every structural figure ever quoted for the entry was measured across both modules at once, which is why they never cohered. Full accounting on [`re-settled-threads.md`](re-settled-threads.md#prot-0968---the-cort-battle-stage-overlay). |
| The literal `0x801F69D8` in `SCUS_942.54` is a cross-image reference naming 0968's loader callsite | falsified (it is the slot-B base constant) | The only literal-word hit outside the shared-base band, and therefore the only one an aliasing argument could not dismiss - which made it read as the last live lead. It is the SCUS global `0x80010390` holding the **slot-B overlay load address**, twin of `0x8001038C` for slot A, read by `FUN_8003EC70` and never written. A reference to a shared load base names the *slot*, not a tenant. Meanwhile the real callsite was never findable that way: the stage-overlay parameter is **computed** (`stage_id + 0x47`), so the constant `0x49` occurs nowhere. |
| Battle `DAT_8007BD0C == 0xB5` at `0x801E6D04` is a test on the Lapis Wave **spell** id | falsified (it is the **formation monster** id - Cort) | Two id spaces collide on `0xB5`: spell `0xB5` is Lapis Wave, formation `0xB5` is monster-archive 181, Cort. The byte the branch reads is `*(u8 *)0x8007BD0C`, which is the formation id array, and its guard is an HP-reached-zero test on the first enemy actor - a form-transition trigger, not a cast. The wrong reading was self-consistent because Cort is also the caster of Lapis Wave. |
| The Muscle Dome `INTERVAL` screen is the `(384, 0)` 320x256 still | **reversed** (true on a re-entered hub; the first visit draws a live render) | The original verdict read a first-visit intermission - `koin1`'s own scene, no primitive sampling a page at `x = 384` - and holds for that visit only. Visits two and three of a dome run read latch `_DAT_801D1AE0 = 1`, hub arm `0x0A`, level `8`, with both still packets (tpages `0x106` / `0x109`, colour `0x2C080808`) in the primitive pool, emitted by `FUN_801D00F8` in PROT 0977 ([settled](re-settled-threads.md#battle--arts--level-up), [`ringside-still.md`](../formats/ringside-still.md#on-a-natural-re-entry)). Anchor kept: [`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md#the-interval-screen-is-a-live-render-not-the-still). |
| `init.pak` carries five logo TIMs | falsified (four) | Plausible: five plausible TIM headers parse out of the region, and a fifth logo is not a surprising thing for a boot image to hold. The fifth sits **inside** logo 3's pixel data, where a header-shaped run of bytes is exactly what image data produces. Counting parseable headers is not counting images unless the parse consumes what it claims. |
| The type-`0x14` FLAG descriptor is a property of every count-4 and count-5 bundle | falsified (28 of 105 tables, at every count from 4 to 7) | Plausible: the first bundles examined were count-4 and count-5 and every one of them carried the FLAG slot last, which makes the count look like the rule. Across all 105 descriptor tables the FLAG slot appears on 28, whose counts run 4 to 7, and in all 28 it is the **last** entry. The position is the rule; the count was a coincidence of the sample. |
| `urudre1`'s 584 unexplained bundle bytes are sector padding | falsified (562 of them are high-entropy residue) | Plausible: the number is smaller than a sector, it sits at the end of the entry, and the pack's payload hashes identically to the standalone copy either way - so "padding" costs nothing to believe. The payload claim does hold exactly: the pack's last member's TIM closes on byte 343,480, the same SHA-256 as `0456_urudre1.BIN`. What follows is not fill. Calling a non-zero tail "padding" hides a question about the mastering step that wrote it - since answered: the run is the packer's buffer, an earlier entry's bytes at the same offset ([settled](re-settled-threads.md#measurement--corpus)). |
| The PROT 0898 head is nine jump tables | falsified (twenty-two; two more sit above it) | Plausible: nine runs of in-image VA words is what a scan of the bytes shows. Tables that abut with no pad word merge into one run; read off their dispatches - each `sltiu` bound times four - twenty-two bases tile the head, 850 arms. |
| A scene bundle's last-sector residue is its walker's unread tail | falsified (it is the packer's buffer) | Plausible: the bytes sit right after the last stream the walker parsed. Byte `k` of that run is byte `k` of the nearest earlier TOC entry whose extent reaches `k`, for 80,337 of 80,337 bytes across 90 bundles, and the same holds for the `lzs_container`, `pack` and `bse_bank` tails. The donor need not be an overlay. |
| The packer-buffer run is confined to an entry's last sector | falsified (PROT 0976's tail is `0x98C` bytes of 0970) | Plausible: every scene bundle's run fits its last sector. The suffix is searched over the whole entry now, which reproduces 82 of the sibling rule's 83 cuts. |
| PROT 0970's 3,152-byte run above the init flag is uninitialised data, or code | falsified (two MDEC command packets and the register-pointer block) | Plausible: one reading followed the image's real `.bss` hole below, the shape classifier called the run `plausible_mips`. `0x801D0D58` is the quant packet (`0x40000001`), `0x801D0DDC` the IDCT packet (`0x60000000`), and `0x801D0E60..0x801D0E9B` fifteen DMA / MDEC register pointers. |
| `FUN_801CFCDC` stages MDEC output rects into `0x801D0D5C` / `0x801D0D9C` | falsified (it uploads the quant tables) | Plausible: two sixteen-word destinations filled from a caller pointer look like a rect pair. They are the luma and chroma matrices of the quant packet behind the header at `0x801D0D58`. A later reading that it copies both packets was wrong too: only the quant packet is written; the IDCT packet at `0x801D0DDC` is static and is sent as is. |
| PROT 0899's 3,552-byte zero run at `0x1EB28` is uninitialised data | falsified (inter-asset fill) | Plausible: a long zero run inside an overlay usually is `.bss` (0970's is). No instruction in any image forms an address inside this one; it lies between the save-menu atlas and the save-slot icon sheet. |
| PROT 0967's prompt pool has no consumer | falsified (twenty-eight strings formed at forty-one sites) | Plausible: no literal pointer word names it. The module's own code forms each string address with `lui`/`addiu` between file `0x278` and `0xA7C`. |
| A slot-B image never calls itself with `jal` | falsified (PROT 0967 does) | Plausible: it holds across the cast band, which is why function extents there are recovered by frame matching. 0967 reaches its frameless leaf `FUN_801F7628` by `jal` from `0x801F7184` and `0x801F7460`. |
| PROT 0900 and 0901 are shifted copies of one image | falsified (an old entry-size artifact) | Plausible: `static-overlays.toml` carried the byte comparison. It compared 0901 with itself through the over-reading entry size; 0901's own content ends at file `0x252A`, where 0900's bytes begin. |
| PROT 0901's inherited tail starts at `0x26B0` | falsified (`0x252A`, donor 0900) | Plausible: it was the sibling rule's cut. 0901's code ends at `jr ra` on `0x24DC`, and the run from `0x252A` opens mid-routine on 0900's epilogue and is referenced only from 0900. |
| PROT 0780 (`edteien`)'s residue is a scene-event-script walker's tail | falsified (the walker never started) | Plausible: the class was right and the bytes were unclaimed. Its prescript holds two records, below the standalone count floor, so the walker refused the entry; it now reads the records positionally, as the loader does. |

### Assets named by the entry the over-read window started in

*Status:* falsified - each asset is where it always was; only the `(entry,
offset)` name for it was wrong

Same root as the pochi row below, but the symptom is a **name** rather than a
corruption, which is why it survived longer. Under the pre-correction entry size
a reader positioned on entry `N` could see entries `N+1`, `N+2`… so an asset was
recorded as "PROT `N` offset `K`" whenever the scan that found it started at `N`.
The coordinate is not wrong about the disc - `start_lba(N)*0x800 + K` really is
where the bytes are - it is wrong about which entry owns them, which is the only
thing a correctly-bounded reader can use.

The plausible part is that each wrong name came with corroboration:

- The kingdom bundle "at `0x1800` of entry 85" had a table there, with the right
  count and the right first descriptor offset. It is entry 86's offset 0 - and
  the block layout (`.MAP` / v12 header / prescript / bundle) says entry 85 is
  the prescript.
- The battle-pack atlases had a *consistent stride from a consistent base*, and
  a truncated last member is a normal thing to find at the end of a container.
  `0x25804` is 1204's own length plus 4, i.e. entry 1205 offset 4; the "seven"
  and the "truncation" were both where the window stopped, and the eighth
  atlas's CLUT row read as a deliberate gap in a 490..497 run.
- The title TIM's three "duplicates" were **byte-equal**, which is exactly what
  you would expect of a multi-bank duplicate - and also what you get when three
  arithmetics resolve to one absolute offset.

Two lessons worth carrying. First, byte-equality between two `(entry, offset)`
pairs is evidence of *duplication* only after you have shown the two pairs
resolve to different absolute offsets; otherwise it is a tautology, the same
shape as the falsified "PROT 0900 and 0901 are shifted copies". Second, a
container's member count and a member's size are properties of its **framing**
(a chunk chain, a descriptor count), not of where a buffer happens to end - a
count derived from "how many fit before the buffer ran out" is measuring the
reader.

The corrected coordinates, and the two invariants that keep them honest, are in
[`prot.md`](../formats/prot.md#a-entry-offset-pair-is-only-a-coordinate-if-the-offset-is-inside-the-entry).

### Pochi-fill slots as stale mastering scratch

*Status:* falsified - the corrupting pages came from the **next** entry, reached
through an over-reading size expression

The plausible part was strong enough to reach [`CLAUDE.md`](../../CLAUDE.md) and
stay there: reserved-but-unused filler holding leftover bytes from an earlier
master is an ordinary thing to find on a PSX disc, and the hazard had a
**reproducible exhibit**. Two `64x256` pages uploading to framebuffer `(768,0)`
and `(832,0)` erased a ground atlas, every run, and the sweep was positioned on a
pochi slot when it happened.

What refutes it: every one of the 266 `Class::PochiFiller` entries is exactly one
2048-byte sector of fill, and **none** carries a parseable TIM header. There is no
stale image in a pochi slot to upload. The corrupting pages belong to the
`scene_tmd_stream` entry that *follows* the pochi slot, and the sweep reached them
through the entry-size expression that spanned into neighbouring entries - since
corrected in [`prot.md`](../formats/prot.md).

The lesson is the transferable part, and it is not about pochi slots. **An
over-reading reader makes the next entry's bytes look like the current entry's
content**, so a symptom gets attributed to the entry the reader is positioned on
rather than the entry it actually read into. The bug reproducing every single time
is what made the wrong attribution durable: reproducibility confirms that
*something* is wrong at that step, and says nothing about which entry owns the
bytes. Format-level claims derived from a sweep are only as sound as the sweep's
bounds - re-derive the bound before believing the claim.

See [`pochi.md`](../formats/pochi.md) for what the slots actually contain.

### Concatenated sub-streams in a `scene_tmd_stream` entry

*Status:* falsified - one entry holds one stream; the "second sub-stream" is the
next PROT entry

The third shape of the same root cause, and the one that got furthest: here the
over-read did not misname an asset or misattribute a corruption, it invented a
**structural feature of the format**. `0006_town01` was read as two concatenated
`[chunk0 TMD][type-0x01 TIM chunks][terminator]` sub-streams - the second at
`0x14000` with its own leading TMD `0x2c20` and TIM chunks at `0x16c24` /
`0x1ee48`. Entry 0006 is exactly `0x14000` bytes, so all of that is PROT entry
**0007**, whose own leading TMD is `0x2c20` and whose own tail chunks are at
`0x2c24` / `0xae48` - the recorded offsets minus the length of entry 0006.

The plausible part was unusually good. The second block really did open on a
`0x800` boundary, really was preceded by zero padding, and really did carry a
valid Legaia TMD followed by two well-formed type-0x01 chunks - because that is
what a scene_tmd_stream entry looks like, and the next entry was one. The reading
even explained a real fact about the walker: `FUN_8001FE70` returns `param_1 + 1`,
just past the terminator, which was taken as the hook a sector-indexed caller
would use to walk the next sub-stream. That invited a follow-on hypothesis - an
unfound "multi-sub-stream caller" in the field/town dispatch - which was filed as
capture-blocked rather than as absent.

What makes it worth recording is how it was **confirmed**: the shape was checked
against the town0b and town0c clusters and reproduced exactly. Those clusters are
four-entry runs of the same layout (TMD bodies `0x383c` / `0x2c20` / `0x2998` /
`0x3af8`, two `0x8220` TIM chunks each), so every over-read spilled into a sibling
of the same shape. The replication was the artifact copying itself. Across the
corrected corpus, 0 of 182 `scene_tmd_stream` entries hold a second sub-stream and
0 yield a post-terminator chunk.

The transferable lesson: **a structural feature that only ever appears at the end
of a buffer is a claim about the reader's bounds until it is shown somewhere
else.** Replicating it across sibling entries does not test it when the siblings
share the layout that produces the artifact - a real second sub-stream would have
to appear somewhere that is not immediately before another entry of the same
class. `sub_streams` and `WalkSource::Continuation` survive in
[`scene_tmd_stream.rs`](../../crates/asset/src/scene_tmd_stream.rs) as regression
detectors for exactly this, with disc-gated coverage in
`crates/asset/tests/scene_tmd_stream_real.rs`.

See [`scene-bundles.md`](../formats/scene-bundles.md#one-entry-one-stream-the-falsified-two-list-shape)
for the corrected layout.

### "The backdrop shell is drawn once, so no completion exists"

*Status:* falsified - retail draws the shell **twice**. The authored half is
real; "therefore nothing completes it" does not follow

Open any `scene_tmd_stream` PROT entry in a mesh viewer and you get half a
bowl: a sky dome, a distant mountain ring and a far ground ring, all sheared
off along a plane through the origin. Two readings of that have now been
tried, and **both** were wrong.

The first was "a whole map got halved, so find the missing bytes". That one
stays falsified, and its measurements stand: the half shape is authored, and
nothing is dropped on the way in. Measured over object 0 all **182**
`scene_tmd_stream` entries put at most **8%** of the shell's X or Z extent on
the far side of `X = 0` / `Z = 0` (widest `0.079`, `0048_vell`); the open side
is `-X` in 129 entries, `-Z` in 49 and `+X` in 4, and never `+Z`, the side the
party is seated on. Every one of the 378 objects has
`vert_top + n_vert * 8 == normal_top` exactly and the parsed body accounts for
the whole declared chunk0 size. There is no unread vertex block, no second
primitive list, no second sub-stream.

The second reading was the inference drawn *from* that: since the file holds a
complete half and the runtime links one background actor, no completion exists
and drawing one is a regression. That is the claim this row now retracts.
`FUN_800513F0` registers the backdrop TMD **once** and allocates **two**
actors from the same descriptor, and the second carries a transform. What the
port had wrong was never *whether* to complete the shell - it was *how*, and
for which stage. Mechanism, evidence and the per-stage table:
[`battle.md`](../subsystems/battle.md#backdrop-shell---two-copies-of-one-mesh).

**Why the counterexample misled.** The engine once shipped a `Ry(180deg)`
duplicate, and for `town01` it planted a second village wall straight across
the open `-X` side - the side that in retail is open sea. That artifact was
real and correctly observed. But `town01` (stage id 4) is **on** the mirror
list: retail completes it by reflecting in the YZ plane, not by turning it
half around. `Ry(180deg)` is the right transform for `town01`'s siblings
`0006` / `0009` and the wrong one for `0007` / `0008`. A wrong transform on
one stage was read as evidence that no transform was wanted anywhere - the
experiment falsified the transform it tested, and the conclusion generalised
past it.

**Why the retail captures seemed to agree.** The same four-angle stage-battle
capture set was quoted as terminal: the distant mountains cover "44-81% of the
horizon columns, not a ring". Two separate things are wrong with that.

The number measured the wrong thing. Re-measured for *presence* of a mountain
band above the horizon, the four angles read **98 / 100 / 100 / 100%** of
columns. The 44-81% spread is what a band-*thickness* threshold of 9-18 px
produces, because the ring's height varies from a few pixels to ~45 across the
arc - and "a ring would hold roughly constant" was the premise that failed.

Then the corrected number settles it the other way. Project `map01`'s drawn
objects through the exact camera of each capture - yaw `_DAT_8007B792`, pitch
`32`, `TR = (0, 1280, 7680)`, `H = 256`, all read out of the save state - and
one copy covers 100 / **71.9** / 100 / 99.7% of the 320 columns against two
copies' 100% throughout. Three of the four yaws cannot separate the models;
one copy already fills the frame there, which is why a single-copy render
looks plausible if you happen to sample those angles. Capture **b**, at yaw
334.7deg, separates them: a single copy leaves columns `0..89` with no
mountain geometry, and retail has a mountain band in **90 of those 90**. The
capture set does not merely permit the second copy - it refutes its absence.

**What this is still not.** Not the `+0x10` mesh puzzle - the walk-visible
`.MAP` cells that name a pack mesh no layer draws. That family is `0x0011`,
and stamping it was separately falsified against retail: it draws a wall down
every river. (What keeps it out is the `0x2000` draw gate its cells never
carry, not the `FLAG_MESH_DRAWN` bit - see the world-map table above.) And
not the site's assembled map view or
the engine's field renderer - those exclude `scene_tmd_stream` entries
entirely and build the scene from the environment mesh pack plus the `.MAP`
placements (`crates/web-viewer/tests/field_scene_assembly.rs`).

**The durable lesson is about what a measurement is *of*.** Every number in
the falsified version was correct. The half-shell sweep measured the file and
said the file holds a half - true, and silent about the runtime. The capture
measurement counted columns above a thickness threshold and was quoted as
counting columns with mountains in them - two different questions with very
different answers on the same pixels. And the four captures were treated as
four samples of one question when three of them cannot answer it at all: at
those yaws both models predict a full frame, so only the fourth carried any
information. A statistic pooled over angles hid that. Before a capture
statistic closes a thread, state the reading it would have refuted, and check
that the samples can tell the two readings apart.

### "Field-pack" was never a format

*Falsified by disc bytes.*

The reading: magic `0x01059B84` opens a Legaia-specific bundle - a 97-entry
strict schema, a byte-identical ~91 KB template block shared by every carrier,
and the per-scene payload in a preamble ahead of packed TIMs/TMDs.

Why it looked right: the word occurs exactly once on the disc, which is what a
magic does; the table after it is regular; and two carriers really do share
tens of kilobytes byte-for-byte.

What is true: the word is a DATA_FIELD chunk header, `(TIM_LIST = 0x01) << 24 |
0x059B84`, and `0x059B84` is town01's payload length - it is unique because
every carrier's length differs. The "schema" is that chunk's `asset::pack`
table `[u32 count = 96][u32 word_offset[96]]` (`0x60` is the count), every
member a PSX TIM; the "byte sizes" of its clusters were word deltas; the shared
block is town01 and town0c carrying the same three leading Rim Elm atlases; and
the "preamble" was the superseded over-reading entry size reading the block's
prescript and asset table ahead of the real entry. A carrier sits at raw-TOC
`+4` of its CDNAME block, loaded by `FUN_800255B8` / `FUN_8002541C`. See
[`field-pack.md`](../formats/field-pack.md).

### The prescript's "per-scene secondary header" is the next entry

*Falsified by disc bytes.*

The reading: after a `scene_event_scripts` prescript, a small `(count,
descriptor[count])` table sits at the next `0x800` boundary, alternating
`(type, size)` and runtime-buffer offset pairs - a per-scene secondary header.

What is true: for all 101 carriers the **next PROT entry** begins with exactly
that table at its own offset 0 (87 `scene_asset_table`, 14 the count-4 form),
and for 99 of the 101 the first `0x800` boundary at or past the last record is
already at or past the entry's end - there are no bytes there to be a header.
The same shape as the `.PCH` "+0x800 prescript" and the pochi "stale TIM"
readings: the over-reading entry size appending the neighbour. See
[`scene-bundles.md`](../formats/scene-bundles.md#scene_event_scripts---prescript-only).

### PROT 0892: a 12 MB LZS container, or a truncated DATA_FIELD stream

*Falsified by disassembly and disc bytes.*

Two readings, one entry. The "12 MB container whose content is unpinned" was
the superseded `toc[p+5] - toc[p+3] + 4` span (5,977 sectors); the entry is 33
sectors, 67,584 bytes. The "truncated DATA_FIELD stream whose final chunk the
runtime continues by DMA" matched the detector's own criteria, but the three
"leading chunks" are an `asset::pack`'s header words (`2`, `3`, `0x208B`) read
as chunk headers, and the "over-large fourth header" is a word inside member
0's pixels. Retail walks it as a pack (`FUN_8002574C`, `0x8002581C..`). See
[`re-settled-threads.md`](re-settled-threads.md#text--fonts--dialog).

### PROT 1221 / 1222 have no loader, because no image names raw TOC `0x4C7` / `0x4C8`

*Falsified by disassembly.*

`0x4C8` is never a literal anywhere; the index is `s0 + 0x4C7` with a runtime
`s0` (`addiu a0,s0,0x4c7` at `0x801F6C3C` in PROT 0978). A literal-only sweep
is the wrong instrument for a computed index - the same failure shape as the
gp-relative blind spot. They are the dome's `int.tim` / `int2.tim` stills.

## Field / locomotion

| Thread | Verdict | Why |
|---|---|---|
| `4C 86` sites are talk records that install the mirror on an interact press | falsified (they install from the spawn prologue at scene entry) | Plausible: the records are player-raised and sit next to talk scripts, and the engine's own capture seated only one of three controllers. All ten sites run before the record's first `0x21` park; the engine's short count was a port defect - field channel steppers moved the channel list out while a script ran, so every cross-context id resolved to nothing. |
| A tile poke cannot cross the world map, and `kor5`'s P2[4] / P2[5] never spawn | falsified (both walk; the pokes landed during the movement lock) | Plausible: the pokes that failed were made while the player was locked, and a crossing under the lock is consumed, not deferred. Poking after the lock clears dispatched P2[4] at once, and `korb2 -> kor -> korout -> map03 -> doman` crossed by pokes alone. The `0x6C4` writer is P2[8], not P2[4] / P2[5]. |
| `0x8007C348 + 4*i` is a per-channel actor pointer table | falsified (a free-stack index, seven list sentinels and the player slot) | Plausible: index 7 lands on the player. `+0x00` is the free-stack top, `+0x04..+0x24` seven list sentinels written by `FUN_8001E1B4`, `+0x1C` the player (written by field MAIN_INIT at `0x801D6D7C`), `+0x28` on the 143-entry free stack. |
| Motion-VM ops `0x37` / `0x41` chase a target along one axis | falsified (an eight-direction compass walk) | Plausible: each moves the actor toward something and the port's one-axis reading produced plausible walks. The step comes from the compass table at `0x80073F14`. |
| `_DAT_1F800384` is a packed camera word | falsified (the scratchpad region box `x0, z0, x1, z1`) | Plausible: the camera path reads it every frame. PROT 0901 restamps its low two bytes at `0x9A0..0x9AC` on the world map, which a camera word would not survive. |
| Field-VM `4C 14` is seven bytes, like every other sub-op of its nibble | falsified (it is eight) | Plausible: outer nibble 1 advances `s8` by seven in its own prologue, so every arm under it looks fixed-width, and the disassembler decoded eight all along - the two never disagreed on a listing, because only the **executing** VM slid. The `0x14` arm at `0x801E0E80` reads `lbu a0,6(s6)` and exits `j 0x801E3624` with `addiu s8,s8,1` in the branch delay slot, so the eighth byte is consumed even when the source id resolves to nothing. Reading seven desynced 94 sites in six scenes from their first occurrence, and `0x08` is not an opcode the VM has an arm for. |
| `4C 87` registers a callback, and both it and `4C 9F` park the script until it fires | falsified (both retire, both advance) | Plausible: `FUN_8003CF40` takes an actor list and a handler VA, which is the shape of a registration API, and the port halted on the op for years without a carrier noticing. The routine walks the list and sets `+0x10 |= 8` on every match - a retire sweep that writes nothing else - and the shared exit `0x801E2DC4` carries `addiu s8,s8,2` in the call's **delay slot**, so the advance has already run. Fifteen scenes issue `4C 9F`, 140 times; a parked reading strands every one of them. |
| `4C 9F` and `4C 87` sweep the same handler | falsified (they sweep different ones) | Plausible: the two arms are the same five instructions and were written up on one line. The VA each forms is what differs - `0x801E2548` forms `LAB_801DA930`, the floor-height-ladder oscillator, and `0x801E2284` forms `0x801E5154`, the reflection controller's tick. Reading `4C 87` as a ladder retire attributes a scene's mirror teardown to an elevation LUT. |
| `FUN_801E573C` is a six-axis rotation setter, and its `+0x90` is the source | falsified (both ends were backwards) | Plausible: the spawner's argument order puts the executing context first and the resolved actor second, so a reader naming them from the call site gets source and destination the wrong way round, and six `s16` into consecutive halfwords reads as a transform. Which end is which is decided by the **tick**: `FUN_801E5154` loads `+0x90` into `a3` and `+0x94` into `a2`, reads `a2` and writes `a3`. So the named actor is the source, the executing script is the image, and the six words are the controller's mirror line and tracking rect. |
| The reflection callback has no spawner on the disc | falsified (the spawner is `FUN_801E573C`) | Plausible: a port blocker said so, and it had been checked - against `0x801F2950`, the descriptor's **handler word**, rather than against the descriptor base `0x801F2948`, which `0x801E5780` materialises with an ordinary `lui`/`addiu` pair. An address one word off a table base is the cheapest way to turn a live spawner into a dead one. |
| A context's `+0x10` player bit means the context carries the player's pose | falsified (it means the player raised the record) | Plausible: the bit is set on exactly the contexts a player-facing beat runs in, so treating it as "this context IS the player" reproduces the right answer everywhere a single actor is involved. Every shipped `4C 86` sits in a player-raised talk record naming `0xF8`, so the reading paired the player with themself and the reflection tick walked them onto the mirror line each frame - two cold spawns ended inside a wall. The destination is the executing channel's placement; the bit only decides when no channel executes. |
| An all-zero op-`0x34` sub-0 operand is a ramp target (fade to black) | falsified (it clears the effect) | Plausible: the arm's other operands are an RGB target, so all-zero reads as "target black", which is also what a fade-out looks like on screen. The arm stores zero to the live-actor cell instead (`sw zero,-0x49d4`) and leaves without spawning, so the beat ends rather than ramping. A white target under blend 2 also loses an eighth of its duration, which is why a capture reads 57 frames for the word `0x41`. |
| The renderers read an `effect_tint` ramp, so the fade's two representations have one live reader each | falsified (neither had a reader) | Plausible: a wiring blocker named the ramp as the representation the renderers consume, and a blocker is the one place a reader expects the wiring answer to be. Grepping the renderers for the ramp finds nothing: both representations were dead, which is a different repair from swapping one for the other. The measured beat is a `(kind, blend, packed)` triple, so the push is the surviving model. |
| A Rim Elm house door is a scene change | falsified (an intra-scene warp) | Plausible: the screen fades, the camera cuts and the player is somewhere else, which is what every scene change looks like. The scene name word does not move - `town0c` stays across the door - so a capture staged on one to answer a cross-scene question measures nothing about scene boundaries at all. |
| Flag `0x5D6` has no script writer | falsified (`koin4` `P1[15]` self-latches it) | Plausible: the script census, a native flag-helper sweep, the move-VM ext sub-ops and the motion-VM census all came back clean, and four negatives agreeing is usually an answer. The census walk stopped at the record's first `0x1F` text segment, and the raw scan looked for the bytes `D6 05` rather than `55 D6`. Behind the line sit `48` and `55 D6`, with a `P2[3]` twin. The **native** negative still stands; it was never the half in question. |
| The port's `FIELD_DEFAULT_VIEW_WINDOW` is the window retail stamps on scene entry | falsified (it is a later region's) | Plausible: the value was read off a live walk, so it is a real retail window - just not the entry one. A poll across a `map01` -> `town0c` door catches the entry stamp at `(-7, -6, 5, 7)`, 78 vsyncs after the scene word flips, with `(-8, -6, 6, 10)` arriving later as the player crosses into another region. A constant lifted from the middle of a walk is a sample, not a default. |
| Nothing in 84 images references `0x801D5C08` or `0x801D5D60` | falsified (both are template handler words) | Plausible: an all-forms reference sweep is exactly the instrument for this question, and it returned clean. It was run `--tables-only`, which drops hits classed as incidental code - and these two *are* data words, the `+0x08` handlers of the field-overlay templates at `0x801F227C` and `0x801F22AC`, spawned through `FUN_80020DE0` at `0x801D245C` / `0x801D2634` / `0x801D57C0` and `0x801D2760`. A negative from a filtered sweep is a negative about the filter. |
| teien's hedge-base cells are filled by an unpinned kind-2-cell draw channel | falsified (retail draws nothing there either) | Plausible: the hedge rows carry object-grid bit `0x0800` and not the `0x1000` ground-draw bit, the port's ground pass gates on `0x1000`, and the result is visible black under the hedges - so an unfound emitter keyed on `0x0800` explained the symptom exactly. A live `teien` field pass visits 1536 window cells and emits 370: every `0x1000` cell and none of the 42 `0x0800`-only cells. Only 8 of 84 images touch `*(0x1F8003EC)` at all, only PROT 0900 / 0901 hold a per-cell pass, and each one's `andi 0x800` reads an **object record's** `+0x12`, not a cell bit. |
| `FUN_801DA390` eases a camera yaw | falsified (a vertical offset) | Plausible: the routine is a two-input ease inside the camera controller, and a yaw is the camera quantity most often eased. `0x801DA3B4` reads `ctrl[+0x4A]`, `0x801DA3B8` reads the actor's `+0x16`, and `+0x16` is the **Y** of the position triple, so the target is a height difference. The port's `ease_camera_yaw` was renamed `ease_camera_offset`. |
| Actor `+0x16` is a heading / a facing / a footing byte | falsified (it is Y, and it always was) | Plausible: three separate pages each read one consumer of the field and named the quantity from that consumer's context, so the docs carried three incompatible readings of one halfword without ever contradicting themselves in one place. Nothing on the disc masks `+0x16` as an angle: 0 of 77 accesses in PROT 0897 are angle-masked, and SCUS's four masked accesses are `actor[+0x96]`. |
| The dance step markers are the gap in `field_actor_plan` | falsified (a different pool) | Plausible: the markers are per-cell field actors and `field_actor_plan` is the per-cell actor planner, so the missing marker draw looked like a missing plan row. The markers come from their own per-cell pool; wiring them into the planner would have drawn nothing. |
| `FUN_801D6058` handles a cutscene element | falsified (a field-overlay template, spawned once) | Plausible: it is reached from a spawn table and its `+0x1A` arm looks scene-scripted. It is the `+0x08` handler of the plain descriptor at `0x801F271C`, spawned exactly once by the field MAIN INIT `FUN_801D6704` at `0x801D6FD8` and gated on `_DAT_8007B8B8 == 0`. |
| `Insn::extended` carries the field VM's op-`0x43` sub-op | falsified (it is a cross-context target marker) | Plausible: the field is set on exactly the instructions that have a sub-op, so a census keyed on it returns a plausible-looking site list. The value is `0x80`, a marker meaning "this op targets another context"; the sub-op is `InsnInfo::ActorCtrl`. Counted properly the family has 311 sites across **ten** ending scenes, not the eight the docs listed. |
| The fishing bring-up rewrites the lure index | falsified (the rod index) | Plausible: two nearby routines each probe a small band of bag items and each rewrite a persistent index, so one description covered both. `FUN_801CF070` walks `0xA0 + _DAT_80084454` (rods); `FUN_801D712C` walks `0x9D..0x9F` and rewrites `_DAT_80084450` (lures). Two indices, two item bands. |
| The `.PCH` sidecar's `+N` fixup words are filled in at runtime | falsified (nothing writes them) | Plausible: the header is called a runtime-fixup header and the words are zero on disc, which is exactly what an unfilled fixup slot looks like. All 97 on-disc tables carry zero there and no writer exists, so the words are reserved, not deferred. |
| "~270 undumped field-overlay functions" (recomp dispatch-entry seed list) | falsified (not a function inventory gap) | [details ↓](#270-undumped-field-overlay-functions-recomp-dispatch-entry-seeds) |
| Rim Elm's reachable south-gate band force-walks the player through the wall | falsified (the record is five inert bytes) | [details ↓](#the-reachable-bands-record-force-walks-the-player-through-the-wall) |
| Scene-bundle type-6 descriptors are "all small placeholders" | falsified (12 are walker tables) | Plausible: the modal slot really is a 4-byte `count = 0` filler (85 of 97 bundles) and the three kingdom tables had been attributed to a "kingdom slot 5" special. But the 80/172/516-byte type-6 payloads (`garmel`, `dohaty`, the `geremi`/`rayman`/tunnel/`son`/`edson` family) parse as the same CLUT-walk table, installed identically for every bundle - the water/waterfall shimmer. The `rayman`-family carrier is the count-4 MAN-less table variant the strict detector rejects; resolve **by type byte** ([field-ambient-fx.md](../subsystems/field-ambient-fx.md#mechanism-1---the-scene-walker-table-bundle-type-6-slot)). |
| Move-VM loop op `0x19` "retires past itself (size 2), loops back to the saved PC" | falsified (both halves inverted by the C rendering) | Wrong against the raw arm (`80023070.txt` `0x800235DC` + the `0x80024150` epilogue): retail **loops while the decremented count has not underflowed**, retires on underflow with size **1**, and the loop-back lands at **saved + 2** (the epilogue adds `a2 = 2` after the PC store) - re-running the `0x18` itself would re-seed the counter forever. jou's 15-instance cycler fan-out is the disc witness. Sibling correction: ext `0x1E` returns size **4**, hidden behind a `func_0x801d4a3c()` label-call return ([move-vm-overlay-ext.md](../subsystems/move-vm-overlay-ext.md#self-modifying-bytecode-ops-0x04--0x1b--0x1e)). |
| Field-VM op `4C` nE sub-3 "syncs the resolved actor's position to the active camera" | falsified (copy direction inverted) | Plausible because the handler tail (`0x801E3178..0x801E31AC`) really does refresh the camera-scroll globals - but that tail is a player-ctx-only side path. The op body (`0x801E3108`) copies the operand-resolved actor's `+0x14/16/18` position and `+0x26` facing **into the executing ctx** - it is the seat primitive of every mid-visit crowd swap (dolk2 `P2[11]`'s eight `CC <crowd> E3 <day>` pairs). Reading the tail as the op's purpose inverted the semantics. See [script-vm.md](../subsystems/script-vm.md#mid-visit-npc-re-arrangement-beats-dolk2-market-swap--garmel-boss-staging). |
| Extraction-0874 §2 F-variant pixels are written by a pause-menu-path uploader (and then: are a parked wrap-scroll phase) | falsified twice | Plausible: 6/6 pause captures held the variant; then the 3 words equal row 273's content, reading as a +2-row scroll park. But the whole pause walk issues **zero** image transfers (DMA2 chain-walk + GP0 PIO hook) and plain field saves carry the variant - session-history correlation; and the strip is not shift-invariant while the wrap-scroll installer ops never fire across the s2→s3 flip window - the row-273 equality is frame-content coincidence. The real writer is the town01 opening record's one-shot `4C 60` face-frame stamp (settled - [details](re-settled-threads.md#field--locomotion)). |
| Field-VM op `0x43` sub-3..6 is a **sound** register ramp: four target values, a `ticks` duration and a `curve` | falsified on all four counts (it is a camera-register *zone* ramp) | [details below](#op-0x43-sub-36-as-a-timed-sound-register-ramp) |
| Prologue gold grade = per-node `+0x74`/`+0x78` depth-cue crush | falsified (grade is a palette-space collapse; the nodes carry no `IR0`) | Plausible because `FUN_8002735C` really does load per-node DPCS far colour + `IR0`, and the motion/move VMs carry op `0x0C` writers of those fields - but the opening never uses them: a live recomp capture reads node `+0x78` (`IR0`) = **0 on every node at every beat**, and the `opdeene` MAN motion section has no op `0x0C`. The real mechanism is a load-time CLUT/TMD palette collapse `L=max(r,g,b) -> (L, max(L-1,0), L>>1)` ([cutscene.md](../subsystems/cutscene.md#full-scene-sepia-grade-the-gold-prologue-look)); the far-field crush is that law seen through dark authored gouraud. |
| `FUN_801DD784` is a cinematic **letterbox** | falsified (it is the scene **shutter blackout**) | The tick eases two full-width bars in from the top and bottom of the screen, which is what a letterbox looks like for its first few frames. It does not stop: the bars meet in the middle and hold, and the template it ticks is `0x801F2858`, the same one the field VM installs for a scene-change blackout. A letterbox reading gives the bars a target height they do not have and leaves the middle of the screen drawn. |
| `0x801F27EC` is the fade family | falsified (it is one rung of the scene floor-height ladder) | The address ticks a small monotone value toward a target, which is the shape of every fade in this overlay, and it sits in the band the fade actors are allocated from. Its destination settles it: the tick writes `0x1F800314 + 0x48 + actor[+0x50] * 2` = `0x1F80035C + rung * 2`, the scene's 16-entry elevation LUT, so what oscillates is a floor height and not a brightness. Installed by field-VM op `0x4C` nibble-9 subs `0..2` via `FUN_801DDE34`; see [`script-vm-menuctrl.md`](../subsystems/script-vm-menuctrl.md#nibble-9-is-the-floor-height-ladder-not-a-fade). |
| `FUN_801CFF3C` is a second spawner for the bar template `0x801F2858` | falsified (it is `FUN_801DE754` printed `0xE818` low) | Two spawners for one template is a plausible shape - one for the script op, one for a scene-entry default - and the dump really does write `+0x54`/`+0x9E` and the operand triple exactly as the known spawner does. It *is* the known spawner: `0x801DE754 - 0x801CFF3C = 0xE818`, the field overlay's base-offset re-key, and the two dumps match instruction for instruction. The bar template has exactly one spawn site, field-VM op `43 0C`. |
| `0x801D44CC` is a per-dancer facing script | falsified (it is the step-marker mesh flipbook) | It is called once per dancer per frame and reads the dancer record, so a facing update is the obvious fit. What it actually indexes is the **marker** actor's `+0x50` - the `clip - 6` value the floor pass stamps when it spawns a step marker - and it selects a mesh row from that, i.e. it flips the marker's picture. Dancers are posed elsewhere. See [`minigame-dance.md`](../subsystems/minigame-dance.md#the-sprite-part-emit-dispatch). |
| `FUN_801D414C` runs on both dance edges and stages `other1` | falsified (one edge, and it stages nothing) | The routine sits between the hall's enter and leave paths and touches the same globals both do, so a shared enter/leave stager reads naturally. It runs on the **exit** edge only, and it is a *restore* - it puts back what entering the hall displaced rather than staging an asset. Nothing in it names `other1`. |
| `_DAT_8007B880` is the dance pad latch | falsified | The word changes every frame while a player is dancing, which is what a latched pad word does. The judge reads its input from the ordinary per-frame pad edge words; `0x8007B880` is written by the hall's own animation clock and read by nothing that resolves a note. Gating a note on it accepts and rejects the wrong beats. |
| `0x801D518C` holds the literal `other1`, so the dance hall is `other1` | falsified (it is a BSS **saved-caller** name slot; the venue is `other7`) | The cell really does hold an `otherN` string at the moment the dance overlay is resident, and reading a live cell is usually stronger evidence than a table. But the slot is where the entry path parks *whichever* venue name last passed through it, so it reads `other1` in a state that entered from elsewhere. The dance venue is `other7`, block base `0x4CC`. Lesson: a saved-caller cell is not a constant. |
| `FUN_80019D50` is a BGR555 cell-grid emitter | falsified (it is the **CLUT-cell HSV cycler**) | It walks a rectangular region of halfwords and rewrites each one, which is exactly a cell-grid emitter's inner loop, and the halfwords really are BGR555. They are palette entries, not pixels: the routine rotates hue/saturation/value over one CLUT cell block and pushes the result with a single `LoadImage` at `0x8001A030`. That one upload is the tell - a grid emitter would emit primitives, not upload a palette. Port `engine-core::clut_cell_fx`; see [`field-ambient-fx.md`](../subsystems/field-ambient-fx.md#the-clut-cell-hsv-cycler-the-pulsating-flesh). |
| The world-map controller calls `FUN_801D362C` (the move-VM `0x2F` extension dispatcher) directly | falsified (one reference disc-wide, and it is the move VM's own arm) | The world map animates through the same overlay-resident helpers the extension sub-ops wrap, so a direct call from the controller is a short and plausible path. There is exactly one reference to the address on the whole disc - SCUS `0x80023AE0`, inside the move VM's op-`0x2F` arm. Everything the world map gets from that dispatcher, it gets by running a move-VM script. See [`move-vm-overlay-ext.md`](../subsystems/move-vm-overlay-ext.md#one-caller-and-it-is-ported). |
| Actor `+0x8A` bit 0 enables the scripted motion VM | falsified (it suppresses it) | Plausible: a per-actor byte tested at the top of a VM tick reads as an enable, and the actors that move do carry a non-zero byte. The gate is a `beq` at `0x80038194`, so a **zero** byte runs the bytecode; the bit gates the player-engaged / actor-busy / off-map early returns at `0x8003819C..F4`. The port and the parser both had the polarity inverted, which makes every un-flagged actor inert instead of ordinary. |
| Motion op `0x06` echoes the pad | falsified (a home-relative one-tile wander) | Plausible: the arm reads a live input-shaped source and writes a direction, which is the shape of a follow-the-player op. It draws `rand() & 6` and steps within a box of signed 7-bit tile deltas taken from the actor's home at `+0x8C` / `+0x8D`, so it never leaves one tile of where it was placed. |
| Motion op `0x0C` is a glide channel | falsified (it fades a tint and a draw mode) | Plausible: the op schedules a two-word ramp on the actor, and the neighbouring ops really do ramp position. The words are `+0x74`, the packed RGB tint, and `+0x78`, the draw mode - scheduler kind 3. The same halfword the locomotion code reads as a speed multiplier is `+0x72`, one field away, which is what made a colour ramp read as movement. |
| The field player is seated by the cold-entry arm only at New Game | falsified (every ordinary scene change takes it) | Plausible: the arm is gated on a word that a fresh boot leaves zero, so the gate reads as a boot-time discriminator. `FUN_801D6704`'s own epilogue clears the word at `0x801D750C`, so the next scene entry finds it zero again. The seat `(0xA40, 0, 0xA40)` that the section quoted is not the player's either - it is the spawn of the one plain ambient template at `0x801F271C`; the player is seated on **both** arms, at `0x801D6F64` and `0x801D6F7C`, from the door operand. |
| Motion ops `0x0F`, `0x15` and the wander `0x06` have authored carriers | falsified (a census over every MAN finds none) | Plausible: the ops are decoded, dispatched and ported, and a decoded op with a table slot reads as a used op. Over all 573 MAN tail-section-1 variants, `0x0E` appears 215 times in four scenes and `0x13` and `0x16` in one scene each; `0x15` appears zero times, as do `0x00`, `0x06`, `0x0F`, `0x10`..`0x12`, `0x1A`..`0x1F` and `0x20`. A consumer gap for an op no disc data reaches is not a gap in the port. |
| `_DAT_8007BACC` has no writer, so the recentre-window arm is unreachable | falsified premise, surviving conclusion | Plausible: a five-form reference sweep returns nothing, and a global with no writer is the cleanest possible dead-code argument. There **is** a writer - `sw zero,-0x4534($v0)` at `0x801D6F50`, sitting in the delay slot of the `jal 0x800567A8` before it, which is exactly the position a call-target sweep steps over. The arm stays unreachable, but for a different reason: the only store is a **zero**. The page even named "the clear at `0x801D6F50`" two clauses after asserting there was no writer. |
| `FUN_801DBE9C`'s zone query is how retail picks a camera region | falsified (that leg is dev-only) | Plausible: the arrival actor really does run the query, really does load the hit and really does install the miss defaults, so following it gives a complete and self-consistent account of the camera. The query sits behind `_DAT_8007B868 != 0`, the dev/dual-mode gate, which is zero in retail. Retail's query is the script-driven `FUN_801DE3E0`, reached from three field-VM arms and the op-`0x45` LOAD - so a port built on the arrival actor has the right arithmetic on a path the game does not take. |
| `FUN_801DBC20` writes the live camera globals | falsified (it writes the parameter block) | Plausible: it is the routine a camera-region record is handed to, and the globals do change after it runs, so cause and effect line up on every observation. It writes `0x8007B607..0x8007B627` and nothing else. The live globals are written by the ease `FUN_801DB510` and the snap `FUN_801DB8EC`, off a staging descriptor a third routine composes - which is why a port that loaded the record and expected a pose got a block nothing had turned into one. |
| Nibble-3 sub-8 and sub-D are one helper with a sub-op discriminator | falsified (they call different routines) | Plausible: the two arms share the tile arithmetic exactly, and a port that routed both through one host hook keyed on the sub-op byte produced the right answer for sub-8 every time. Sub-8 is `FUN_801DE3E0`, the camera-zone query; sub-D is `FUN_800180EC`, the walk-region **attribute** refresh. One hook for both hid that the second one never touches the camera. |
| `[4C C4]` is a sub-tile broadcast | falsified (it is a camera-zone query at a named tile) | Plausible: the label was read off the caller's shape - two coordinate bytes going somewhere - rather than off the callee. The arm at `0x801E2878` calls `FUN_801DE3E0(x & 0x7F, z & 0x7F)`, the same query-and-load as nibble-3 sub-8, so a scene can frame a shot from a camera-region record the player is not standing in. |
| `[4C CF]` is a position broadcast | falsified (it is the script **camera-focus override**) | Plausible: the arm resolves each operand byte to a world coordinate, a tile centre or zero, which is exactly what a position broadcast would do, and nothing in the arm says where the two values go. They go to `_DAT_8007B628` / `_DAT_8007B62A`, and the focus clamp `FUN_801DAA50` stores their **negation** into the camera focus at `0x801DAB68` / `0x801DAB84` when non-zero - so zero is "no override", not a coordinate, which is why the arm clears both before writing either. |
| `_DAT_8007B628` / `_DAT_8007B62A` have no writer | falsified (six `sh` in one field-VM arm) | Plausible: an absolute-word reference scan really does return nothing for either address, and "no reference exists" is the verdict that scan is built to give. Both forms here are `lui`+`sh` pairs, the shape a word scan is structurally blind to; the `gp`-relative sweep finds all eight references, six stores in the `[4C CF]` arm and two `lh` reads in the clamp. |
| The port's compass residual is its pad quantisation | falsified (it is the scene's camera offset through pitch) | Plausible: the port did quantise the pad remap to four cardinals, zeroing the camera focus did move the failing figure, and a quantisation artefact is what a heading test usually catches. At orbit `0` the residual is a pure `+Z` world walk, which no heading quantisation can bend; what bends it is `town01`'s roughly 14.8-degree camera offset acting through the camera pitch. Wiring the 45-degree ring was worth doing and was not the fix. |
| Fifteen scenes raise the camera re-query flag | falsified (eight) | Plausible: the field-op census can count the decoder's masked flag bit, and fifteen scene MANs carry a word whose low five bits decode to `0x16`. Only eight carry the literal `[2E 16]` operand in a coherent record - `ropeway`, `station`, `tunnela`, `tunnelb`, `tunnelc`, `nilboa`, `nilboa2`, `noaru` - and the other seven all sit inside desynced records. Count operand bytes, not decoded bits. |
| `0x801EFE7C` is a third octant derivation on the tile board | falsified (a restore) | Plausible: it stores into `gp+0x2D8` like the walker's two other sites. `0x801EF320` saves the octant into `0x801F35C4` on board entry and `0x801EFE7C` puts it back on exit; the walker's real second band is cells `0x0B..0x0E`, whose delay-slot `sll v0,v1,1` at `0x801EF8C8` re-uses `v1 = cell - 3` and lands on octants 0/2/4/6 after the `& 7`. |
| A black frame from a name-hijacked scene load says the scene is dark | falsified (the entry path is black, not the scene) | Plausible: rewriting a door's inline name does load the named bundle - the load trace's VAB, BGM and packet count all move with it - so a frame taken afterwards looks like a frame of that scene. A hijack into `bylon`, a scene the game draws brightly, measures identically black over 1000 vsyncs (non-black fraction 0.005 vs 0.003, the player sprite). From a world-map door the frame proves nothing about the destination. |
| The `0x4C` outer dispatch bound is tested at `0x801E0C44` | falsified (one instruction later) | Plausible: the `srl v1,s3,4` that forms the nibble is at `0x801E0C44`, and a reader citing the arm's first instruction lands there. The `sltiu v0,v1,0x10` is at `0x801E0C48`, its `beqz` exit lands on the nibble-B error arm, and the table base `0x801CEE60` is the `lui`+`addiu` pair at `0x801E0C50`/`54`. |
| The two `0x4C` error arms share a preamble | falsified (a tail) | Plausible: both print through `jal 0x8001A068` with one string each, so one shared head looks like the natural shape. Each arm forms its own string pointer (`0x801CEC98` for nibble F, `0x801CECAC` for B) and F jumps into B's last three instructions at `0x801E3558`. No scene carries a coherent `4C Bx`; the 27 `4C FF` totals are all decode-desynced. |
| The fishing water class is read after the walk-grid probe reports the `0x4000` bit | falsified (two unrelated reads of the same point) | Plausible: both reads happen once per frame in the same tick, both take the lure's position, and `0x4000` appears in both - once as a cell-word bit and once as a buffer displacement. The water gate is bit `0x4000` of the `+0x8000` per-tile cell halfword (`andi v1,v1,0x4000` at `0x801D3374`), and its class word is rebuilt by `FUN_800180EC` at `0x801D3384`. `FUN_801D7030`'s probe over the `+0x4000` walk grid feeds the credit nothing: its hit drifts the lure's `x` accumulator by `frame_delta << 11`, signed by the low bit of the lifetime cast counter. |
| The Baka Fighter editor's actor record starts where its callback was cited from, or the band is five records from `0x801D7618` | falsified twice (eight, from `0x801D75DC`) | The first reading took a record's `0xFFFF0000` filler for its head; the pass that fixed the offset kept a band start which is itself the fourth record's `+0x0C`. Eight `0x18`-byte records tile `0x801D75DC..0x801D769C`, bounded below by an 8-byte-stride action-name string table and above by the `Vahn` roster pool, carrying `FUN_801CF388` / `801D3468` / `801D3390` / `801D6310` / `801D3F44` / `801D6F18` / `801D4FC8` / `801D49E8`. `FUN_80020DE0` spawns one per site: `0x801CF184` takes `0x801D75DC`, `0x801D01C4` `0x801D7624`. |
| `0x801D918E` is the fishing lure's z | falsified (it is the lure's **height**) | Plausible: the cell sits between the lure's x and a third halfword, and a tracked point written by a cast reads naturally as a planar pair plus a spare. The store is `sh a0, 2(a2)` at `0x801CFCEC`, and what it writes is the spawning actor's `+0x16` less `0x80` - a Y, not a Z. The lure's z is the halfword at `0x801D9190`, whose 24.8 copy is `0x801D917C`; a reader taking `+2` as z drives the lure along the wrong axis and reads the height as a coordinate. |
| The port's camera visible-tile window is seeded at scene entry | falsified (it was seeded once, at camera construction) | Plausible: the doc sentence said "seeded at scene entry, then replaced by whichever of the two writers the scene runs", and the two writers really do run. The seed did not. `Camera::new` stamped the default once, and every later write only overwrote it - so a scene that scripted a wide window handed it to the next scene's clamp, and only a scene that scripts one at all ever looked right. A claim about *when* a value is written is not testable by reading the writers that overwrite it. |
| `resolve_field_slide` cannot be wired because its rests are pinned on the non-sliding stepper | falsified (both pinned legs are slide-neutral) | Plausible, and it is the honest shape of a blocker: wiring a resolver that widens a held direction does move where a player rests against a wall, and there were wall-press oracles. At both pinned rest positions the resolver hands back exactly the held cardinal, so the oracle was measured where the two models agree - it was never asserting the defect. Retail's slide fires in ordinary free-roam, 15 widenings in 276 calls down one town wall ([settled](re-settled-threads.md#field--locomotion)). |
| The scene-authored octant word is read only inside the field overlay | falsified (its primary readers are in `SCUS_942.54`) | Plausible: the writer set is entirely field-overlay, the two known readers were too, and a reader list assembled from the same scan that found the writers is self-consistent. The SCUS pad remapper `func_0x800467E8` loads the word **twice** (`0x800467E8`, `0x80046840`), which is what makes the octant a rotation at all; the walker's restore at `0x801EFE7C` was missing from the writer list for the same reason. Both forms - `imm(gp)` and `lui`+load - are invisible to an absolute-word scan. |
| Retail's cold entry into `conc` runs `P0[34]` / `P0[36]`, whose body clears flag `0x6DE` | falsified (those are walk-on trigger scripts a cold entry never reaches) | Plausible: the engine's fresh entry executed them and the clear is in their bodies. A write watch across retail's own card-boot entry sees the clears come from `P1[1]`'s spawn prologue and the `P1[0]` entry script instead, and then `P1[0]`'s per-frame body re-SETs the flag every other frame from a player bounding-box test - so the state holding it set said where the party stood. |
| Seeding a system script's position anchor once at scene load is equivalent to reading the player each time | falsified (the script install resets it to the origin) | Plausible: the player does not move during a load, so one seed looks sufficient. The ctx-`0xFB` context's anchor was seeded only in three opening scenes' load-frame pre-run and reset to the origin by the script install everywhere else, so every per-frame `CD F8` box test disc-wide answered from tile `(-1, -1)`. Retail resolves `0xF8` to the live player on each evaluation. |
| The `juui1` walk is blocked by the pad ladder's budget | falsified (it is blocked by a record gate) | Plausible: a two-scene pad walk is long and encounters break it. `conc2` P2[20] and its spawners carry `C1 = [0x3E1]` / `C2 = [0x3E5]`, and the only card save that reaches `conc2` already holds `0x3E1` set, so no budget reaches the door; a tile poke crosses a door in about ninety vsyncs anyway. |
| `4C D8`'s model operand is a slot in the global TMD pool, and `balden`'s `99` / `100` / `109` / `110` are simply not in it | falsified (it is a **scene-bank** index) | Plausible: `FUN_801D77F4` reads `DAT_8007C018[slot]` with no adjustment. The op's arm has already added `*(u16 *)0x8007B6F8` at `0x801E2DE0..0x801E2DE8`, and read that way all seventeen blocks fit their mesh vertex for vertex. Read as raw slots, `jagaroom` / `garmel` had bound battle effect models on seven of eight sites. |
| The morph block is a fixed-pitch slab of records | falsified (retail walks it with three pitches) | Plausible: a record array is normally walked with one stride. The spawner's size sum steps `0xC`, its rest-pose copy `n_vert * 8`, and the apply pass `n_vert * 0x60`; they agree only when a block holds one record, which every shipped block does. |
| `FUN_8001FA00` seeds a cutscene sprite list | falsified (its one caller seeds the fog-particle pool's free stack) | Plausible: an identity index list under a top index is a generic free-list seed, and the name was picked from a neighbour. Its only `jal` is MAIN INIT's at `0x801D7384` (PROT 0897), passing `(pool, pool + 4, 0x50)`. |
| `0x801F21B4` is one twelve-row probe table whose later rows have no caller | falsified (three tables, each with a consumer) | Plausible: the 192 bytes share one row shape and the locomotion reads only the first four rows. Three consumers form three bases - the actor probes at `0x801F21B4`, the wall probes at `0x801F2214` and the facing compass at `0x801F2254` - and the "uncalled" rows were the second and third tables. |

### Op-0x43 sub-3..6 as a timed sound-register ramp

*Status:* falsified on all four counts - it is a **camera**-register ramp, the
byte operands are a tile rectangle, and the two halfwords are the ramp's
endpoints rather than a duration and a shape.

*The reading:* `FUN_8003C6A4` scales four byte operands `* 0x80 + 0x40` and
stores two trailing halfwords, which reads as "four targets plus timing"; the
destinations are unnamed `0x8007Bxxx` globals, and the op sits in a
sub-dispatcher whose neighbours include sound work.

*What settles it:* the tick. The descriptor at `&DAT_80074304` carries handler
`0x80037018` in its `+8` word, which `FUN_80020DE0` copies to the new actor's
`+0x0C`. That routine reads `+0x88`/`+0xC8` and `+0x8A`/`+0xCA` as an **AABB
over the player's position**, `+0x80`/`+0x84` as two endpoints, and `+0x8C` as
the destination *store width* (`1` = `sb`, `2` = `sh`, `3`/`4` = `sw`).
Nothing counts down - the register lerps on the player's Z as he crosses the
zone, and runs backwards when he walks back.

All four destinations are field camera-configuration registers, each read by
the field-overlay camera composer into the camera descriptor whose ten fields
are the retail camera globals: `B60C` pitch, `B610` yaw, `B614` eye-space Z,
`B618` GTE `H`. The `0x1B8` default of `_DAT_8007B60C` is the same pitch
`FUN_80025C24` seeds at field entry, which is what identifies the register.

*Why it survived:* the two halves were decoded years apart and each disclosed
the other as missing - the spawn port said "the per-frame interpolator is
untraced", the tick port said "the port has no spawner for this actor". Two
`NOT WIRED` notes pointing at each other read as two separate gaps.

Details: [script-vm.md](../subsystems/script-vm.md#0x43-sub-36---the-camera-register-zone-ramp),
[motion-vm.md](../subsystems/motion-vm.md#fun_80037018-is-not-a-slot-of-this-pool).

### The reachable band's record force-walks the player through the wall

*Status:* falsified - disassembling the record settled it in five bytes.

*The reading:* record 10 covers the only band a player can stand in, its
timeline ran for two frames and moved the player `+8` in `z` before ending, and
`+8` is one locomotion step - so the record must force-walk the player through
the sealed band and then run the `0x3F`, which would also explain how retail
crosses a wall that blocks ordinary locomotion.

*Why it is wrong:* record 10's entire body is `21 21 26 FE FF`. There is no
walk op and no `0x3F` in it. The two-frame termination is the choreography-wrap
rule doing its job on a `Nop`+`JmpRel`-to-self park, and the `+8` is one frame
of the player's own pad locomotion, observed only because the modal-timeline
install is what stopped it. The `0x3F` lives in record 0, on the *other* band,
and that band is opened by a collision paint rather than crossed by a scripted
walk - see
[`re-settled-threads.md` § Rim Elm's south gate](re-settled-threads.md#rim-elms-south-gate).

*The general lesson:* a timeline that "ends without doing the thing" is
evidence about the runner only if the record contains the thing. Disassemble
the record before theorising about the interpreter - five bytes settled a
thread that had already cost several diagnosis cycles on collision, grids,
standoffs and dispatch.

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

### A second stage-id writer "at `0x801FD514` in the 0897 band"

**Falsified:** the coordinate and the novelty, not the writer. The mid-battle
`_DAT_8007B64A` writer is real, but there is no routine at `0x801FD150` -
that printing was **already re-keyed** to `FUN_801E6968` (the Lost Grail
Final Heal sweep; the writer is its tail arm, store at `0x801E6D2C`) in
[`overlay-va-aliases.md`](overlay-va-aliases.md#0x801fd4c0) and
[`battle-action.md`](../subsystems/battle-action.md)'s shifted-alias table.
A lane still re-derived it from the base-tag-less
`overlay_0897_xxx_dat_801fd150.txt` dump, and reading the body at the
phantom coordinate turned the *settled* Lost Grail sweep (`0xE7` = the Lost
Grail item id, the accessory-slot consume) back into an unknown "`0xE7`
record scan" - a closed thread nearly re-opened under a new name.

The near-miss trap: the phantom dump is internally coherent - clean
prologue, sane guard logic, real SCUS `jal` targets (base-independent) - so
everything corroborates the wrong address *except* the overlay-local jumps
(`j 0x801E6A7C` / `j 0x801E6CE8` point `0x16000` below the "function"; at
the real base they are intra-function). The byte check settles it in one
step: the store's word pair `24020003 a082b64a` occurs in **no** PROT entry
but 0898, at file `0x18510` = `0x801E6D28` under the tagged base.

**Generalises to:** a dump without a base tag proves content, never
coordinates ([`dump-corpus-integrity.md`](../tooling/dump-corpus-integrity.md)).
Before naming a "new" function in an aliased band, grep the reference
indexes for the **re-keyed** address, not just the printed one - the alias
tables exist precisely so this lookup is one grep.

### `0x801D84B4` is inter-function padding

**Falsified:** that the VA is alignment `nop` in every overlay that maps it, and
therefore no routine at all.

The reading is right about four images and wrong about the one that mattered.
`0x801D84B4` really is padding in the fishing, dance, debug-menu and slot-machine
extractions - 17 consecutive `nop`, and 32 with one stray `sllv zero,zero,zero`
in the baka-fighter image - and the only dump that resolves an entry here is the
field overlay's, whose header reads `entry=801d8308`, i.e. interior. Both facts
are true and neither is about the field overlay's bytes.

Read `overlay_field_0897.bin` at base `0x801CE818` instead: `jr ra` at
`0x801D84AC` with `addiu sp,sp,0x20` in its delay slot closes the predecessor,
and a six-instruction leaf follows - store master game mode `_DAT_8007B83C = 0x16`
(22, CARD INIT), raise the entry-context word `_DAT_8007BB00 = 1`, `jr ra`. That
is the overlay-local twin of the SCUS scripted game-over trigger `FUN_8003C7EC`,
and the field image carries exactly one `jal 0x801D84B4`. Two base-tagged dumps
hold that seven-word body as well, both of them field-overlay captures, so the
padding reading was not even the only dump evidence available.

**Generalises to:** a padding verdict is per image, like every other containment
fact. Counting how many extractions agree does not make the disagreeing one
wrong - slot A holds a different overlay per game mode, so `nop` in four of them
says nothing about the fifth.

### `FUN_801dfb10` is a scripted player-turn state machine

**Falsified:** that a routine exists at `0x801DFB10` at all.

The address is a phantom of the `overlay_0897_xxx_dat` import's `+0xE818` base
error, and its bytes are field (0897) `0x801EE328` - the world-map `ON RULA`
travel-art actor, which is documented and ported under that VA. The printed VA is
interior in every image that covers it: the fall-through of
`bnez v0,0x801dfb28` in the battle overlay, a branch label in the field overlay,
and the delay slot of `jal 0x8003ce64` in the menu overlay.

What makes this one durable is that the *behaviour* attributed to the phantom is
accurate - the player-input lock, the per-frame `+0x16` angle rotation, the
story-flag `0xb` gate - because it was read off a correctly-decoded body. Only the
address is fiction, so nothing in the description looks wrong.

**Generalises to:** a plausible write-up is not evidence of a base. The same
routine is also printed at `0x801E8B10` by the `overlay_0896` batch at its
`+0x5818` delta, and two independent phantoms landing on one VA is the check that
pins it - see [`phantom-print-index.md`](../tooling/phantom-print-index.md).

### A scene with no `0x3F` in its MAN has no door

*Falsified twice over, by disassembly and by capture.*

The reading: the chapter-1 frontier ladder's clean per-partition walk finds no
`0x3F` in `uru` / `urudre1..3` and nothing at all in `jouine`, its tile sweep
fires no transition, and 160 executed record bodies reach no scene change - so
those five scenes are sealed and the port may treat them as one-way.

Why it looked right: three independent instruments agreed, and each one is
right about what it measured.

What is true: `uru`'s `0x3F` is in plain sight - `(2, "MAP03")`, upper-case,
which the port's lower-case-only label gate rejected; the others sit
`0x2DC` (`urudre1`) / `0x124C` (`urudre2`) / `0x2034` (`urudre3`) bytes into
record bodies past inline `0x1F` text the fall-through walk desyncs on, and
`jouine`'s FMV op at `0x1A8F`; the
tile sweep stops at 48 deduped gate-1 tiles while `uru` carries 118 with its
exit band at positions 63..66; and the 24-tick post-step budget cannot reach a
tail behind 300+ frames of explicit waits. `jouine` has no `0x3F` because its
exit is the FMV hand-off `4C E2 08`, already on the FMV dispatch table. All
five exits are carried by the scene's `.PCH` sidecar, and `uru`'s fired live.
Ask the `.PCH` before calling a scene sealed. See
[`re-settled-threads.md`](re-settled-threads.md#the-uru-mais-chain-and-jouine-exits).

### A frontier-ladder scene that will not walk is a scene or a seat problem

*Falsified by measurement.*

Twenty-eight consecutive scenes of the chapter-1 closure reported zero driven
tiles, and both engine changes that had just landed (the destination case
fold and the wait-discounting timeline cap) were the obvious suspects. Three
isolation builds gave byte-identical results with either reverted, and each
"broken" scene walked fine on a fresh host. What was latched was one
player-actor bit - `tower`'s ledge-hop steering lock, leaked across a scene
change - so the failure was ordered by closure position, not by scene. Before
blaming a scene, walk it first in the sweep order and alone. See
[`re-settled-threads.md`](re-settled-threads.md#field--locomotion).

### Field-VM op `0x45` sub `0xC0` returns the operand `s16` as the next PC

*Falsified by disassembly.*

The reading is in the decompiled C and was in `script-vm.md`'s own opcode row
("APPLY ... then absolute jump"). Retail's arm at `overlay_0897` `0x801DF210`
exits `j 0x801E3624` with `addiu s8, s8, 4` in the delay slot, and the `s16` at
`operand+1` goes to `FUN_801DE084` as the apply trigger - the identical call the
CONFIGURE arm makes with its `s16` at `operand+2`. The sibling arms corroborate
(`0x40` `+0x14`, `0x80` `+2`). Cost of the wrong reading: every record whose
trigger is `0` restarted from byte 0, which is what made `urudre2` read as
one-way and let four `keikoku` records do nothing.
It also had `man_edit` refuse to resize any record containing one
(`AbsoluteRef`) while listing it as relocatable - the field VM stores **no**
absolute PC anywhere, so every control-flow field shifts with its record.

### Op `0x43` subs 0/1/A/B resume at an operand `s16`

*Falsified by disassembly.*

The sub table at `0x801CEDA8` vectors subs 0/1/A/B to `0x801DF384`, whose
shared exit `0x801DF5B4` is `j 0x801E3624` / `addiu s8, s8, 8` (plus `+2` at
`0x801DF534` for `sub >= 0xA`). The three `s16`s are `FUN_801D25EC` arguments;
the sub-A/B `+7` is negated into the target's **Y** (`0x801DF524..0x801DF530`).
Instruction widths are 8 / 10 bytes (9 / 11 extended), not 5 / 9.

## No overlay function lives below `0x801CE818`

**Falsified:** "an undocumented address in the `0x801C0164`..`0x801CE000`
band is an overlay-resident function awaiting a doc entry."

The reading is plausible because the repository's own orientation says
overlay code lives "at `0x801C0000+`", and because dumps in that band
disassemble cleanly, carry function-shaped prologues and epilogues, and
are filed under `overlay_<label>_801c….txt` names. Nothing about them
looks wrong.

They are not functions. They are **printed addresses from imports based at
`0x801C0000`**, and the true VA of every one of them is `printed + delta`.
The structural argument needs no dump at all: every occupant of the
slot-A overlay window bases at `0x801CE818` and every slot-B occupant at
`0x801F69D8` (see
[`static-overlays.toml`](../../crates/asset/data/static-overlays.toml)),
so **no extracted overlay image contains any VA below `0x801CE818`**. An
address in that band cannot name a function in any overlay, whatever its
dump looks like. The measurement agrees: disassembling every extracted
image at its mapped base and asking which of those addresses is a `jal`
target, a `j` target or an instruction boundary returns nothing at all for
the whole band.

The deltas are the same ones
[`dump-corpus-integrity.md`](../tooling/dump-corpus-integrity.md)
tabulates - `+0xE818` into the field (0897) or menu (0899) overlay,
`+0xD018` into fishing (0972) through the 0971 over-read tail, `+0x9818`
into dance (0980), `+0x5818` for the `overlay_0896_*` family - and they
are constant per import, so the whole band resolves mechanically. Worked
examples: the "function at `0x801C6FEC`" is the fishing reel tug-of-war
`FUN_801D4004`; the "function at `0x801C56B4`" is the hooked-fish handler
`FUN_801D26CC`; the "function at `0x801C2704`" is menu-overlay
`FUN_801D0F1C`. Each is already documented under its real address.

The same failure extends **above** `0x801CE818`, where it is harder to
see because the printed address is then inside a real overlay's span and
so cannot be rejected on range alone. There the test that works is the
one above: resolve the dump's bytes to an image and offset, and
separately ask whether the printed VA is a `jr ra`-preceded boundary in
any image. A VA that is only ever a `j` or branch target is an
intra-function label, not a port site.

**Generalises to:** treat "the dump prints an address" as evidence about
the *import*, never about the game. The identity questions - which image,
which offset, is this a function at all - are answered from the extracted
image at its mapped base, and only from there.

## Menus / UI

| Thread | Verdict | Why |
|---|---|---|
| The play page draws the fishing point-exchange screen | falsified (the page opened, bought and closed it inside one call) | Plausible: the page composed `fishing_exchange_draws` every frame and a unit test drew it; nothing ever held the exchange open across a frame, so the compose always returned empty. Both hosts now drive it through `engine-core::fishing_exchange_input`. |
| Retail's shop quantity control is a nine-row list whose cursor **is** the quantity | falsified (it is a pair of pad steppers) | Plausible: the port's own shop screen is that list, and a screen read back off the port looks like a screen read off retail. `FUN_801DB7F4` and `FUN_801DBD94` step a single value at `DAT_801E46B4` by `+1` / `-1` / `+10` / `-10`, clamped, with the bound `min(gold / price, 99, 99 - held)`. While the list reading stood, the port capped every purchase at 9 - the list's own row count. |
| The retail item bag holds 72 slots | falsified (256, behind an active window) | Plausible: 72 is what a cheat database's item page enumerates, and a page of a UI is the easiest thing to mistake for a capacity. The array at `0x80085958` is 256 slots and every accessor checks the window pair `gp[+0x2D2]` / `gp[+0x2D4]`, which `FUN_8004313C` alone writes; a lone character sees one 128-slot half. A real three-member memory-card block carries items up to index 159, so the 72-slot bound dropped 88 of them on every lift. |
| Retail's op-`0x49` arm spawns a driver actor that **opens the pause menu itself** for the kind-`0x0D` entry context | falsified (row `0x0D` of the dispatch table is `-1`; nothing opens) | The reading explained why the port could not reach `ContextNotice` / `ContextReady` and pointed at a pending-request channel as the fix. But the submode dispatcher indexes a **signed** 14-byte table at `0x801F33A4` with the parked operand's first byte and returns on `-1` (`0x801F1468..0x801F1470`) *before* it writes the driver's state or clears `_DAT_8007B450`. So a `0x0D` park simply stands, and the player's own Start is what enters the menu it gates. The port's own close-tick fallback for that row was the defect: it retired within a few frames and took the context with it. |
| Actor VM = "the title screen's sprite-walk interpreter", with an ANM-trigger opcode | falsified (it is the menu overlay's window-widget script interpreter) | Two readings fell together. `FUN_801D6628` is resident in PROT 0899 (the menu overlay), and its base materialisation `lui 0x801e / addiu 0x4738` indexes the **window descriptor table** - instruction byte 1 is a window id, not a sprite-actor slot. And no arm of the 13-way dispatch hands off an ANM id (`see ghidra/scripts/funcs/overlay_menu_801d6628.txt`); "trigger animation" was a guess from the sprite-VM framing. Programs are overlay-resident data ([window-script.md](../formats/window-script.md)), so "find the per-scene carrier" was never answerable. |
| Op `0x36`'s request/acknowledge gate covers subs `0` / `2` / `3` | falsified (sub `3` is ungated, and sub `1` has a *different* gate) | The three subs are one protocol, so one gate over all of them is the tidy reading, and the C renders the arms in a shape that supports it. The instructions disagree per arm: sub `0` and sub `2` halt at PC unless `_DAT_8007BABC == _DAT_8007BAA0` (`0x801E0340`, `0x801E03A8`); sub `1` stores only when the pair is equal **or** the acknowledge cell reads the idle sentinel `-1` (`0x801E0374..0x801E037C`); sub `3` - the teardown `FUN_801D8450` - is ungated and yields the frame rather than falling through. A script that waits on the wrong arm deadlocks. See [`script-vm.md`](../subsystems/script-vm.md#overlay-0897-command--submenu-support-functions). |
| `_DAT_8007B868` only *skips* the bit-15-set arm of op `0x36` | falsified (it points the two halves in opposite directions) | Reading it as a single "disable" flag matches the first arm you meet: non-zero skips the whole bit-15-set sub-switch and advances the op (`bnez v0,0x801DF898` at `0x801E031C`). On the bit-15-**clear** arm the same word *bypasses* the equality test instead of adding one (`0x801E03E8..0x801E0410`), so it opens a path it closes elsewhere. Retail boots the word `0`, which is why the asymmetry never shows in a capture. |
| The shop buy-row layout is untraced, and `build_price_gated_rows` is the port of it | falsified (the layout is pinned, and the port was a different routine) | The engine had a row builder that dimmed unaffordable rows, which is a real retail rule, so it read as the port of the buy list. Retail's builder (case `0x0B`) does more: it splits the walked rows at `record_count - 3`, stages the rows **below** the split into `0x801C6220` tagged `0x3000`, writes the last three straight out tagged `0xA000` (ink 5), and appends the staged group afterwards - so the on-screen order is not the record order and the top strip is highlighted. The dim rule is `purse < price` **or** `held >= 99`. See [`shop.md`](../subsystems/shop.md#the-last-rows-come-first). |
| Menu sub-screen `0x02` is the save entry | falsified (`0x02` is the dev character editor; save is `0x19`) | The entry-context byte was read by position rather than by key, and `0x02` is what the sentinel row produces. The byte is keyed on the **record kind**: `0x00` shop -> `0x1A`, `0x01` save -> `0x19`, `0x07` casino -> `0x20`, `0x0D` -> `0x04`, and the sentinel `1` -> `0x02`, the debug character-parameter editor. See [`save-screen.md`](../subsystems/save-screen.md#debug-character-parameter-editor-fun_801d6e18). |
| The inline `0x1F` dialogue segment carries a geometry header | falsified (the `0x1F` is a MES line-start marker and nothing follows it but glyphs) | The port rendered only a segment's first line and the box geometry was unexplained, so an unparsed header in front of the text is the obvious missing piece. There is no header: the box's rect, pens and advance hand belong to the pager (`FUN_801D84D0`, row capacity `_DAT_801F2740 = 3`), and consecutive `0x1F` lines pack into one window. See [`field-menu.md`](../subsystems/field-menu.md#dialog-reading-box-fun_801d84d0). |
| PROT 0898 never calls `FUN_8002C69C`, so the post-battle report windows are not the nine-slice | falsified (the caller is in SCUS, one hop away) | A `jal` sweep of the battle overlay finds nothing, which is a real absence - the overlay does not call it. It does not have to: `FUN_80031D00` (SCUS) drives the window emitter with `jal 0x800323E4` off the **retained widget list**, every frame a battle is up, so the report chrome is the same nine-slice as every other window. A sweep scoped to one image cannot answer a question about a shared driver. See [`level-up.md`](../subsystems/level-up.md#fun_8002c69c-does-run-in-battle---the-jal-sweep-was-blind-to-its-caller). |
| The sparring prompt is an undecoded Yes/No box | falsified (it is the ordinary 4-option picker) | The prompt reads as binary on screen, so a dedicated two-way confirm is the natural guess and a bespoke undecoded widget the natural excuse. The script emits `3E FF <row>` - the standard option-picker sequence the rest of the field VM uses - so there is nothing new to decode, only the row indices to read. Its install coordinate is on [`encounter.md`](../formats/encounter.md). |
| The item bag has no writer outside the five SCUS helpers | falsified (the pause menu zeroes slots directly) | Plausible: the five helpers are the whole add / remove / query surface, they all respect the active window, and every path anyone had traced went through one. The pause menu's Throw Out confirm `FUN_801D8734` stores zero straight into the bag at `0x801D88FC` and `0x801D8910`. A port that models the bag as "whatever the five helpers did" therefore silently keeps a thrown-away item. |
| The item menu's Throw Out cursor is a display row | falsified (it is a bag slot) | Plausible: the cursor drives a list, lists index rows, and on an unholed bag the two are the same number - which is every bag a fixture builds. The list hides empty slots while the payload does not, so on a bag holed at slots 1/3/6 the cursor takes 0, 2, 4, 5, 7, 8 and a port removing by row throws away the wrong stack. |
| The Point Card applier is blocked on an unported counter | falsified (the counter was already there) | Plausible: the blocker was written when the counter was missing, and nothing re-reads a blocker once it is written. `World::minigames.point_card` is the `0x800845B4` bank. The row underneath it survives for a different reason - no item on the disc carries the class that reaches the arm. |
| The bag-row builders are missing their gate tables | falsified (both tables were parsed already) | Plausible: a builder that cannot answer "is this sellable" looks table-shaped, and two of the three really were missing one. Sell price is the item record's `+2` through `shop_catalog::ShopItemData` and the discard gate is the equipment record's `+7` bit 0 plus the item-effect not-discardable kind; wiring them surfaced a real defect underneath, a sell list drawn id-sorted while the commit walked slots. |
| The screen that opens windows 25 and 41 is the shop's equipment-buy recipient flow `FUN_801DB380` | falsified (two screens, and that one opens neither) | Plausible: the two windows draw the same shape of stat comparison, so one screen raising both is the economical reading, and the recipient flow does draw a compare panel. Window 25 is named by one open command only - the Equip screen's candidate step, sub-screen `0x14`, script `0x801E4DC8` - and window 41 by the shop-entry script `0x801E4E64`; the recipient sub-screen adds only window 36 over the set already up. While the merged reading stood, one host drew window 25 nowhere at all. |
| A host may pass the `0x40` no-passive sentinel and get the identical screen | falsified (true of the class-`1` arm only) | Plausible: every equipment bonus row on the disc really does carry `0x40` at `+5`, and the equip screen is where equipment is browsed - so the sentinel reproduces the screen on every id anyone checked. The category byte has two sources, and only one of them is that table: 151 of the 255 non-zero ids take the item-effect arm instead, 80 of them carrying a real passive index at `+3`. A host feeding the sentinel unconditionally loses the HP / MP and SPD / INT / AGL row sets entirely. |
| The equip browse row is the equip-byte index, with row `0` a Best-Equipment row | falsified (a two-table slot map; row `0` is the **weapon** row) | Plausible: the port's slot list is the equip-byte array in order, so row and byte index coincide there, and a leading row that is not one of the four gear slots reads as a convenience entry. Row `0` takes a per-character halfword from `0x8007B42C` (`2, 3, 2` - Vahn and Gala's weapon byte is `2`, Noa's `3`), and rows `1` and up index `0x801E43E8`, `00 01 00 04 05 06 07`. The order is weapon, helmet, body, footwear, three Goods - and the `slti v0, s0, 4` guard silences exactly the four gear rows ([settled](re-settled-threads.md#field--locomotion)). |
| The equip candidate list is already category-gated per slot | falsified (true of the armament half only) | Plausible: the armament rows really are masked - cases `0xE` / `0xF` / `0x10` read a per-character mask of their own - and a screen that gates one half of its rows reads as a screen that gates its rows. The three Goods rows go to different builder cases entirely (`0x1C` / `0x1D` / `0x1E`, selected by the eight-byte content-id table the browse step writes), and their filter is item class `2` plus an item-effect byte - no character term anywhere in it. A port copying the armament rule onto all seven slots offers the wrong candidates for three of them. |
| Window 35 shows a quantity x price line | falsified (it is quantity / **bound**) | Plausible: a shop quantity prompt that prints two numbers with a running total beneath is reading naturally as unit price times count, and both hosts printed `x{unit price}` off that reading. The second number is `DAT_801E46B8`, the purchase bound the picker's phase 0 fills with `min(gold / price, 99, 99 - held)`, and the glyph between the pair is a separator rather than a multiplication sign. The price appears once, in the total - and neither host drew the currency pictogram beside it ([settled](re-settled-threads.md#field--locomotion)). |
| Bytes `[7..10]` of the `0x801E43E8` run repeat the four gear-slot indices | falsified (a pad byte plus another table's first three entries) | Plausible, and it is a good observation about the bytes: `00 01 02 04` really does look like the gear indices again, which is exactly what a continued or longer array would look like. The run is seven bytes; `0x801E43EF` is alignment with no word, no `lui`/`addiu` pair and no branch anywhere in the corpus; `0x801E43F0` is a four-byte character equip mask and `0x801E43F4` eight halfwords of slot pictograms, each with three materialisation sites of its own. Three tables, not one or two ([settled](re-settled-threads.md#field--locomotion)). |
| Window `0x15` is the list-reorder screen | falsified (two id spaces; the screen is sub-screen `0x15`) | Plausible: one number, one menu system, and a scanner that reports both. Window `0x15` is the **Equip** screen's party window, opened by the slot-browse's own script `0x801E4DA0` at `0x801D9ACC`. The reorder page is **sub-screen** `0x15`, written into `DAT_801E46A4` by exactly one site disc-wide, `0x801D6C4C` in the root picker's row-3 arm. Searching for a bare id without saying which space it is in answers whichever question the corpus happens to hit first. |

### The save screen's block grid has a sixteenth Return cell

*Falsified by disassembly.*

The mode-`4` arm is real but unreachable - both cursor words are clamped
(`col <= 4`, `row <= 2`) and the linear seed's single writer stores
`col + row*5`. See [settled](re-settled-threads.md#the-dead-return-view-mode).

### `0x801E5AE8` is a shared armament placer that `FUN_801D71F0` calls

*Falsified by bytes.*

A dump mis-based by `0xE818` prints the body low while its `j` targets print
true, so a self-jump reads as an outbound call. `FUN_801D71F0` is the phantom
print of `FUN_801E5A08`, and `0x801E5AE8` is that routine's own inline placer.
The sibling reading "`FUN_801D71F0` is a dead add-item copy" was half right for
the wrong reason: the routine is dead, but it is the equip applier and its
`FUN_800421D4` call is a refund.

### The disc's item population is far below 128, so a half-window bag cannot fill

*Falsified by disc bytes.*

The static item-name table carries 250 non-empty names over its 256 ids. The
half-window OOB is still unreached in normal play, but the reason is progress
during the solo phase, not the size of the id space. See
[`re-settled-threads.md`](re-settled-threads.md#full-window-item-add-oob-reachability).

### Item-effect flag `0x40` is consumed by the item-info panel `FUN_801D0F1C`

*Falsified by disassembly.*

The panel does branch on a `0x40` right where the accessory-passive block is
chosen, and the five `0x40` subtypes really are the battle specials - so the
attribution read naturally. But `FUN_801D0F1C` contains no `andi 0x40` at
all: the instruction is `slti a0, 0x40` at `0x801D107C` / `0x801D1110`, a
magnitude test on the record's `+3` passive index against the no-passive
sentinel - a different field and not a mask. The bit's only readers are the
target-side forks at `0x801D18E0` (items) and `0x801D1C50` (spells) in PROT
0898. See
[`re-settled-threads.md`](re-settled-threads.md#battle--arts--level-up).

## Rendering / camera

| Thread | Verdict | Why |
|---|---|---|
| The page may skip "sky" meshes because they read as a wall from the follow camera | falsified (neither the native window nor retail filters them) | Plausible: the full-map viewer needed the filter. On the play page it removed 45 scenes' matching draws, among them the opening crater shell, so the Seru tableau stood in a navy void. |
| The page's yellow strips in the battle command phase are the billboard outline pass | falsified (the retained field ground heightfield) | Plausible: the outline is flat and bright. `FX_OUTLINE` was off with zero vertices in the frame; the strips were town01's walk ground drawn outside the draw list through the battle camera, which the native window never does. |
| `ScreenTintPush::kind` selects which of retail's screen-effect quads is pushed | falsified (`FUN_80024EE4` emits exactly one full-display quad; `kind` is the ordering-table bucket) | Plausible: the spawner passes three words and names the first `kind`, and a family of effect quads is what a screen-effect layer usually is. The routine builds one `POLY_F4` from the display rect and adds it at `OT + a0*4`; the second word is the ABR equation and the third a colour with red in the low byte. The name is the spawner argument's, not a style. |
| `FUN_80029888` zeroes the GTE light block | falsified (it writes the far-colour trio from registers) | Plausible: the routine is three back-to-back `ctc2` in a GTE setup path, and Ghidra renders each as a helper call with the control-register number buried in an argument, so "three control writes at the top of a transform" reads as a light-block clear. The three are cr21 / cr22 / cr23 - `RFC` / `GFC` / `BFC` - and the sources are `t4` / `t5` / `t6`, not zero. The routine that zeroes anything is `FUN_8003D190`, whose three `ctc2 zero` target cr5 / cr6 / cr7, the **translation** vector. The battle-intro swirl rolls about X and Z with it. |
| The field follow camera's pitch, yaw and height are scene-invariant constants | falsified (each is a per-scene, per-tile output) | Plausible: one save state pins all three, and a second scene often agrees, because neighbouring regions share a camera record. Over the walkable state population the pinned height matches 12 of 19 states, the pitch 8 and the yaw 1. Retail's arrival handler queries the MAN section-3 zone table and hands the hit's camera-region record to the config loader. Pinning a camera from one state measures that state, not the camera. |
| The koin4 coplanar sliver is a residue below the detection floor | falsified (the port's own repair pass makes it) | Plausible: the scene's other coplanar families clear, the remaining strip is tiny, and a sub-threshold residue is exactly what a per-family repair leaves behind. The strip is exactly one `DRAW_NUDGE` wide because the lift applied to that family, `[0, -0.75, -0.75]`, lies inside the second plane - zeroing the offset takes the measured overlap from 94.56 to 0. A repair pass is part of the instrument measuring the defect, so its own artifacts read as findings. |
| `FUN_801D629C` is a per-fog-particle actor | falsified (it is the spawner) | Plausible: it runs per frame, it reads the player's tile and it touches particle state, which is the whole shape of a per-particle update. It maps the tile to a MAN section-4 region record and pops one record from the pool at `_DAT_8007B7E0`; the per-particle work is SCUS's `FUN_8003F3FC`. The dump taken of it from the 0896 image compounds the error - that one is a fragment of a different routine, with no prologue. |
| The fog draw emits two GP0 **line** packets, command `0x9000000` | falsified (one textured quad; `0x09` is the packet's word count) | Plausible: `0x09000000` really is written into the packet's first word, and a value in a primitive's leading word reads as a GP0 command. That word is the ordering-table tag, whose top byte is the packet **length**: nine words, which is a `POLY_FT4` body. The command byte is `0x2E`, written by the caller at `0x8003F77C`, and the emitter stages four UV words into the packet's `+0x0C` / `+0x14` / `+0x1C` / `+0x24` slots - the four corners of one textured, semi-transparent sheet. |
| `_DAT_80089118` is the camera focus Z and `_DAT_80089120` the X | falsified (the other way round) | Plausible: both are written the same way by the same routine, both scroll by the same step, and neither name says which axis it is - so the pair can be read either way and nothing in the arithmetic objects. `0x80089118` takes the negated actor `+0x14` (X) and `0x80089120` the negated `+0x18` (Z), which makes the world-map top view scroll X with Left / Right and Z with Up / Down. Two pages carried the labels crossed while quoting those very stores four lines away. |
| `FUN_80026F50` is the field view builder | falsified (it is another mode's) | Plausible: it is the same five-call view-build shape over the same globals, and a static reader picking one of the two has no tiebreak. It folds the ROM-constant base matrix at `0x80010B84` (a 4x scale) rather than the live `0x8007BF10`, copies the eye trio as sign-extended **low halfwords**, and runs no focus `MVMVA` at all. It also fires zero times across three field runs of 719 vsyncs each, which is what identified the field one. |
| `FUN_80025C24` only zeroes the camera eye trio | falsified (it writes three different values) | Plausible: the first store really is `sw zero` at `0x80025C28`, and a routine that opens by zeroing a global usually carries on zeroing. The `addiu v0,v0,0x40b8` after it re-bases the next two stores, so the trio lands as `(0, -0x100, 0x4024)` with an angle trio `(0x1B8, 0x64, 0)`. Reading only the first store also loses the reason the entry seeds differ between the two writers. |
| The field view matrix is built once a frame | falsified (three times a vsync) | Plausible: one caller was found, one count was taken from it, and a per-frame view build is what a renderer normally does. Three callers enter it - two in the field overlay, one in SCUS - on 133 of 134 sampled vsyncs. The distinction matters because on a scene-entry frame two of the three read different live camera words, so "the frame's view matrix" is not yet a single object. |
| `FIELD_CAM_DEPTH` cannot be derived, only calibrated | falsified (it falls out of the scale) | Plausible: the constant was fitted to make one scene's framing match, fitting is what an unpinned constant invites, and the composed eye trio was not being fed to the view at the time. The eye trio *is* the eye-space translation, in GTE units the base matrix does not scale, so a renderer drawing at `1x` reproduces the frame by dividing the trio by that scale - the perspective divide is invariant under a uniform scale of the whole eye-space vector. |
| The dance count-in numerals are animated by `FUN_801D2D98` | falsified (that routine draws the banner; `FUN_801D3FD0` spawns the numerals) | Plausible: the count-in is one visual event, so one emitter is the natural reading, and `0x77` / `0x78` appear in the emitter's operands where widget ids would. Those two are **y seats**, not widget ids - the emitter clears `a2` at `0x801D2EBC` / `0x801D2EEC` / `0x801D2F04` - and `1 2 3`, `GO!` and `FINISH!` come from the sprite spawner instead. |
| The field view matrix is built three times every vsync | falsified (two or three times per **field frame**, and the first site is optional) | Plausible: a capture that counted one site's entries on 133 of 134 consecutive vsyncs reads as "every frame", and a renderer does rebuild its view every frame. Over 1800 captured vsyncs of a world-map-to-town entry only 749 carry a build at all, and of those 389 run the full three-site order against 313 that run only the last two; a static town scene splits 504 / 396 the same way. The builder runs per field frame, and a port pinning "three per vsync" is matching a number the run never produced. |
| The last build before the draw wins, so the trio's last reader is the frame's camera | falsified (the last build frames no geometry at all) | Plausible: the last write before a read is the one a read sees, which is true of the GTE registers and says nothing about when primitives were emitted. Ranking ordering-table links by the live build already put 16810 of 31046 under the first and 14 under the last; splitting by GPU command code sharpens it to the real figure, because the link count was mixing attribute packets and 2D rects in with polygons. Of 4289 polygons over three runs, 3861 are under the first build, 428 under the second and **none** under the last ([settled](re-settled-threads.md#rendering--camera)). |
| The builder's two extra call sites are in a resident field-render module | falsified (they are PROT 0901's world-map bracket) | Plausible: the capture recorded them returning into the slot-B window during a run whose scene name looked like a field one, and slot B does host render code. Exactly one of the statically extracted overlay images holds `jal 0x800172C0` inside that window - PROT 0901, the world-map render module - and the run was a world-map one (`map01`, mode `0x03`). Both sites are one bracket in `FUN_801F73E4`: save the yaw word `_DAT_8007B792`, zero it in the first call's delay slot, draw one screen-fixed band, restore and rebuild. Neither is a frame's camera, and a field scene entry has at most the three field sites. |
| Each landmark TMD passes once per frame through `FUN_8002735C` | falsified (the near arm takes every one) | Plausible: the renderer is the one that walks the per-mode descriptor table, landmarks are the meshes that table describes, and the dispatcher's case 5 does name it. Case 5 names it behind a gate. On `map03` the `+0x42` test fires 756 times and takes the near arm every time, so no landmark reaches it; a census over 720 vsyncs of four states records zero entries against 10621 into the per-prim leaf ([settled](re-settled-threads.md#rendering--camera)). |
| `FUN_80029888` is reached whenever `actor[+0x7A] != 0` on the overworld | falsified (0 of 5089 gate hits) | Plausible: `+0x7A` really is the choice between the two near-arm leaves, so a reader who has found that test has found a true statement about the inner choice. It is the **inner** one: nothing reaches either near-arm leaf's env-mapped half unless the outer `+0x42` gate is non-zero, and across four sampled states every one of 5089 gate reads was zero. A per-actor flag nothing in the sampled corpus raises makes both of its arms unreachable, not just the far one. |
| Nothing on the disc raises `actor[+0x42]`, the mesh-renderer gate | falsified (two writer families) | Plausible, and the measurement behind it is sound: 5089 gate reads across four states and three game modes, zero non-zero. It is a statement about the *sample*. The allocator `FUN_80020DE0` writes `2` at `0x80020EC0` when the world-map dev counter has bit 1 up, and move-VM op `0x10` writes its own `u16` operand into the field at `0x8002342C` - shipped programs issue it non-zero. What survives is narrower and still useful: across the sampled modes no **drawn** actor had the bit up ([settled](re-settled-threads.md#rendering--camera)). |
| `FX_OUTLINE` draws the page's yellow strips | falsified (it is off on both hosts) | Plausible: the strips are flat untextured quads, which is the outline pass's look. `play_battle_fx.rs` sets it `false` and the native pass sits behind `LEGAIA_DIAG_FX`, so the strips have another producer. |
| The engine's `fade::FadeState` is the retail fade ramp | falsified (the two disagreed on a ramp's last frames) | Plausible: both model the one `+0x7C` block. The engine latched on the target; retail keeps accumulating and clamps on the delta's sign, a delay of `n` suppresses `n - 1` frames, and a hold of `0` retires the frame after the ramp lands. A `REPLACED-BY` marker would have hidden that. |
| Actor render mode `4` (`+0x56`) is set from an asset, so a census of carriers finds its users | falsified (every write of `4` is code) | Plausible: other render-mode values arrive in data. The four stores are move-VM arms `0x80023460` / `0x800237E4` / `0x80023F98` and `0x8004D574`; no carrier holds the value. |

## Measurement readings

Falsified claims about the *instruments*, not about the game. They belong here
for the same reason the rest do: each was a plausible reading, each was believed,
and each shaped what work looked worth doing.

| Thread | Verdict | Why |
|---|---|---|
| A reference sweep may pair a `lui` with any later memory access until the register is reloaded by another `lui` | falsified (6,015 of 23,200 pairs were false) | Plausible: it is how the idiom usually reads. A register overwritten by an unrelated instruction in between still paired, so `lui v1; lw v1, ..(v1); lbu v1, 0(v1)` reported a reference to the `lui` page. The strict walk drops the register on any write. |
| PROT 0897 / 0899 hold 23 / 44 indexed-form accesses the byte account cannot see | falsified (they hold none) | Plausible: a Python scan reported them. It was the lax pairing above. |
| `FUN_801D84B4` dumped from PROT 0972 / 0976 is code | falsified (padding followed by data) | Plausible: a dump exists under that label and the byte account credited it by name - 22 KB across the two images. It has no prologue and no `jr ra`; the attribution CSV already called the window `zero_window`. |
| `0x801C9688` / `0x801C2B2C` are PROT 0897 relocation copies of the world-map emitters | falsified (they are `FUN_801D7EA0` / `FUN_801D1344` printed `0xE818` low) | Plausible: the bytes match the emitters exactly, which is what a copy looks like. They are one routine each, printed under a mis-based dump. |
| `0x801F90DC` is a Baka Fighter item-acquisition caption | falsified (a mis-based print of the menu overlay's item-info panel `FUN_801D0F1C`) | Plausible: the operands checked out, and the waiver that let the tag stand checked only the operands. Every attribution row for that VA is `misbased`. |
| PROT 0929's run has no spawn site and slot-B `$a2` records are always formed directly | falsified (a `lui` above a branch with the `addiu` in the call's delay slot; also register copies and switch-arm delay slots) | Plausible: the layout walk found every other record by a direct `lui` / `addiu` pair into `$a2`. |
| The fog rows need a composing ladder that does not exist | falsified (it existed, green and unlisted) | Plausible: the rows sat unconverted through several exports, and "no ladder composes this" is the usual reason. The ladder was already in the tree and already green; it was simply not in the export's list, so every reader of the list concluded a fixture was owed. Check the fixture directory before costing a new fixture. |
| A composed-overworld ladder would convert the three field effect handlers | falsified (none of the three has a production constructor) | Plausible: the three ticks look like ordinary field effects, so composing the scene they belong to reads as the missing step. No production path constructs an actor on any of them - the gap is a **constructor**, not a ladder - and all three carry a port tag that does not disclose it, which is why the rows read as merely unentered. |
| `sell_quantity_draws_for` has no native call site | falsified (it has one) | Plausible: a grep was run and reported nothing. It was a `| head` pipe truncated before the site, and a truncated search reads exactly like an empty one - the failure the shell-observer traps page catalogues, in its quietest form. |
| The page never replays a camera snap, so the snap beats are a page gap | falsified (the feature was dead on **both** hosts) | Plausible: a side-by-side read found the beats on one host and not the other, which is the shape of every real drift row. The native arm watched `pending_field_events` after the camera had already drained the configure events, so it never saw one either. A one-host read reports the host it looked at second. |
| The Muscle Dome hub's title art is drawn only by the page | falsified (both hosts draw it) | Plausible: the constant lives in a page-facing module and the native hub was known to be thinner, so "page-only" fit. Both hosts reach the same builder; what differed was which frames each drew it on. |
| The minigames page gates the catch HUD on a phase the engine does not have | falsified (retail gates on one word, and the port matches) | Plausible: the page had an extra predicate the native path lacked, and an extra predicate is usually the drift. Retail gates the depth readout and the tension bar on the single word `DAT_801d91b4`, set at the hook; the page's extra gate is its own idle phase, which is a session-model difference rather than a fidelity one. |
| A reach-triage cell that says a row is not driven is a measurement | falsified (it is prose next to an address) | Plausible: the page's rows carry addresses and a checker runs over the page, so the cells look audited. The audit resolves the addresses a row cites and says nothing about the sentence beside them, so a row stays worded as open after a canonical ladder converts it - nine of eleven "content not driven" rows were already entered. Re-measure a reach cell before planning work off it. |
| A mode-seat parity check can compare the two hosts' mode words | falsified (an INIT mode never survives the call that enters it) | Plausible: both hosts expose a mode word, and comparing two words is what parity usually means. The seat's entry call resolves an INIT mode and hands off to RUN before returning, so no post-call sampler on either host can ever observe one; and the word one host exposed was a front-end flag rather than the mode index. The witness that works is the edge **count**. |
| The two 24-instruction tally helpers at `0x801D14B0` and `0x801D6710` are different routines | falsified (one routine, linked into two overlays) | Plausible: a byte comparison of the two extents reports nothing in common, which is about as strong a "different" signal as a corpus gives. The relocated `lui`/`lw` pair that materialises the gate word comes **first** - `0x801D1AB4` in PROT 0977, `0x801DBF00` in the Baka Fighter image - so the comparison diverges on instruction one and never recovers. The other 22 instructions are identical. |
| PROT 0900 ships the same ground emitter twice | falsified (a depth-cued / flat **pair**) | Plausible: the two bodies are near-identical for hundreds of bytes, sit in one image, and are chosen between by a single global - every sign of a duplicate left in by a build. The difference is two instructions: `FUN_801F69EC` runs `GTE.dpcs` at `0x801F6C44` and stores the depth-cued colour register (`swc2 $22,4($t5)`, `0x801F6C4C`), where `FUN_801F6D48` stores a plain colour word (`sw $s2,4($t5)`, `0x801F6F88`). The selector's non-zero arm calls `SetFarColor` first, which is the tell. "Near-identical" is not "identical", and the diff is where the feature is. |
| A slot-B module carries un-dumped code above its frame partition | falsified (it is the spawn-record band) | Plausible: the uncovered run scores as plausible MIPS, because a record's move-VM bytecode contains a `lui $rt,0x80xx` word by accident and the `in_data_segment` test rejects any run that does. It is `[i16 model_sel][u16 reserved][bytecode]` records, addressed by the consumer's own `lui`/`addiu` and handed to `FUN_80021B04` / `FUN_80050ED4` in `$a2`; 62 of 64 images carry one. |
| PROT 0943 and 0961 hold a second region of *their own* code above the record band | falsified (it is the donor's residue) | Plausible, and it was this page's own correction of an earlier row - which is what makes it worth keeping. A dump really does print framed bodies at `0943 +0x135C..+0x17E0` and `0961 +0x1C60..+0x1D90`, above each image's records, and interleaving code and data is an ordinary thing to build. But 0943 is byte-identical to 0942 from file `+0x1037` up, so its own content ends there and those bodies are 0942's; 0961 ends at `+0x1918` the same way, donor 0960. A dump printing at an address under two images' names is not evidence of which owns it. |
| `0x801F99D8` is where a slot-B module's content ends | falsified (it is `base + 0x3000`) | Plausible: several images stop having recognisable content there, and one constant covering several images looks like a structural boundary. It is simply the end of the four 12 KB images in the band; images of other sizes end elsewhere. |
| `0x801C4BEC` is the libcd directory-entry cache | falsified (`0x801CB408`) | Plausible: the address appears in dumps and in the docs, and it is in the right region for overlay-adjacent scratch. It is the *offset* half of a `lui at,0x801d` / `sw ...,-0x4bf8(at)` pair pasted into the high half of an address; `FUN_8005DEA0` forms no address in `0x801C4***` at all. The wrong address outlived the correction in two other files' reason strings. |
| `engine-core::dialog` ports `FUN_8001FD44` | falsified (it implements nothing of it) | Plausible: the tag was written by someone who knew the dialog code and the address is a real routine in a related area, and no gate has ever checked the pairing. `FUN_8001FD44` is the name-based scene-change packet; the code that implements it is the field VM's op-`0x3F` arm. This is the project's mis-provenance class: a `// PORT:` tag naming the wrong routine passes every check that exists. |
| A PCSX-Redux `.sstate` carries main RAM only | falsified (it also carries a 64 KiB hardware blob) | Plausible: the repo's own reader exposes main RAM and nothing else, so the reader's surface was read as the format's. The blob holds the scratchpad at its own offset 0 (file `0x01080034` in one measured state), which means the per-cell scratchpad joins a mednafen state answers are also answerable in PCSX-Redux - once `legaia_pcsxr` grows the accessor. |
| The port-tag checkers read a tag's continuation lines | falsified (they read only the opening line) | Plausible: the tag format documents a wrapped list, so the readers were assumed to implement it. Neither did. The correct rule is narrow - a following comment line continues the list only when the text so far ends in a separator and the line starts with an address token - and it changes 0 of 879 addresses today, while the naive "read to the first blank line" fix would add 47 addresses nobody claimed. |
| The dump-extent attribution CSV lags the corpus | falsified (it does not) | Plausible: attribution lag is a real and documented failure of the neighbouring instrument, so the same explanation was reached for this one. The CSV was current; the number that looked like lag came from somewhere else. |
| The disc-coverage report's excluded dumps are "typically the ones that report `0 instructions` and hold only decompiled C" | falsified (zero of them reported `0 instructions`) | The files that *do* report `0 instructions` were passing the header regex and being credited a byte each. Of the excluded set, three were C-only and four fifths were not dumps at all - pointer stubs, recorded negatives, data windows, analysis output. The count was real; the sentence attached to it had never been checked against the files. |
| The inner of two nested overlay spans "cannot be repaired ... no amount of dumping moves it" | falsified | Address ambiguity really is total for the inner span - every extent in it falls in both by construction. Byte attribution then places most of those extents in one image or the other, and the row reports. The **starting point of a measurement was mistaken for its limit**, and the structural-sounding argument made it read as settled. |
| The unattributable residue "is repaired by re-dumping, not by extracting another overlay" | falsified | Re-dumping repairs almost none of it. What remains is windows a few instructions long that no image reproduces at that VA, bytes in no extracted image at any VA (which needs an *extraction*), and extents where two dumps genuinely disagree (which is an answer). The residue had been described from its class names rather than counted from the artifact. |
| `0x8005BA38` is "not a function - the dump reports `size=1 bytes, 0 instructions`" | falsified (it is a complete `RotTransPers`) | The dump was empty when the row was written and is 11 instructions now: load `VXY0`/`VZ0`, `RTPS`, store `SXY2` / `IR0` / the GTE `FLAG` word, return `SZ3 >> 2`. Nothing re-reads a caveat when its dump improves, so **a claim quoting a dump statistic decays silently** while reading as evidence-backed. Sibling instances: a "truncated dump" at 752 bytes that is 1528 today (three things left unported on it), and `0x8003D38C`'s ignore row, whose *verdict* survives - it is one instruction past the real entry `0x8003D388` - but whose stated evidence did not. Checker: `scripts/ghidra-analysis/check-dump-stat-drift.py`. |
| "About 2 % of retail camera beats set a non-zero roll" | falsified (the figure was the scan's own filter) | The number came from a **byte scan** - decode an op-`0x45` CONFIGURE at every offset of every scene MAN - which finds 4257 "sites" where control flow reaches 371. Because junk sites set a junk roll almost every time, the scan applied a post-hoc "credible" filter and then measured roll over the survivors, so the ratio is a property of that filter. Its sibling strict linear sweep reported the opposite (zero non-zero rolls) by reaching 21 sites and none of the eight real ones. The answer - retail *does* roll, in eight scenes - came from executing the records, not decoding them: [`re-settled-threads.md`](re-settled-threads.md#does-any-retail-shot-author-a-non-zero-camera-roll). |
| An over-strict header regex is one instrument's bug | falsified (every instrument had its own) | Each tool over the dump corpus carried a private header regex, and the corpus spells all four header fields several ways, so each silently rejected a different subset of **real dumps** and reported them as a corpus deficiency. Fixed by one shared parser; see [`dump-corpus-integrity.md`](../tooling/dump-corpus-integrity.md#not-every-file-in-funcs-is-a-dump). |
| A zero-reference SCUS function is a safe code cave once the five-form address scan and hours of live probing clear it | falsified for `FUN_800605C8` (boot-live) | The libapi VBlank-tier slot has no static reference of any form and every live probe ran clean with it overwritten - yet a **cold boot** parks at boot mode `0x10` the moment its body changes, because the kernel/libapi init invokes it before any save state's world exists. Save-state probes structurally cannot exercise boot, so "unreferenced + probe-verified" still has a boot-shaped hole; a claimed cave must also pass a cold-boot watch (`scripts/pcsx-redux/autorun_boot_watch.lua`, bisect via disc variants). Neighbouring CD-arm caves `FUN_8003EDAC` / `FUN_8003F210` passed the same cold-boot test. |
| PROT 0977 `0x801D1EF0` is data past the last `jr ra`, carrying no function | falsified (it is the arena settlement bring-up) | A sector-granular PROT extent truncates the body, so a missing `jr ra` is not evidence of data - the tail still carries RAM-page `lui`s and calls `0x8006BCB4` / `0x80026018` / `0x80024EE4`. Same shape in PROT 0902 (`0x801CED68`, a third function) and PROT 0979. |
| SCUS `0x80045CB4` is interior to a body 11 128 bytes back and nothing references it | falsified (entry `FUN_80045BB4`, 256 bytes back, referenced) | Its address is word 12 of the bank-3 primitive-handler table at `0x8007668C` (kinds 8..19); the routine is a 1272-byte frameless GTE emitter ending in `j 0x80045E54`. The table reproduces from the SCUS bytes. |
| `overlay_0897_xxx_dat_801f138c.txt` is a routine at `0x801F138C` | falsified (it is `FUN_801DABA4` printed `0x167E8` too high) | Its absolute `j 0x801db0f0` / `j 0x801db0f8` targets give the true base, and an instruction-by-instruction diff against `overlay_battle_action_801daba4.txt` differs only in the 33 PC-relative branch operands. Reading it as a second monster-record `+0x1C` consumer double-counts one routine. |
| The highest spawn record's end is unbounded, because a move-VM walk lands within 4 bytes of it in only about three quarters of images | falsified (the near-miss was the missing alignment step) | Plausible: a rule that is right most of the time and off by a few bytes the rest reads as an approximation of a rule that does not exist. The records are word-aligned, so the walk's end rounds up to 4 - after which 991 of 1027 extents are exact and 58 of 62 tops are bounded. A residual measured before the last step of the rule is a measurement of the missing step. |
| A build-buffer tail can only come from an image sharing this one's load base, and only from a longer one | falsified (both restrictions) | Plausible: a donor at the same base makes the printed addresses line up, and a shorter donor cannot reach a longer image's end - both feel like preconditions rather than assumptions. The mastering buffer is indexed by **file offset**, so five cast-band images end in the menu overlay's code and the game-over image ends in the world-map renderer's; and `content_bytes` is a sector extent, so a donor of nominally equal length still supplies bytes. The cross-base half was already written down when the instrument was built, and the instrument was never updated to it. |
| An uncovered run in an overlay image is a code gap | falsified (the same measurement had already classified most of them) | Plausible: a run of bytes no dump covers is the definition of a gap, and ranking runs by size is the obvious worklist. The instrument also shape-classifies each run - `no_exit`, `no_boundary`, `constant_table`, `return_tail`, `psyq_lib_stamp` - and then counted every byte of every run anyway, so the ranked worklist was mostly its own rejected shapes. |
| An image's frame partition bounds its own content | falsified (a partition can hold a donor's whole function) | Plausible: frame matching recovers real function extents, and a function that starts and ends inside this image's bytes looks like this image's function. PROT 0949's partition holds `0x801F8504`, which is 0948's stager sitting in 0949's inherited tail. What bounds own content is the record chain's top, not the highest frame. |
| A VA that appears in two neighbouring images names one routine measured twice | falsified (compare the bytes) | Plausible: the band's images share a load base and a build buffer, so the same VA printing in two of them is usually residue - and usually it is. `0x801F81DC` is **two routines**: a 272-byte stager in PROT 0951 and a 2040-byte applier in PROT 0910, differing at the first instruction. Six other "also in" cells around it really are residue, verified pair by pair. The rule is to diff the bytes, not to assume either way. |
| Correcting a function's extent in the attribution CSV corrects the corpus | falsified (it orphans the body) | Plausible: the CSV is where extents are read from, so editing it is where a wrong extent gets fixed. The extent is produced by the shared dump header parser and regenerated from it; a CSV-only edit leaves the dump still claiming the short extent, so the whole body becomes VA-ambiguous between the two lengths. Fix the parser, then regenerate. |
| A decompiled function's dump covers its body | falsified for `FUN_801DD9D4` (all seven dumps stop at 276 of 588 bytes) | Plausible: seven independent dumps agreeing on a length is as strong a corpus signal as exists. The decompiler stops at the `jr v0` jump table at `0x801DDA88`, which the `beq` at `0x801DDA78` branches past; the body runs on to the `jr ra` at `0x801DDC18`. Agreement across dumps of one program is agreement about the *program*. |
| A feature view's Port % is a property of the port | falsified (the ignore list sat on both sides of the fraction) | Plausible: the dashboard's other columns behave, and a percentage that moves when work lands is what a progress figure should do. Ignored rows were counted in the numerator *and* the denominator, so the headline also moved when an ignore row was added and nothing about the port changed - `cd-io` read 2.6 against a true 100, `field-vm` 66.2 against 100. |
| Every slot-B record-end miss stalls below the measured end, so the rule cannot over-claim | half falsified (the direction is not uniform) | Plausible: a walk that dies on a halfword it cannot decode intuitively stops *early*, and the first measurement of the residue reported exactly that. An independent walker over the 1023 pointer-credited extents finds 990 exact and **33 misses: 14 walk past the end** before dying on a non-opcode halfword, 13 stop exactly on it and 6 stop below. What survives is narrower and is the property the consumers need: no miss ever **terminates** above the end, because only a terminator - `0x08` HALT or an armed idle loop - produces a claim at all. A direction measured on one walker is a property of that walker. |
| A backward scan of a function finds every source of the value it stores | falsified (the value can arrive through a pointer argument) | Plausible: a store's operand is produced somewhere above it, and for a register-formed constant the scan is exhaustive. `FUN_801D6704` seats the player from a stack pair that `FUN_8003AEB0` fills **through a pointer argument** - `a1 = sp + 0x20`, set in the `jal`'s delay slot at `0x801D6DAC`, with the writes `sh v0,($s6)` / `sh v1,2($s6)` at `0x8003B7D0` / `0x8003B7D4` off `_DAT_80073EF4` / `_DAT_80073EF8`. Scanning `FUN_801D6704`'s own 3604 bytes for `0x80073EF4` returns nothing, so the scan reports a value with no source. |
| An open-ended CDNAME block's range is safe to use as a length | falsified (it reserves the rest of the address space) | Plausible: every other block in the map is bounded by the next `#define`, and the last one's end had never been the number anything multiplied. `other7` runs from entry 1226 to the end of the map, so the un-clamped range fed a `Vec::with_capacity` of 64 GiB on a scene load - which Linux overcommit granted silently until a test run met the memory watchdog. Reproduce this class with `ulimit -v` on the suspect binary: an overcommitted reservation only aborts under a cap. |
| `asset overlay scan`'s recovered base is a vote | falsified when the image offers one prologue | Plausible: the instrument is a vote over recovered prologues and reports a winner, and a winner over one voter is still the shape the output prints. On PROT 0981 it answered `0x801D58B8` off a single prologue; decoding the image's own `lui`+`addiu` pairs against each candidate scores 21 of 23 at slot-A `0x801CE818` and 0 of 23 there. The instrument degenerates silently - nothing in its output says how many prologues the margin rests on. |
| The browser play page emits no audio | falsified (an observer-order defect) | Plausible: the probe read zero in every block on every scene, which is what silence looks like, and the page's audio path was the un-instrumented half. The listener was registered **before** the mixer installed its own `onaudioprocess`, so it watched a handler that was later replaced. Intercepting the setter instead gives 82 non-zero blocks of 82 on town01 and 76 of 76 on the boot chain. |
| A dump whose window matches the image confirms coverage there | falsified (a `nop` encodes `0x00000000`) | Plausible: a byte-identical window is the strongest attribution evidence there is, and every other class of match has held. A window of `nop` matches every zero hole on the disc, so a 20,060-byte dump signed for a 131,172-byte zero run in an image it does not belong to; two dumps at one address "agreed" because both windows were zeros. Attribution needs a non-zero discriminator, which is why the sweep now classes a zero window separately. |
| PROT 0896 holds no strings to anchor it | falsified (its head is a label table) | Plausible: the base-fitting work found no base and no Shift-JIS *code*, and "nothing to anchor it" is the natural next sentence. The head is a Shift-JIS label table and the image carries a format string unique on the disc, so the entry has identity evidence even though no base fits its address-forming pairs - the two questions are separate, and answering the second `no` does not answer the first. |
| A port's eye-Z figure quoted against retail names a state that exists | falsified (one of the pair had no state) | Plausible: a before/after pair of numbers reads as one measurement, and the other half of the pair did reproduce. There is no `town01` mode-5 state in the library, so the `town01` half was quoting a framing nothing measured; the free-roam half reproduces exactly. Label every relayed figure with the state it was taken on, or half a pair can outlive its evidence. |
| PROT 0981 is a monster-test harness | falsified (it is the world-map top-view debug image) | Plausible: the entry's CDNAME label reads `monster_test`, and a label that specific is hard to argue with. Labels inherit forward from the block that opens at extraction 0978, so the name says which block the slot is in and nothing about its content. The image's own operands are world-map ones - the location table `DAT_80073EE0`, the kingdom filter, the camera pair - and its entry `0x801CE850` is the top-view prologue ([settled](re-settled-threads.md#world-map--kingdom-bundles)). |
| `0x801CE9C4` is a 324-byte routine | falsified (it is a `jr $v0` arm inside `FUN_801CE850`) | Plausible: the corpus prints a body at the address and the bytes are real, which is the usual evidence for an entry. The tick at `0x801CE850` bounds its own dispatch with `sltiu a0, 6` over a six-word table at `0x801CE838`, and all six words land inside that one 3164-byte body - so the address is an arm. Nothing about the image's load base depends on it, which is what made the mis-reading cheap to carry. |
| `0x801D388C` hosts a Muscle Dome routine as well as the battle flow state machine | falsified (one routine; the dumps differ only in prefix) | Plausible: two dumps with different overlay prefixes at one VA is the signature of slot-A aliasing, which is real and common. PROT 0977 is `0x3800` bytes from `0x801CE818` and therefore ends at `0x801D2018`, below the address entirely; the `overlay_muscle_dome_` and `overlay_battle_action_` dumps are the same instructions of PROT 0898. A dump prefix records the capture a dump came from, not the image its bytes belong to. |
| The browser play page's frame path has no early-out | falsified (the early-out is in the page's JavaScript) | Plausible: the Rust runtime's `tick_frame` was read end to end and the arms counted, which answers the question asked of the wrong layer. The page gates the whole call to `tick_frame` behind its own condition and draws outside it, so a guarded frame runs none of the per-frame kernels and still paints - the same shape as the native loop's `continue` arms, one language further out. A drift tier that reads only Rust cannot see it. |
| `0x801CF344` may be a phantom print from a mis-based dump | falsified (it is PROT 0897 data) | Plausible, and the class is real: the `0x801C****` / `0x801D****` band is full of addresses whose printed VA belongs to no runtime image. This one is the field overlay's own file offset `0xB2C`, formed by `lui`/`addiu` pairs inside the renderer's body, and a live `map03` window is byte-identical to 0897's head. What made it look unattributable is that PROT 0981 aliases the same VA and the two are never co-resident ([settled](re-settled-threads.md#world-map--kingdom-bundles)). |
| The native minigame step runs fishing venue actors and dance count-in spawns the browser has no twin for | falsified (both reach the browser) | Plausible: the waiver was written from the native side's call list, and a kernel named in one host and not the other is the ordinary shape of drift. Both do reach the browser. Exactly three sub-steps have no browser counterpart - the dance sequence-clear spawns, the Baka round chrome and the effect-pool ageing - and all three for one reason: only the native window owns `window/minigame_fx.rs`. A waiver naming the wrong members hides the reason they are missing. |
| ... and its correction named the effect pool as the one reason | falsified again (the dance spawns were never the pool's) | The row above is this page's own correction, and it was wrong in the other direction. Two of the three "no browser twin" members do not belong to the host pool at all: `DanceGame::judge_press` spawns the three sequence-clear parts into the **run's** own pool, so the native window spawned a duplicate set and drew both - every cleared sequence painted its banner and stars twice - while the play page drew neither and the minigames page had been drawing them from retail's widget cells all along. The pool is world state now and all three hosts drain it. A waiver that names one cause for a group has to hold for each member. |
| No load base makes PROT 0896 self-consistent | falsified (`0x801D4DF0`, on the call graph) | Plausible, and the instrument was the problem rather than the image: a window slid across every address the file's `lui`+`addiu` pairs form, scoring the best position, with a pinned control holding all but two of more than twelve hundred. A resolution ratio is **one-sided** - a base whose high halves catch few pairs scores perfectly on all of them - and on this image it reports 65 of 65 at the refuted slot-A base against 110 of 177 at the true one. It also cannot see the call graph, which is where the answer was: ten corroborating `jal` targets, 218 internal `j` all landing in-file, and three in-image VA word runs ([settled](re-settled-threads.md#title--boot--overlays)). |
| PROT 0974's uncovered run is a pointer table | falsified (sparse `.bss`) | Plausible: the run is long, low-entropy and sits where a table would, and "pointer table" is the residue class this corpus really does carry. It is 81 % zero with a single RAM word in it - an uninitialised data segment, which is what a code image's tail usually is. A residue class is a hypothesis about bytes, and the zero fraction is the cheapest test of it. |
| The `0x2399C` run in PROT 0897 is a halfword table | falsified (it is the field overlay's **data segment**) | Plausible: its head really is a table - the locomotion probe footprints - and a run that starts as a table reads as one. The segment is file `0x2399C..0x25000`, 5732 bytes, with 231 distinct sites in the image forming addresses inside it; the probe table is its first 192 bytes, twelve rows, of which the locomotion reads four. Do not name a segment after the first structure in it. |
| A frame kernel paired by name does comparable work on both hosts | falsified (an empty body pairs with anything) | Plausible: the host-drift gate's tier 11 exists precisely to catch a step one host does not take, and a paired step plus a written reason is what "checked" looks like. The native `tick_field_prop_anims` was `pub(super) fn tick_field_prop_anims(&mut self) {}` while its aliased browser twin drained the ANIMATE cues and advanced every NPC clip, and the alias row's reason asserted both "advance the scene's posed actors". Tier 12 compares the engine call sets instead - and states its own hole rather than closing it ([settled](re-settled-threads.md#measurement--corpus)). |
| `engine-core` has no host hook for field-VM op `0x34` sub-0 | falsified (the hook is live) | Plausible: the port tag said so, and a tag is the one place a reader expects the wiring answer to be. `World::op34_sub0_color_intensity_setup` is reached on both hosts. The real gap was a **representation** conflict rather than a missing call, and the blocker was wrong about that too: it said the renderers read an `effect_tint` ramp while the op filled a push pool nothing read, when in fact *neither* representation had a reader. A blocker naming a missing caller directs effort at a caller that already exists; one naming the wrong live reader directs it at a swap where a deletion was owed. |
| The port's field direction ring is byte-identical to retail's | falsified (retail's is `u32[8]`) | Plausible: the eight values agree, and "byte-identical" is the phrase that gets used when they do. The remapper reads `DAT_800766FC` with `lw` and steps it by `addiu 4` (`0x80046818` / `0x80046834`), so retail's ring is eight **words**; the port's `FIELD_DIR_RING` is `[u16; 8]`. The *values* agree on all 64 (octant, direction) cells, which is the claim worth making. |
| 32 battle-presentation names are web-only - the native host never calls them | falsified (31 have native call sites) | Plausible: a `.name(` scan finds none. Path calls (`Type::name(`) are the native host's usual form, so scan `[.:]name(`; the 32nd, `packet_color::hybrid`, is the page's WebGL vertex-colour stream, which the wgpu fragment shader replaces. |
| The play page has no Records dev-menu page | falsified (it has one, from the same builder) | Plausible: the page's menu showed no Records row at the moment compared. `play_dev_menu.rs` builds it with the native builder and a drift-gate row pairs them; what differs is **when** each host builds the list. |

**Generalises to:** a measurement instrument has no oracle, so a number it prints
is believed on the strength of its *explanation*. Check the explanation against
the files, not against its own plausibility - three of the four rows above are a
correct count with a wrong story attached, and the story is what directed effort.

### Two overlay lengths that were the neighbour's sectors

*Falsified by TOC arithmetic.*

| Reading | Why it looked right | What is true |
|---|---|---|
| `arena_init` (PROT 0977) own content is about `0x4800` bytes | The file the old entry size produced was that long and disassembled cleanly to the end | The entry is `0x3800`; the extra `0x1000` is PROT 0978's two sectors, read through the superseded over-reading entry size |
| The battle overlay (PROT 0898) is `0x28800` of `0x29800` bytes, with a diverging `0x1000` `.bss` tail | A RAM capture matched the first `0x28800` and the tail differed, which is what `.bss` does | The entry is `0x28800`; the diverging tail is PROT 0899's first two sectors |
| `0x801D2784` is PROT 0979's battle-intro transition tail | falsified (the bytes are PROT 0976, Baka Fighter) | The dump is labelled 0979 and 0979 is the battle-intro overlay, so the tail reads as its own. PROT 0979 and 0976 are byte-identical from file `0x3C68` to the end of the smaller image, so no attribution sweep can separate them - the **operands** can: the routine reads `0x801DBED8..0x801DBEF0` and calls `0x801D6710`, all past 0979's own `0x4000` and inside 0976's `0xE000`, bracketed by 0976's documented emitters `801D6480` / `801D6770`. Slot-A residue rule: a byte range shared by two images belongs to the one whose addresses it names. |
| `0x801DDA90` and `0x801DDB44` are two slices of one loop, neither a function entry | falsified for the first of the two | Both look like fragments - no prologue, and each ends in a `j` to a shared tail - so "one loop, printed twice" is the economical reading, and it is right about `801DDB44` (it is `0x24` into slot 4's arm, at the `j 0x801DDBC8` and its delay slot). `801DDA90` is slot **0** of the eight-entry `jr` table at `0x801CEC40` that `FUN_801DD9D4` dispatches through, and that table word is its only reference on the disc - which is what makes it an entry. A frameless routine reached only through a table is still a routine. |
| `FUN_801E59B0` gives its two trig tables two different angle indices | falsified (one index, both tables; the components are swapped) | The C renders two subscripted loads with different-looking expressions, and a rotate that samples sin at `angle` and cos at `angle + 0x400` is the textbook shape - so `vec[0] * t1[angle] + vec[1] * t2[(angle + 0x400) & 0xFFF]` reads as correct. The instruction stream computes `i = (angle + 0x400) & 0xFFF` once at `0x801E59B0..0x801E59BC` and reuses it at `0x801E59C8` and `0x801E59E4`: the body is `(vec[1] * t_a[i] + vec[0] * t_b[i]) >> 12`, with the components the other way round. `0x8007B81C` and `0x8007B7F8` are table **pointers** the routine `lw`s, not the tables. |
| The PROT 0898 entry tables resolve the slot-B attribution residue | falsified (they close 6 extents / 48 bytes, and name the wrong module for 10 of 15) | The three link-time tables are the right instrument for *reachability*, and having just used them to map the whole band it is natural to expect them to close the byte residue too. Measured, they do not: the residue is a **byte**-denominated ambiguity between images that share bytes, and a table that names an entry says nothing about which image the bytes at that entry belong to. Ten of fifteen table-derived attributions named a different module than the bytes do. |
| A `disc-coverage --check` floor regression means coverage was lost | falsified (it is attribution lag) | A ratcheted floor going down is the definition of a regression for every other gate in the tree, so the reading transfers by habit. This gate's denominator is the **disc**, and its numerator is what the attributed dump corpus claims: adding a new, not-yet-attributed dump raises the denominator before it raises the numerator, so the percentage falls while the corpus strictly grew. Re-attribute, then re-read the floor. It is not a worktree artifact and re-running in the main checkout does not clear it. |
| `801D84C0` is wired because `panel_anchors` is called | falsified (the bucket is a property of the anchor, not of the address) | The address appears in a tag on a module whose exported function is called from a live host, so "reachable" reads as settled. The `// PORT:` tag that carries `801D84C0` sits on `panel_labels`, a different item, and the live-audit walks from the **tagged item**. Reading "is this address live" off a neighbouring symbol's reachability is how an inert port keeps a green audit. |
| The world-map overlay's per-prim handler table is based at `0x801F8988` | falsified (`FUN_80043390` loads `0x801F8968`) | The first non-zero word of the table is at `0x801F8988`, and naming a table by its first entry is the natural instinct when the eight words before it are zero. The dispatcher's own pair says otherwise: `lui s4,0x8020` / `addiu s4,s4,-0x7698` at `0x800435F4..0x800435F8` materialises `0x801F8968`, and it adds the same `(flags >> 1) * 4` index it would add to the SCUS table `0x8007657C` - which has the identical shape, words `0..7` zero and `8..19` populated. Re-basing by the zero prefix shifts every kind by eight. |

Both notes lived in `crates/asset/data/static-overlays.toml`, whose rows predate
the entry-size correction. The general law is on
[`prot.md`](../formats/prot.md); the measured consequence - the TOC is a
gapless partition - is on
[`re-settled-threads.md`](re-settled-threads.md#measurement--corpus).

## Related pages

- [`open-rev-eng-threads.md`](open-rev-eng-threads.md) - the live hunts.
- [`re-settled-threads.md`](re-settled-threads.md) - the answered questions, each with an evidence grade.
- [`docs/tooling/ghidra.md` § decompiler artifacts](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims) - the seven C-rendering artifacts that produced several of the readings above.
- [`docs/tooling/call-target-integrity.md`](../tooling/call-target-integrity.md) - why a decoded `jal` target is a property of the bytes, not the load base.
