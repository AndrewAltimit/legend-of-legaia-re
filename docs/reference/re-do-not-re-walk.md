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
| Gaza 2 `0x51` park: clamp asymmetry as a standalone retail generator | falsified (amplifier only; its exhibit was a phased mid-action state) | [details ↓](#gaza-2-0x51-park---the-two-falsified-generators) |
| Gaza 2 `0x51` park: the Final Heal revive lands "at the worst possible moment" (mid-drain) | falsified on the Gaza 2 move set (12/12 revives found the accumulator already drained) | [details ↓](#gaza-2-0x51-park---the-two-falsified-generators) |
| Muscle Dome as a **card battle** with a per-fighter "score out of 108" | falsified (it is a 4-turn battle; the readout is the opponent's HP percentage) | [details ↓](#muscle-dome-was-never-a-card-battle) |
| Muscle Dome awards a **Seru** on a win | falsified (a leg pays nothing; a contest pays casino coins) | [details ↓](#the-dome-victory-caption-is-not-a-prize) |
| `FUN_801DBC30` blits the party panels' name plate | falsified (its page + CLUT resolve to the `etim` red cross-out X) | [details ↓](#fun_801dbc30-is-not-the-battle-name-plate) |
| The retail party HUD carries HP / MP gauge bars | falsified (no bar primitive in either readout's packet run) | [details](../subsystems/battle.md#the-party-status-readout---and-it-has-no-gauge) |
| Screen-element kinds named by what sits at their seat (`0x32`/`0x33` = "the roster panels") | falsified (naming by seat named the wrong record) | [details ↓](#a-kind-named-by-its-seat-can-name-the-wrong-record) |
| The battle message banner is "a gold border over a blue interior" | falsified (border only - no fill primitive under it) | [details ↓](#the-battle-message-banner-has-no-interior-fill) |
| `FUN_801E2524` / `FUN_801E2650` are a full-screen flash / fade ramp | falsified (they are the **Arts announcement banner**) | [details ↓](#the-flash-ramp-is-the-arts-announcement-banner) |

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

## Audio / sound driver

| Thread | Verdict | Why |
|---|---|---|
| `FUN_80068D94` as "`SsSepOpen` / SEP loader" (with `FUN_80068B98` as "`SsSeqOpen`") | falsified (it is the VAB-open head) | The plausible part: it validates a magic, reads a count at `+0x12`, `SsSpuMalloc`s, and patches a pointer table - the shape of a SEP/track loader, with the magic read as 'VAP'. The disassembly refutes it: the compare is `0x564142` against `word >> 8` plus low byte `0x70` - `pBAV`, the **VAB** magic - and `+0x12` is `ps`. The "per-track pointer table" is the ProgAtr table receiving the program → packed-tone-page rank map ([`vab.md`](../formats/vab.md#program-slots-vs-packed-tone-pages)); the mislabel hid that map, and with it the engine's tone collapse on sparse banks. Correct roles: [`audio.md`](../subsystems/audio.md#ssapi-seq-management-layer-above-libspu). |
| The entry that matches "`[u32 format == 2][u16 spu_addr[256]]`, every address `>= 0x8000`" is `monster.snd` | falsified (it is `summon.dat`; `monster.snd` is a multi-bank VAB two entries away) | [details ↓](#the-256-slot-spu-address-run-that-was-really-a-clut) |
| Op-`0x35` sub-op 9 is a **queue**, triggered by the next scene entry | falsified (it is a start behind an asset-load barrier) | [details ↓](#op-0x35-sub-op-9-was-never-a-queue) |
| `_DAT_8007B910` is the live screen brightness | falsified (it is the live **audio level**) | The reading fit the behaviour: the cell ramps down during a summon and back up when the action ends, and a summon does visibly dim the screen. But no reader supports it - all 26 dumped read sites end in a volume setter, none in a draw primitive. The dim rides the separate accumulator `_DAT_8007B440` (`FUN_801ED308` → the wipe emitter `FUN_8003479C`); the two ramp together, which is what made one look like the other. Answer: [`re-settled-threads.md`](re-settled-threads.md#_dat_8007b910-is-the-live-audio-level-not-screen-brightness). |

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

## Containers / placeholder slots

| Thread | Verdict | Why |
|---|---|---|
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
i.e. `FLAG_MESH_DRAWN` **clear**, and stamping it was separately falsified
against retail: it draws a wall down every river (`FLAG_MESH_DRAWN` in
`crates/asset/src/field_objects.rs`). And not the site's assembled map view or
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

## Field / locomotion

| Thread | Verdict | Why |
|---|---|---|
| "~270 undumped field-overlay functions" (recomp dispatch-entry seed list) | falsified (not a function inventory gap) | [details ↓](#270-undumped-field-overlay-functions-recomp-dispatch-entry-seeds) |
| Scene-bundle type-6 descriptors are "all small placeholders" | falsified (12 are walker tables) | Plausible: the modal slot really is a 4-byte `count = 0` filler (85 of 97 bundles) and the three kingdom tables had been attributed to a "kingdom slot 5" special. But the 80/172/516-byte type-6 payloads (`garmel`, `dohaty`, the `geremi`/`rayman`/tunnel/`son`/`edson` family) parse as the same CLUT-walk table, installed identically for every bundle - the water/waterfall shimmer. The `rayman`-family carrier is the count-4 MAN-less table variant the strict detector rejects; resolve **by type byte** ([field-ambient-fx.md](../subsystems/field-ambient-fx.md#mechanism-1---the-scene-walker-table-bundle-type-6-slot)). |
| Move-VM loop op `0x19` "retires past itself (size 2), loops back to the saved PC" | falsified (both halves inverted by the C rendering) | Wrong against the raw arm (`80023070.txt` `0x800235DC` + the `0x80024150` epilogue): retail **loops while the decremented count has not underflowed**, retires on underflow with size **1**, and the loop-back lands at **saved + 2** (the epilogue adds `a2 = 2` after the PC store) - re-running the `0x18` itself would re-seed the counter forever. jou's 15-instance cycler fan-out is the disc witness. Sibling correction: ext `0x1E` returns size **4**, hidden behind a `func_0x801d4a3c()` label-call return ([move-vm-overlay-ext.md](../subsystems/move-vm-overlay-ext.md#self-modifying-bytecode-ops-0x04--0x1b--0x1e)). |
| Field-VM op `4C` nE sub-3 "syncs the resolved actor's position to the active camera" | falsified (copy direction inverted) | Plausible because the handler tail (`0x801E3178..0x801E31AC`) really does refresh the camera-scroll globals - but that tail is a player-ctx-only side path. The op body (`0x801E3108`) copies the operand-resolved actor's `+0x14/16/18` position and `+0x26` facing **into the executing ctx** - it is the seat primitive of every mid-visit crowd swap (dolk2 `P2[11]`'s eight `CC <crowd> E3 <day>` pairs). Reading the tail as the op's purpose inverted the semantics. See [script-vm.md](../subsystems/script-vm.md#mid-visit-npc-re-arrangement-beats-dolk2-market-swap--garmel-boss-staging). |
| Extraction-0874 §2 F-variant pixels are written by a pause-menu-path uploader (and then: are a parked wrap-scroll phase) | falsified twice | Plausible: 6/6 pause captures held the variant; then the 3 words equal row 273's content, reading as a +2-row scroll park. But the whole pause walk issues **zero** image transfers (DMA2 chain-walk + GP0 PIO hook) and plain field saves carry the variant - session-history correlation; and the strip is not shift-invariant while the wrap-scroll installer ops never fire across the s2→s3 flip window - the row-273 equality is frame-content coincidence. The real writer is the town01 opening record's one-shot `4C 60` face-frame stamp (settled - [details](re-settled-threads.md#field--locomotion)). |
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

## Measurement readings

Falsified claims about the *instruments*, not about the game. They belong here
for the same reason the rest do: each was a plausible reading, each was believed,
and each shaped what work looked worth doing.

| Thread | Verdict | Why |
|---|---|---|
| The disc-coverage report's excluded dumps are "typically the ones that report `0 instructions` and hold only decompiled C" | falsified (zero of them reported `0 instructions`) | The files that *do* report `0 instructions` were passing the header regex and being credited a byte each. Of the excluded set, three were C-only and four fifths were not dumps at all - pointer stubs, recorded negatives, data windows, analysis output. The count was real; the sentence attached to it had never been checked against the files. |
| The inner of two nested overlay spans "cannot be repaired ... no amount of dumping moves it" | falsified | Address ambiguity really is total for the inner span - every extent in it falls in both by construction. Byte attribution then places most of those extents in one image or the other, and the row reports. The **starting point of a measurement was mistaken for its limit**, and the structural-sounding argument made it read as settled. |
| The unattributable residue "is repaired by re-dumping, not by extracting another overlay" | falsified | Re-dumping repairs almost none of it. What remains is windows a few instructions long that no image reproduces at that VA, bytes in no extracted image at any VA (which needs an *extraction*), and extents where two dumps genuinely disagree (which is an answer). The residue had been described from its class names rather than counted from the artifact. |
| `0x8005BA38` is "not a function - the dump reports `size=1 bytes, 0 instructions`" | falsified (it is a complete `RotTransPers`) | The dump was empty when the row was written and is 11 instructions now: load `VXY0`/`VZ0`, `RTPS`, store `SXY2` / `IR0` / the GTE `FLAG` word, return `SZ3 >> 2`. Nothing re-reads a caveat when its dump improves, so **a claim quoting a dump statistic decays silently** while reading as evidence-backed. Sibling instances: a "truncated dump" at 752 bytes that is 1528 today (three things left unported on it), and `0x8003D38C`'s ignore row, whose *verdict* survives - it is one instruction past the real entry `0x8003D388` - but whose stated evidence did not. Checker: `scripts/ghidra-analysis/check-dump-stat-drift.py`. |
| "About 2 % of retail camera beats set a non-zero roll" | falsified (the figure was the scan's own filter) | The number came from a **byte scan** - decode an op-`0x45` CONFIGURE at every offset of every scene MAN - which finds 4257 "sites" where control flow reaches 371. Because junk sites set a junk roll almost every time, the scan applied a post-hoc "credible" filter and then measured roll over the survivors, so the ratio is a property of that filter. Its sibling strict linear sweep reported the opposite (zero non-zero rolls) by reaching 21 sites and none of the eight real ones. The answer - retail *does* roll, in eight scenes - came from executing the records, not decoding them: [`re-settled-threads.md`](re-settled-threads.md#does-any-retail-shot-author-a-non-zero-camera-roll). |
| An over-strict header regex is one instrument's bug | falsified (every instrument had its own) | Each tool over the dump corpus carried a private header regex, and the corpus spells all four header fields several ways, so each silently rejected a different subset of **real dumps** and reported them as a corpus deficiency. Fixed by one shared parser; see [`dump-corpus-integrity.md`](../tooling/dump-corpus-integrity.md#not-every-file-in-funcs-is-a-dump). |

**Generalises to:** a measurement instrument has no oracle, so a number it prints
is believed on the strength of its *explanation*. Check the explanation against
the files, not against its own plausibility - three of the four rows above are a
correct count with a wrong story attached, and the story is what directed effort.

## Related pages

- [`open-rev-eng-threads.md`](open-rev-eng-threads.md) - the live hunts.
- [`re-settled-threads.md`](re-settled-threads.md) - the answered questions, each with an evidence grade.
- [`docs/tooling/ghidra.md` § decompiler artifacts](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims) - the seven C-rendering artifacts that produced several of the readings above.
- [`docs/tooling/call-target-integrity.md`](../tooling/call-target-integrity.md) - why a decoded `jal` target is a property of the bytes, not the load base.
