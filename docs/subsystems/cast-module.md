# Capture-class cast modules

The per-spell overlay programs behind every capture-class cast - Seru
capture, the item-capture Amulet, and the boss cinematic specials. A
capture-class spell (class byte `'c'` at spell-table `+0`) does not run its
choreography from the battle overlay: the action SM's `0x63` arm pages a
**module** - one PROT entry from the band `0935..0966`, selected by the
spell record's `+1` sub-id
([spell-table.md](../formats/spell-table.md#capture-class-module-index-prot-09350966)) -
into the battle's side overlay window and re-enters it every frame until it
reports done. The three Delilas signature modules are the decoded
exemplars: PROT **958** (Blazing Slash `0x79`, Gi), **959** (Megaton Press
`0x7A`, Che), **960** (Plasma Strike `0x7B`, Lu).

This page is the module anatomy: image structure, phase machine, tick ABI,
and damage shape. The staged clip *sequences* per module live in
[monster-animation.md](../formats/monster-animation.md#a-special-attack-can-be-a-chain-of-entries);
the per-module damage-wrapper census lives in
[battle-formulas.md](battle-formulas.md); what a *player*-caster port of a
module has to change is on
[randomizer.md](../tooling/randomizer.md#the-retail-cast-route).

## Paging and the drive loop

Battle phase `0x28` routes a capture-class action to `0x6E..0x71` and pages
the module via `FUN_8003EC70(record[+1] + 0x28)`. Phase `0x70` re-enters
the module's tick **every frame and advances only when the tick returns
zero** - there is no timer and no bail-out, so a module phase whose exit
gate can never pass is a softlock
([re-do-not-re-walk.md](../reference/re-do-not-re-walk.md)).

All modules link at the slot-B base **`0x801F69D8`**, and while one is
resident **the word at `0x801F69D8` is the module's own word 0** - a third
meaning for an address the corpus already maps twice (PROT 0900's
jump-table head, and the world-map band's `FUN_801F69D8`). A probe watching
that word sees `0x001000E2` (empty - that is what 0898's own image holds
there), then the module's word 0 at paging. Word 0 is **not** an entry VA:
where the module heads with a jump table it is that table's arm 0, and
where it does not it is the first instruction. Both real entries are named
from outside the image - see [the entry tables](#the-entry-tables-and-where-the-addresses-live).

| Module | Size | Head shape | Tick trampoline | Tick body |
|---|---|---|---|---|
| PROT 958 | `0x3000` B | 256-entry VA dispatch table fills file `0x0..0x400` (default arm `0x801F8CFC`) | `0x801F8E60` | `0x801F6DD8` = file `+0x400`, 8024 B |
| PROT 959 | `0x3000` B | 6-entry head table (`0x801F8290, 82C4, 8478, 850C, 8600, 878C`), its own **stager** dispatcher at `0x801F8250` | `0x801F87F4` | `0x801F69F0` = file `+0x18`, 6240 B |
| PROT 960 | `0x2800` B | none - code at file `+0` | `0x801F8638` | `0x801F74E4` (`0x7B` Plasma Strike, 4436 B) / `0x801F69D8` (`0xA6` Neo Star Slash, 2828 B) |

Two figures in that table **correct** earlier entries here. 958's tick body is
file `+0x400` (the first word past the head table), not `+0x4C4` - `0x801F6E9C`
is 0xC4 bytes INTERIOR to it, inside the register-save block. 959's was given as
file `+0x18B8` (`0x801F8290`), which is interior to its 1444-byte **stager**
`0x801F8250`, not a tick at all. Both are now read off PROT 0898's own arm
table (below) rather than inferred from the head table.

Module code cites below are given as file offsets with their VA
(`0x801F69D8 + off`); the three images share the base, so an offset is
meaningless without naming its module. The tail region past `~+0x2A00` is
words 958 and 959 hold in common, and it is **not** shared library code -
every image in the band ends in a same-file-offset copy of another extracted
image's bytes, mastering residue rather than anything the module runs. See
[the band as a port worklist](#a-module-image-ends-in-another-images-bytes).

## The entry tables, and where the addresses live

A module is a **library, not a program**: nothing inside it names its own
entry points. No entry VA appears anywhere in any extracted image - not
as a word, not as a `lui`+`addiu` pair, not as a `jal`/`j` target (five-form
sweep, `scripts/ghidra-analysis/find-address-word-refs.py`) - except inside
**PROT 0898**, where they are fixed at link time. Every module therefore has
two callable routines, and **three** tables in 0898 name them: two tick tables
(one per band) and one stager table.

**The cast tick.** `FUN_801F1ED4` (0898, file `+0x236BC`) loads the caster
`actor_table[ctx+0x13]` from `0x801C9370`, takes its queued action byte
`actor[+0x1DF]`, bounds-checks `id - 0x81 < 0x20`, and jumps through the
32-slot table at **`0x801CF4EC`**. Each arm is a hard-coded `jal` into the
resident module plus `s0 = v0`; the shared epilogue at `0x801F2128` calls
`FUN_801F2410` when `ctx[+0x27A] != 0` and returns `s0`. Id `0x98` has no
arm - its slot points straight at the epilogue, so that id ticks nothing.
The battle SM `FUN_801E295C` calls the dispatcher from three sites:
`0x801E4B1C` (cast start - zeroes `ctx+0x278` and the module phase
`ctx+0x279`, seeds the countdown `s7[+2] = 0x78`), `0x801E4C7C` (per-frame
re-entry; the countdown running out forces battle phase `0x36`), and
`0x801E4CA8` (proceed only when the tick returns `0`).

**The move-VM extension.** The 64-word table at **`0x801F6734`** (0898, file
`+0x27F1C`, bounded by byte tables below and ASCII above) holds one routine
per module. The SM copies the selected row into `gp[+0x714]` = `0x8007BA2C`
at `0x801E44C8` (capture class, row `sub_id + 0x20`) and `0x801E4630` (row
`move_id - 0x81`), and **move-VM opcode `0x20`** calls it: `lw v0, 0x714(gp);
jalr v0` at SCUS `0x80023764`, with `a0` = actor and `a1`/`a2` = the
move-table instruction's two signed halfwords. `a1` is an arm index: 15 of the
64 routines bound it with `sltiu vX, a1, N` and jump through a table, and the
rest compare it against literals in a `beq` / `slti` chain - either way this
entry is the per-spell **spawn stager** the move script drives, not the
choreography.
(The corpus previously called `0x8007BA2C` "the per-summon effect-data
pointer"; it is a code pointer, and `0x801F6734` is a table of entry VAs.)

Both indexings land on the same entry: **table row `i` is extraction entry
`903 + i`** for `i = 0..0x3F`, i.e. PROT 0903..0966. The `move_id - 0x81`
band `0x81..0xA0` covers 0903..0934, the capture-class `sub_id + 0x20` band
covers 0935..0966, and the pager's own `id - 0x79` (`FUN_8003EC70`, `+0x381`
into TOC space) agrees with both.

**The capture-class cast tick.** The `0x801CF4EC` switch reaches only PROT
0903..0934, and the rest of the band has its own dispatcher: `FUN_801F2160`
(0898, file `+0x23948`; `0x801F2160..0x801F2410`, 688 B with its arms and epilogue). It derives the
caster the same way, then - instead of `id - 0x81` - reads the queued action id
`caster[+0x1DF]`, indexes the **static spell table** `0x800754C8` at `id * 12`,
takes the record's `+1` byte (the capture-class sub-id), bounds it
`sltiu v0, sub_id, 0x20`, and jumps through a second 32-slot table at
**`0x801CF56C`**. Arm `i` = extraction PROT `935 + i`, each a hard-coded
`jal` into the resident module plus `s0 = v0`, with the epilogue at
`0x801F23D8` mirroring `0x801F2128` exactly (`ctx[+0x27A] != 0` ->
`FUN_801F2410`, return `s0`). The battle SM calls it from **one** site,
`0x801E50C8`, against the summon dispatcher's three.

This **settles** what this page previously recorded as open ("no hard-coded
`jal` into the capture-class band was found"). The sweep that found none looked
for a second copy of the `0x801CF4EC` shape keyed on `id - 0x81`; the table is
keyed on the spell record's sub-id instead, so it sits at a different index
space and was missed.

**The capture-class arm is a trampoline, not the body.** Where a summon module's
`0x801CF4EC` arm calls the tick directly, most capture-class arms land on an
88..204-byte routine that re-reads `caster[+0x1DF]`, compares it against the
module's own spell ids, and `jal`s the matching tick body - the shape PROT 0957
was already documented with, generalised across the band. PROT 0960 is the clear
case: its trampoline `0x801F8638` sends `0x7B` (Plasma Strike) to `0x801F74E4`
and `0xA6` (Neo Star Slash) to `0x801F69D8`, so a "multi-spell cell" is two
whole choreographies in one image, not one body branching internally. Modules
whose cell holds a single spell (0935, 0936, 0937, 0939) skip the trampoline and
the arm points straight at the body.

That rule is checkable against the disc, and it checks out on the whole band.
For every one of the 64 entries, the `0x801F6734` row and the arm from whichever
tick table covers it land on a function head recovered from that image's own
bytes and from no other image - all 64 stager rows, all 31 `0x801CF4EC` arms
(id `0x98` has none) and all 32 `0x801CF56C` arms. That is the identity test the
map rows in [`static-overlays.toml`](../../crates/asset/data/static-overlays.toml)
rest on; it needs no capture and no dump corpus, only the disc.

Two arms need one refinement: a module's entry can sit a few instructions
**above** its prologue, where the routine materialises the battle ctx
`0x8007BD24` and the frame-delta scalar `0x1F800393` before setting up the
frame. PROT 0946 and 0953 both enter at `0x801F69FC` with the prologue at
`0x801F6A0C`. A prologue scan alone reports the later address; the table is the
authority.

## Image anatomy, recovered from the bytes

Most of these images carry **no internal `jal` at all**: every call leaves
for SCUS or for the resident battle overlay, so Ghidra's auto-analysis finds
at most the routine some other reference reaches, and the dumps have to be
driven from an address list rather than from a call graph. That is not
uniform across the band, though - **25 of the 64** images do have internal
calls, and the trampoline shape above is why: a capture-class module that
serves several spells calls its own bodies. (This page previously said 0957
was the single exception with two; it is one of 25.)

The bytes supply the address list. A function starts at `addiu sp, sp, -F` and
ends at the first `jr ra` whose delay slot restores the **same** `F`. That
frame-matched pairing is exact over the whole band and, unlike a
count-and-interleave rule, it survives the three shapes that break the counts:
a frameless leaf, an early `jr ra` inside a body, and a `jr ra` word that is
just data in the image's tail (PROT 0906 has one at `0x801F8070`). The
partition recovers **196 framed functions** across the 64 images, between one
(0903, 0926, 0939, 0947, 0954) and eight (0955) each; the entry tables then
confirm it. A module is laid out as:

| Region | Contents |
|---|---|
| head | the jump table of **one** of the two switches, 0 to 256 words; the first function's prologue is the first word past it |
| tick | the `ctx+0x279` phase machine (function A), reached from `0x801CF4EC` |
| stager | the `sltiu a1, N` spawn switch (function B), reached from `0x801F6734` |
| tail | data: the spawn/emitter records the module hands to `FUN_80050ED4` and `FUN_80021B04`, plus its own scratch words |

Which switch owns the head table varies, and the arm count settles it: PROT
0934's table is 26 words and its tick bounds on `sltiu a0, 0x1A`, while PROT
0929's is 9 words and its **stager** bounds on `sltiu a1, 9` (0928: 7 and 7).
Reading the head table as the tick's is therefore wrong about half the time;
read the `sltiu` immediate. Mechanised over the band, that test resolves the
head table for 19 images - the tick's in 8 (0921, 0922, 0925, 0930..0934) and
the stager's in 11 (0906, 0909, 0910, 0913, 0914, 0916, 0917, 0928, 0929,
0941, 0959) - and 25 images head with code and no table at all. The per-image
answer is in the table on
[`functions/battle.md`](../reference/functions/battle.md#slot-b-summon--cast-modules-prot-09030966).

The tail is the bulk of the residue `disc-coverage.py` still reports on
these images, and it is data: PROT 0934's is `0x801F9C08..0x801FA9D8`, and
the `lui 0x8020` + negative-displacement operands its stager passes as `a2`
resolve into exactly that span.

### The three entries that nearly lost their base row

All 64 entries carry a
[`static-overlays.toml`](../../crates/asset/data/static-overlays.toml) row and an
extracted image. **0915** (spell `0x8D` Mushura), **0926** (spell `0x98`, the id
with no tick arm and no spell-table record) and **0935** (capture sub-id `0x00`,
Earthquake) were the last three to get a row, and 0926 - the 1-sector null stub -
is still the one entry whose own content is eight bytes.

The reason they were last is the slot-B base cross-check in
`crates/asset/tests/static_overlay_extract.rs`, which used to reject them. That check
counts each image's `lui 0x801f`/`0x8020` + `addiu` pairs and asks how many
resolve **inside** the image, and in its one-sided form it counted every
reference that leaves the image as evidence against the base - which a module
legitimately makes, in two directions:

| Entry | Where the misses point |
|---|---|
| 0915 | `0x801F6978` / `0x801F6980` - *below* the slot-B base, inside PROT 0898's own data |
| 0935 | `0x801FA320..0x801FA3B8` - above the image end, the post-image `.bss` working storage in the shared slot-B buffer |
| 0926 | `0x801F7D3C` / `0x801F7F2C` - the same post-image scratch; a 1-sector stub has almost nothing else to measure |

Excluding both kinds rather than crediting them makes the ratio a statement
about self-references only, and lets the acceptance floor *rise* from 0.60 to
0.90. All three then pass, and they already passed the stronger test - their
`0x801F6734` row (and, for 0915, their `0x801CF4EC` arm) lands on a
byte-recovered function head.

## The module phase byte (`ctx + 0x279`)

The tick is a dispatcher over its own phase byte at battle-ctx `+0x279`
(ctx pointer `0x8007BD24`) - values walk `0 .. ~0x11` and `0xFF` marks the
choreography done (960's dispatcher is the `beq`-chain at file
`+0x0BB0..+0x0C70`). This is a **second, module-local phase space** riding
under battle phase `0x70`; probes log it alongside `ctx+7`. Arms gate their
own exit - on the caster's clip state, on progress halfwords, or on the
shared-tail settle loop that waits until no live actor is playing anything
but the settle id `8` (960 file `+0x0A40`).

## Tick ABI: caster, victim, staging

**Derivation.** The tick prologue derives both parties from the actor
pointer table `DAT_801C9370`: caster = `table[ctx+0x13]`, victim =
`table[caster+0x1DD]` (the caster's target-slot byte). 958 at file
`+0x0438..+0x0460`, 960 at `+0x0B44..+0x0B6C`. Register discipline differs
per module and it matters to any patch: 959 keeps the victim in `$s4`
(written exactly twice in the image - the derivation and the epilogue
restore); 960 keeps it in `$s3` (derived once at `+0x0B6C`); **958's `$s1`
holds the victim only until an arm reuses it** - its finale arm burns
`$s1/$s3/$s4` (and even `$s2`) as GPU-packet constants.

**Staging.** The module drives clips through the actor anim channel
([battle.md](battle.md#one-staged-anim-channel-actor0x1da)): store the
action id to `+0x1DA` and bump the restage counter `+0x1DC`; the commit
mirrors the id into `+0x1D9` (the *playing* id) and `+0x1DB`. `+0x1F4`
counts clip loops while a staged clip repeats. Victim reactions are staged
from the victim's own reaction map - `lbu +0x1F1` (knockdown) stored back
to the victim's `+0x1DA` - with Block (`+0x1F3`'s id) held through build-up
phases. Two caster idioms appear, literal (`li <id>`) and stepper
(`lbu +0x1DA; addiu; sb`); the per-module walks are on
[monster-animation.md](../formats/monster-animation.md#a-special-attack-can-be-a-chain-of-entries).

**Paired stage/confirm gates.** A staging literal can have a twin: 960's
phase-5 arm stages id `0x0D` per tick and then holds the phase until
`lbu caster+0x1D9` **equals the same literal** (compare at file `+0x118C`,
VA `0x801F7B64`), ANDed with a progress check
(`ctx[+0x22C]->+0x68 >= 0x90`). An edit that remaps the stage without the
compare stalls phase 5 forever (probe-measured on a natural duel cast).
The gate census over the three modules: 959's two gates compare `+0x1D9`
against `+0x1F2` - register-register, immune to id remaps; 958 has **no**
caster-literal gate (its `+0x2134` gate is victim `+0x1D9` vs `+0x1F1`,
and the shared-tail gates compare the settle id `8`, which no cast
stages).

## Damage shape

Each hit is one call into a roll wrapper with a **baked per-hit power
constant** in `a0`, then an apply shape: clamp the roll against the victim's
HP `+0x14C`, accumulate into the victim's damage-popup word `+0x10`, load the
victim again, write HP back. Which wrapper a module calls - and the
per-module call census - is
[battle-formulas.md](battle-formulas.md)'s table; the constants are
[below](#the-baked-power-constants).

The apply shape is **three** shapes, not one, and which one a module uses
decides whether its hit can kill - see
[the three clamp shapes](#the-three-clamp-shapes). All three are ported at
`legaia_engine_vm::cast_module_ticks`.

### The baked power constants

Read off the `a0` set at each `jal` into `0x801DD0AC` / `0x801DD4B0` /
`0x801DD6B4`, in call-site order:

| Module | Wrapper | Powers (call-site order) |
|---|---|---|
| 927 (Juggernaut) | `FUN_801DD0AC` with `a1 = 7` - the shared kernel's summon branch | `0x12` |
| 945 (Water Column) | `FUN_801DD4B0` | `0x30` |
| 957 tick A `0x801F6A14` | `FUN_801DD4B0` | `0x100` |
| 958 (Blazing Slash) | `FUN_801DD6B4` | `0x30, 0x38, 0x38, 0x38, 0x40, 0x30` |
| 960 (Plasma Strike) | `FUN_801DD6B4` | `0x1C0` |
| 966 (Evil Seru Magic) stager `0x801F8D64` | `FUN_801DD4B0` | `0x100` |
| 966 tick body (the image's other function) | `FUN_801DD4B0` | `0x327` |

The 966 rows are two different routines in one image, and the split matters:
the module's stager bakes `0x100` and clamps shape B, while its tick body bakes
`0x327` at `0x801F8610` and clamps shape A at `0x801F863C`. PROT 0927 is the
same arrangement with the same constant on both sides (`0x12` at the stager's
`0x801F8758` and the tick's `0x801F7E0C`), so only the clamp differs there.
Ask a module's **entry number** what it hits for and you get its stager.

This page previously gave 958's run as "`0x30, 0x38, 0x38, 0x38, 0x40, ..`".
The sixth site (`0x801F88D8`) is `0x30`, so the escalation does not continue -
the run ends where it started.

Every one of these is an immediate compiled into the module image, so a
capture-class cast never reads the move-power table for its magnitude. The
engine seeds the wrappers from this table instead
(`World::baked_module_power`, feeding `capture_bypass_predamage` /
`capture_respect_predamage`).

### The three clamp shapes

<a id="the-two-clamp-shapes"></a>

The measurement is every `jal` into `FUN_801DD0AC` / `FUN_801DD4B0` /
`FUN_801DD6B4` in the 64 images: **83** such call words, **79** of them inside
a frame-matched function of the image carrying them. The other four sit in an
inherited tail and are a neighbour's site read twice - PROT 0911's
`0x801F887C` is PROT 0910's, 0951's `0x801F8E04` is 0934's, 0952's
`0x801F7F88` is 0951's and 0965's `0x801F853C` is 0964's. Of the 79 own sites,
**70 clamp shape A, seven shape C, and two shape B**.

**Shape A - clamp to HP, floor 0.** 70 of the 79 sites, PROT 0945, 0957 (both
tick bodies), 0958 and 0960 among them:

```text
a0 = victim[+0x14C]
sltu v0, a0, dmg        ; UNSIGNED
if v0 { dmg = a0 }
victim[+0x10]  += dmg
victim[+0x14C] -= dmg
```

The comparison is **unsigned** while the wrapper's return is a signed word, so
a negative net damage - which the bonus arm makes rare but not impossible -
compares above any HP, the clamp rewrites it to the victim's whole bar, and
the victim dies. A negative roll on these modules kills outright rather than
healing.

**Shape B - clamp to `HP - 1`, floor 1, signed.** Two sites, and both are a
module's move-VM **stager**: `0x801F8758` in PROT 0927 and `0x801F8F08` in
PROT 0966. Both images' tick bodies clamp shape A, so "0927 / 0966 never kill"
is true of the sweep and false of the tick:

```text
v0 = victim[+0x14C]
v1 = v0 - 1
slt v0, v1, dmg         ; SIGNED
if v0 { dmg = v1 }
victim[+0x10]  += dmg
victim[+0x14C] -= dmg
```

Here the comparison is **signed**, so a negative roll passes unclamped and the
subtract raises HP; and the cap is `HP - 1`, so neither sweep can kill - a
live seat is left at 1 HP at worst.

**Shape C - clamp to `HP - 1`, unsigned.** Shape B's cap with shape A's
comparison. This page previously said the band had no third shape - "every
tick body in the band, whole-row sweeps included, takes shape A" - and that
sentence is **false**: seven sites take shape C, and every one of them is a
tick body or a body a tick calls, never a stager.

```text
v0 = victim[+0x14C]
v1 = v0 - 1
sltu v0, v1, dmg        ; UNSIGNED, against the HP-1 cap
if v0 { dmg = v1 }
```

| Wrapper site | owner | routine |
|---|---|---|
| `0x801F76CC` | 908 `summon_zenoir` | tick `0x801F69D8`, the `/4` splash arm |
| `0x801F7108` | 915 `summon_mushura` | tick `0x801F69D8` |
| `0x801F8114` | 928 `summon_palma` | tick `0x801F69F4` |
| `0x801F7E4C` | 929 `summon_mule` | tick `0x801F69FC`; the apply is 117 instructions later at `0x801F8020` |
| `0x801F7BA0` | 932 `summon_meta` | tick `0x801F6A34` |
| `0x801F7CCC` | 933 `summon_terra` | tick `0x801F6A30` |
| `0x801F8E04` | 934 `summon_ozma` | tick `0x801F6A40`; apply at `0x801F8F9C` |

Shape C can neither kill nor heal: a negative roll reads as a huge unsigned
and is rewritten to `HP - 1`, which is the one outcome shapes A and B each get
wrong in opposite directions. The two far-apart sites are why a census has to
follow the roll register to its apply rather than window the call: 0929 and
0934 park the return in a saved register across a branch first.

**One site picks its cap at run time.** PROT 0910's applier `0x801F81DC`
counts its slashes in the module word `0x801F8DAC` and chooses the cap from
that count - `HP - 1` on slashes 1..3 (`0x801F88D8` / `0x801F88E4`), live HP
on slash 4 (`0x801F88E8`) - with one `sltu` at `0x801F88F0` either way. Kill
capability there is per **hit**, not per module, so a damage-shape table keyed
on the routine is one level too coarse for it.

### The two AoE sweeps

Those same two routines are the band's only row-wide appliers **among the
stagers**, and they are `0x801F6734` stagers, not tick bodies - the move
script drives them through move-VM opcode `0x20`, so the damage lands from the
spawn stager and not from the `ctx+0x279` machine. Three *tick* bodies sweep a
row too, and none of them shares the never-kill clamp -
[below](#the-twelve-bodies-the-trampoline-map-names).

| | PROT 0927 (Juggernaut) | PROT 0966 (Evil Seru Magic) |
|---|---|---|
| seats swept | `actor_table[3 ..]`, the enemy row | `actor_table[0 ..]`, the party row |
| bound | `ctx[+1]` (monster count) | `ctx[+0]` (**party** count) |
| skips | `+0x14C == 0`, `+0x16E & 4` | the same two |
| wrapper | `FUN_801DD0AC(0x12, 7, seat)` | `FUN_801DD4B0(0x100, ctx[+0x13], seat)` |
| also writes | - | `+0x1DA = +0x1F1`, `+0x1DC += 1`, `+0x21D = 2` |

So Cort's ESM hits the whole **party**, stages each victim's own knockdown
reaction, and drops every hit seat into slow motion. The two sweeps partition
the eight-slot table rather than overlapping on it.

#### `ctx[+0]` is the party count, not the actor count

The two bytes bound disjoint halves of `actor_table`, and both readings are
pinned by what their consumers index:

- **`ctx[+1]` = monster count.** The two party-wipe sweeps read it as the loop
  bound (`lbu a1,1(ctx)` at `0x8004B10C`, `lbu v0,1(ctx)` at `0x8005039C`) and
  index `actor_table[(i + 3)]` (`addiu v0,v0,3; sll v0,v0,2` at `0x8004B12C` /
  `0x800503B4`) - the enemy row.
- **`ctx[+0]` = party count.** `0x8004B3F0` reads it as the bound of a loop
  whose body indexes `DAT_8007BD10[i]`, the per-seat **1-based party character
  id**, and turns it into a `0x414`-byte party-record address:
  `v1 = DAT_8007BD10[i] - 1`, then the shift/add chain at
  `0x8004B430..0x8004B444` multiplies by `0x414` and adds `0x80084140`, reading
  `+0x6C0` off it. Only the party seats have such a record, so the bound cannot
  be the eight-slot actor count.

Engine mirror: `legaia_engine_vm::cast_module_ticks::CastModuleCtx::party_count`,
seeded at `World::cast_module_ctx` from the engine's present-party list
(`PartyState::party_count`) clamped to the party row, not from the actor
table's length.

### The seat-0 hardcode, and where it does not hold

**The victim load is hardcoded to seat 0 in the three decoded exemplars.**
Every apply site in them loads `actor_table[0]` (`lw rX, 0x9370(base)`)
instead of the derived victim: twelve sites in 958 (six clamp/write pairs),
five in 959, two in 960 (`+0x17AC`/`+0x17DC`). Retail never notices, because a
boss cinematic's victim is always the party - seat 0 - but any reuse that
points the cast at a monster (or any multi-target future) inherits friendly
fire from these sites. The same seat-0 assumption shapes the finale: a
dead-victim arm declares game over on the spot, correct only while the victim
is a hero.

It is **not** a band-wide rule, though, and the sweeps above are the
counter-example: both index the actor table by their own loop counter and pass
that seat to the wrapper as `a2`.

## The cast's own CD-XA voice

**A cast's voice is the module's own hardcoded cue, not a character-banded
one.** Near its head every module in `0903..=0966` calls the cue dispatcher
`FUN_8004FCC8` (a few reach the battle sound funnel `FUN_8004FE5C` instead)
with a **literal** id in `$a0`, and that id is what selects the `XA*.XA` file,
the sector-filter channel and the read span the CD-XA clip starter
`FUN_8003D53C` runs with. Nothing about the caster enters the choice: the same
module raises the same cue whichever character cast it.

That matters because the cast-audio dispatcher `FUN_801F3990` also emits a cue
band, and that band *is* character-split (`char_kind * 0x10 + 0xF8..0xFC`, plus
`0x20C..0x20E` on the enemy leg). The two are different cues on different
paths, and the module's own is the one a cast is heard through.

**A cast cannot reach the other band at all, and that is structural rather than
a sampling result.** `FUN_801F3990` has one caller, battle-SM state `0x3D`,
which is entered only from `0x3C`; the Magic category arm sets `0x3C` only when
the spell's class byte is `< 0x14` **and** its id is `< 0x65`, so the player
Seru block `0x81..0x8B` fails the id test outright. The Item arm stores `0x3C`
unconditionally, so an ordinary item use walks straight into the band - and
does, on every driven item action measured. The door is an item, not a spell.
Both halves in
[`battle-action.md`](battle-action.md#the-one-caller-is-state-0x3d-and-it-is-an-item--spirit-state).

### How a cue id becomes a file, a channel and a span

Straight off `FUN_8004FCC8` (`0x8004FCC8..0x8004FD7C` in `SCUS_942.54`):

* `sltiu v0, s0, 0x100` - ids below `0x100` leave on the SFX-queue path and
  never reach CD-XA. There is **no upper bound**.
* two decline gates sit ahead of the XA arm, so a cue can resolve and still
  play nothing: `ctx[+0x276] != 0` (the pointer is `gp+0xA0C`, which at the
  live `gp` is the battle context `0x8007BD24`), and `FUN_8003DE7C(1) != 0`.
  The battle sound funnel `FUN_8004FE5C` runs the same pair on its voice leg
  (`0x8004FE84..0x8004FEA4`). Neither is the module's own: `ctx[+0x276]` is
  the **side-band applier stage** - the per-turn `summon.dat` / `readef.DAT`
  streaming phase byte `FUN_801DABA4` seeds `1` every turn and `FUN_801F12D0`
  steps back to `0` ([`summon-readef.md`](../formats/summon-readef.md)), so
  no CD-XA clip starts while an ME archive is streaming; a summon module polls
  that byte itself before installing its actor record and raises its head cue
  only after (PROT 0903: `lbu v0,0x276(s1)` at `0x801F6CC0`, `jal 0x801F19EC`
  at `0x801F6D3C`, the cue at `0x801F6E50`). `FUN_8003DE7C(1)` is the read-span
  countdown `gp+0x91C` the starter arms with `dur` and each poll steps down by
  the frame-speed byte, plus the read-in-flight cells - a cast inside the
  previous clip's span plays no voice. The engine models the second gate
  (`AudioState::battle_xa_busy_frames`) and passes the first as `0`, its
  side-band being resident rather than streamed
  (`engine-vm::battle_cast_cue::admit_voice_cue`).
* `a1 = id - 0x100`; the clip slot is `a1 >> 3` with three remaps applied in
  sequence off that one value (`1 -> 0x1A`, `3 -> 0x1B`, `5 -> 0x1C`), and the
  runtime clip table names slot `n` as `XA<n + 1>.XA`.
* the channel is `andi a1, a1, 7`.
* the span is `(raw * 60 + 99) / 100` - written as `(raw << 4) - raw << 2`
  plus `0x63`, then the reciprocal `0x51EB851F` with `sra 5` - where `raw` is
  the `u16` at `0x800788B8 + (id - 0x100) * 2`. The pointer is formed
  `lui v1, 0x8008; addiu v1, v1, -0x7748; sll v0, a1, 1; addu v0, v0, v1`,
  with **no range check**, so the table is exactly as long as the ids reach.

Four of the band's own `FUN_8004FCC8` sites pass an id **below** that `0x100`
threshold and so raise an SFX cue rather than a voice: `0x22` at `0x801F88FC`
and `0x801F894C` and `0x21` at `0x801F8B54` in PROT 0957, and `0x56` at
`0x801F87BC` in PROT 0958. None of them is a head cue - every one of the 64 head
cues is `>= 0x130` - so a census that reads any dispatcher call in the band as a
voice cue counts these four wrongly.

Its real extent is `0x110` entries: index `0x110` is where ASCII text begins,
and the ids the cast band uses run to `0x20F`, i.e. index `0x10F`. A reader
that stops at `0x40` entries covers the menu/jingle band only and drops every
cast cue, the enemy leg's `0x20C..0x20E` included.

The resolved spans reproduce the demuxed per-channel clip lengths: `XA7.XA`
channels 0..6 carry the first seven summons' beds and their table spans match
the decoded audio to within 0.05 s on all seven (`capture`, N = 7 channels,
`extracted/XA_WAV`). Two live casts confirm the whole chain end to end - Vera
(`0905`) started `FUN_8003D53C(6, 1, 568)` and Gimard (`0903`)
`FUN_8003D53C(6, 4, 686)`, exactly the census rows below.

### Two modules pick their take at random

`0936` and `0937` are the only two that do not carry a constant: both call the
BIOS RNG (`jal 0x80056798`), reduce it to `v0 % 2`, and add the remainder to a
base - `addiu a0, v0, 0x1b0` in `0936` and `addiu a0, v0, 0x1b2` in `0937`. So
Hyper Crush speaks on `XA23.XA` channel 0 or 1 and Hyper Lightning on channel
2 or 3, one coin flip per cast. A backward scan for `addiu a0, zero, imm`
reports "no literal" on both, which is the shape to expect when the id rides a
reused register instead.

### Per-module cue census

The **head** cue of each image - the first dispatcher call inside the image's
own `content_bytes`, never its inherited tail. Several modules raise further
cues later (`0955` has six sites, `0962` four, `0941` seven counting the
funnel); those are the per-phase beds, not the cast's voice.

| PROT | image | site | cue | file | channel | span (vsyncs) |
|---|---|---|---|---|---|---|
| 903 | `summon_gimard` | `0x801F6E50` | `0x134` | `XA7.XA` | 4 | 686 (11.43s) |
| 904 | `summon_theeder` | `0x801F6CA8` | `0x136` | `XA7.XA` | 6 | 807 (13.45s) |
| 905 | `summon_stager_x83` | `0x801F6E70` | `0x131` | `XA7.XA` | 1 | 568 (9.47s) |
| 906 | `summon_gizam` | `0x801F6D08` | `0x133` | `XA7.XA` | 3 | 1141 (19.02s) |
| 907 | `summon_nighto` | `0x801F71F4` | `0x135` | `XA7.XA` | 5 | 960 (16.00s) |
| 908 | `summon_zenoir` | `0x801F6E30` | `0x132` | `XA7.XA` | 2 | 903 (15.05s) |
| 909 | `summon_viguro` | `0x801F6C20` | `0x130` | `XA7.XA` | 0 | 1278 (21.30s) |
| 910 | `summon_swordie` | `0x801F6CEC` | `0x160` | `XA13.XA` | 0 | 898 (14.97s) |
| 911 | `summon_orb` | `0x801F6BC8` | `0x161` | `XA13.XA` | 1 | 935 (15.58s) |
| 912 | `summon_freed` | `0x801F6C2C` | `0x162` | `XA13.XA` | 2 | 1392 (23.20s) |
| 913 | `summon_nova` | `0x801F6D1C` | `0x163` | `XA13.XA` | 3 | 1630 (27.17s) |
| 914 | `summon_gola_gola` | `0x801F6CA0` | `0x164` | `XA13.XA` | 4 | 1281 (21.35s) |
| 915 | `summon_mushura` | `0x801F6D10` | `0x165` | `XA13.XA` | 5 | 1137 (18.95s) |
| 916 | `summon_aluru` | `0x801F6F34` | `0x166` | `XA13.XA` | 6 | 1400 (23.33s) |
| 917 | `summon_barra` | `0x801F6DE8` | `0x168` | `XA14.XA` | 0 | 1578 (26.30s) |
| 918 | `summon_kemaro` | `0x801F6F80` | `0x169` | `XA14.XA` | 1 | 1953 (32.55s) |
| 919 | `summon_spoon` | `0x801F6DA0` | `0x16a` | `XA14.XA` | 2 | 941 (15.68s) |
| 920 | `summon_slippery` | `0x801F6CF4` | `0x16b` | `XA14.XA` | 3 | 1152 (19.20s) |
| 921 | `summon_iota` | `0x801F6D70` | `0x16c` | `XA14.XA` | 4 | 1233 (20.55s) |
| 922 | `summon_puera` | `0x801F6D5C` | `0x16d` | `XA14.XA` | 5 | 1438 (23.97s) |
| 923 | `summon_gilium` | `0x801F6CC4` | `0x16e` | `XA14.XA` | 6 | 2208 (36.80s) |
| 924 | `stager_ultimate_rave` | `0x801F6D10` | `0x189` | `XA18.XA` | 1 | 1713 (28.55s) |
| 925 | `summon_spikefish` | `0x801F6D5C` | `0x188` | `XA18.XA` | 0 | 1487 (24.78s) |
| 926 | `summon_stager_x98` | `0x801F6D5C` | `0x188` | `XA18.XA` | 0 | 1487 (24.78s) |
| 927 | `summon_juggernaut` | `0x801F6D94` | `0x177` | `XA15.XA` | 7 | 2590 (43.17s) |
| 928 | `summon_palma` | `0x801F6D9C` | `0x171` | `XA15.XA` | 1 | 2771 (46.18s) |
| 929 | `summon_mule` | `0x801F6E34` | `0x170` | `XA15.XA` | 0 | 2706 (45.10s) |
| 930 | `summon_horn` | `0x801F6DA0` | `0x173` | `XA15.XA` | 3 | 1857 (30.95s) |
| 931 | `summon_jedo` | `0x801F6D5C` | `0x175` | `XA15.XA` | 5 | 2890 (48.17s) |
| 932 | `summon_meta` | `0x801F6DF8` | `0x172` | `XA15.XA` | 2 | 2789 (46.48s) |
| 933 | `summon_terra` | `0x801F6D60` | `0x174` | `XA15.XA` | 4 | 2844 (47.40s) |
| 934 | `summon_ozma` | `0x801F6D60` | `0x176` | `XA15.XA` | 6 | 2532 (42.20s) |
| 935 | `cast_earthquake` | `0x801F6C10` | `0x19c` | `XA20.XA` | 4 | 583 (9.72s) |
| 936 | `cast_hyper_crush` | `0x801F6B30` | `0x1b0/0x1b1` | `XA23.XA` | 0/1 | 705/839 (11.75/13.98s) |
| 937 | `cast_hyper_lightning` | `0x801F6AFC` | `0x1b2/0x1b3` | `XA23.XA` | 2/3 | 735/696 (12.25/11.60s) |
| 938 | `cast_chaos_breath` | `0x801F6AA4` | `0x1ac` | `XA22.XA` | 4 | 792 (13.20s) |
| 939 | `cast_spore_gas` | `0x801F6BB8` | `0x152` | `XA11.XA` | 2 | 522 (8.70s) |
| 940 | `cast_glare_divide` | `0x801F6AEC` | `0x1b7` | `XA23.XA` | 7 | 539 (8.98s) |
| 941 | `cast_steal` | `0x801F6AB0` | `0x155` | `XA11.XA` | 5 | 783 (13.05s) |
| 942 | `cast_power_up` | `0x801F6A9C` | `0x1b6` | `XA23.XA` | 6 | 1355 (22.58s) |
| 943 | `cast_curse` | `0x801F6A9C` | `0x1c3` | `XA25.XA` | 3 | 1061 (17.68s) |
| 944 | `cast_guilty_cross` | `0x801F6B3C` | `0x1ad` | `XA22.XA` | 5 | 879 (14.65s) |
| 945 | `cast_water_column` | `0x801F6AA8` | `0x19d` | `XA20.XA` | 5 | 441 (7.35s) |
| 946 | `cast_call_wave` | `0x801F6C28` | `0x151` | `XA11.XA` | 1 | 604 (10.07s) |
| 947 | `cast_v_windhash` | `0x801F6AEC` | `0x19e` | `XA20.XA` | 6 | 375 (6.25s) |
| 948 | `cast_cross_beam` | `0x801F6CB0` | `0x148` | `XA10.XA` | 0 | 411 (6.85s) |
| 949 | `cast_water_crystals` | `0x801F6E20` | `0x145` | `XA9.XA` | 5 | 597 (9.95s) |
| 950 | `cast_rolling_flare` | `0x801F6AC8` | `0x1a8` | `XA22.XA` | 0 | 1388 (23.13s) |
| 951 | `cast_chaos_flare` | `0x801F6C60` | `0x1b5` | `XA23.XA` | 5 | 1288 (21.47s) |
| 952 | `cast_bloody_horns` | `0x801F6E88` | `0x15f` | `XA12.XA` | 7 | 392 (6.53s) |
| 953 | `cast_terio_punch` | `0x801F6CDC` | `0x15a` | `XA12.XA` | 2 | 602 (10.03s) |
| 954 | `cast_fatal_decision` | `0x801F6D74` | `0x149` | `XA10.XA` | 1 | 540 (9.00s) |
| 955 | `cast_white_shield` | `0x801F6B0C` | `0x157` | `XA11.XA` | 7 | 359 (5.98s) |
| 956 | `cast_water_hazard` | `0x801F6AB4` | `0x15b` | `XA12.XA` | 3 | 783 (13.05s) |
| 957 | `summon_effect_table` | `0x801F6AF8` | `0x15c` | `XA12.XA` | 4 | 1012 (16.87s) |
| 958 | `cast_blazing_slash` | `0x801F6E9C` | `0x198` | `XA20.XA` | 0 | 1192 (19.87s) |
| 959 | `cast_megaton_press` | `0x801F6B80` | `0x199` | `XA20.XA` | 1 | 1437 (23.95s) |
| 960 | `cast_plasma_strike` | `0x801F6AC4` | `0x15d` | `XA12.XA` | 5 | 849 (14.15s) |
| 961 | `cast_dead_end_crisis` | `0x801F6AD4` | `0x1c1` | `XA25.XA` | 1 | 1224 (20.40s) |
| 962 | `cast_blade_breath` | `0x801F6A90` | `0x1c0` | `XA25.XA` | 0 | 767 (12.78s) |
| 963 | `cast_genocidal_cannon` | `0x801F6AD4` | `0x1b4` | `XA23.XA` | 4 | 1828 (30.47s) |
| 964 | `cast_element_change` | `0x801F6C1C` | `0x1c4` | `XA25.XA` | 4 | 1202 (20.03s) |
| 965 | `cast_doomsday` | `0x801F6B0C` | `0x1c2` | `XA25.XA` | 2 | 1910 (31.83s) |
| 966 | `cast_evil_seru_magic` | `0x801F6B1C` | `0x1ae` | `XA22.XA` | 6 | 3269 (54.48s) |
`0925` and `0926` share a cue because `0926` is the null-stager sibling of
`0925` (see [Six of the 64 stagers are a null
routine](#six-of-the-64-stagers-are-a-null-routine)); `0936` / `0937` show both
arms of their coin flip.

## What of the choreography is data, and what is code

A signature cast is **half data**. Its particle layer is a record in exactly the
format a player art already names by id; its lift and its camera are the
module's own instructions and nothing outside the module can reach them.

**The spawn layer is data, in the art path's own format.** Each module reaches
the pool spawner `FUN_80050ED4` from hardcoded `jal` sites - 15 in PROT `0958`,
41 in `0959`, 24 in `0960` - and at every one of them `a2` is a **constant
module-resident pointer** and `a3` is a scale literal (`0x1000` at every 958 and
960 site; 959 also uses `0x0C00` four times and `0x0800` once). The pointers land
in each module's data band and nowhere else: `0x801F8EB8..0x801F9348` in 958 (14
distinct records for 15 sites), `0x801F884C..0x801F95CC` in 959 (44 for 41 - a
site that `switch` arms jump to receives one pointer per arm), and
`0x801F8768..0x801F8E0C` in 960 (23 for 24). An earlier count of 13, 41 and 21
came from a resolver that read only a `lui`/`addiu` pair above the call and so
missed the pointers completed in a delay slot or loaded in an arm
([`slot-b-module-layout.md`](../formats/slot-b-module-layout.md#resolving-the-pointer-a-spawn-call-is-handed)).
What they point at is the **summon
part-record shape** the whole spawn stack shares - `[i16 model_sel][u16 reserved]
[move-VM bytecode]`, `model_sel = -1` on the pure-transform records, a real index
on the modelled ones (960's `0x801F8CB8` carries `27`) - i.e. byte-for-byte the
shape of the art path's own effect prototypes
([move-power.md](../formats/move-power.md#effect-prototype-records---the-spawn-path)).

**The art path is id-driven over the same shape.** A move's effect-list byte
`0x01..=0x63` spawns `0x801F6324[id]` through the same `FUN_80050ED4` at scale
`0x1000` and copies the CLUT row `0x801F6418[id]`. That table is 61 entries
(`(0x6418 - 0x6324) / 4`), **every one populated** - no null slot to claim - and
all 61 resolve to 54 distinct records in the battle overlay's own data band
`0x801F5484..0x801F62C0`, seven ids aliasing a shared record (`00/30/31`,
`04/05/06`, `0E/22/23/24`). So the art path can already name a record of this
shape; what it cannot do is name one of *these* records.

**Why not: the module's band is not resident when an art plays.** The three
modules link at slot-B base `0x801F69D8` and their parameter blocks all sit above
it, whereas the prototype table and its 54 records sit **below** it, inside the
battle overlay that is resident for the whole fight. A `0x801F6324` entry pointing
at `0x801F8EB8` would read whatever occupies slot B at that moment. A duplicated
block therefore has to be copied into overlay-resident space, and the battle
overlay has **247 bytes** of it: two zero runs of 64 bytes or more in the whole
`0x28800`-byte image, `131` B at `0x801F4FC3` and `116` B at `0x801F6960` (the
latter being the tail immediately below the slot-B base). That fits one small
block, not a move's worth.

**And the rest is not in the record at all.** The lift - the victim's knockdown
and the caster's clip chain - is clip staging written by module instructions:
`sb` into the staged-action byte `+0x1DA` 16 / 6 / 8 times across 958 / 959 / 960,
each paired with a `+0x1DC` restage bump (15 / 7 / 7), read back through the
`+0x1D9` confirm gates and the victim's `+0x1F1` reaction map. The camera is the
same shape - a `ctx+0x279` phase arm, not a record field. A spawn record is passed
`(world_pos, src_pos, record, scale)` and has no way to express either.

**Verdict (thread closed).** *Partially.* Reskinning a signature move's **fire**
is a data edit the art path can already drive: duplicate the parameter block into
overlay-resident space and repoint one `0x801F6324` id at it - with the caveats
that no id is free, so an alias group must be split or an existing id retargeted,
and that only ~131 contiguous bytes of overlay slack exist without moving
something. Its **lift and camera are code**: they live in the module's phase
machine, reachable only through the capture-class `0x63` action arm that pages
PROT `935..966`, and no eight-record art effect script can name them. A full
reskin is a module edit, not a data edit.

## The band as a port worklist

Every address the port catalog still lists in this band is one of the two
routines PROT 0898 names per module, a body one of those two reaches, or an
artefact. There is no third population: the worklist addresses resolve into
`0x801F6734` stager entries, six tick bodies reached from a module's own
trampoline, and one framed routine nothing references.

One shape sits outside the table below because the frame partition cannot see
it: a **frameless leaf reached only through a module's own jump table**. PROT
0949's stager is the case - `0x801F75BC` has no prologue at all, bounds its
operand with `sltiu v1, a1, 8`, forms the table base with
`lui v0, 0x801F; addiu v0, v0, 0x69F0` and `jr`s through it. The table is
eight words, `0x801F69F0..0x801F6A0C` **inclusive** (`0x801F6A10` is the next
routine's `addiu sp, sp, -0x50`, not a ninth arm), and the arms are one
eight-step ramp writing the victim's tint `+0x0C` (`0x200`..`0x1000`) and anim
rate `+0x21D` (`7`..`0`). Seven of the eight are 20-byte leaves ending in
`jr ra` with the `sb` in its delay slot (arm 0 at `0x801F761C` is the
fall-through immediately past the `jr`); arm 7 at `0x801F76A8` is twelve bytes
with **no** `jr ra` of its own and falls into the shared epilogue at
`0x801F76B4`, which is also where the out-of-range `beqz` lands. A routine reached only through a table is
still a routine, and eight of them are still eight.

The verdict column says what a port owes each one, and the port has now
acted on every row - see [what the port runs](#what-the-port-runs) for where
each verdict landed. **DATA** means the routine
is an arm switch on the move-VM operand `a1` whose arms do nothing but call
`FUN_80021B04` / `FUN_80050ED4` / `FUN_801DFDF0` / `FUN_80024E80` with a
module-resident record pointer and a scale literal - the spawn-record data
layer, expressible without code. **PORT** means the routine reads or writes
simulation state: a damage roll through `FUN_801DD0AC` / `FUN_801DD4B0` /
`FUN_801DD6B4` with the HP clamp, the staged-clip bytes, the module phase
`ctx+0x279`, `ctx+0x278`, or the victim's own fields. **SCOPE-IGNORE** means
there is nothing there - six modules' stagers are a bare `jr ra` + `nop`, and
one address is a routine no table names and no image references.

"Owner" is the image whose PROT 0898 table row or whose own trampoline names
the VA, established from the bytes, not from a dump's filename. Where the
same VA carries a byte-identical routine in another image, the "also in"
column says so; those copies are residue (below), not second call sites.

**One of those "also in" cells was not residue.** Six of the seven pairs check
out byte-for-byte at the same file offset - `0x801F8EAC` (904 in 908 and 910),
`0x801F7F2C` (944 in 945), `0x801F8118` (942 in 943), `0x801F8504` (948 in
949), `0x801F8578` (919 in 920) and `0x801F86B0` (960 in 961), each run ending
exactly at the shorter image's length. `0x801F81DC` does not: PROT 0910's
words there differ from PROT 0951's in the first instruction, and the identity
run around the address is three bytes long. They are **two different
routines** at one VA - 0951's 272-byte spawn stager and 0910's 2040-byte
damage applier - so the table carries both, and a "DATA" verdict read off the
address alone would have credited the applier to the spawn pool.

| VA | owner | what the routine is | also present in | verdict |
|---|---|---|---|---|
| `801F6A14` | 957 (`summon_effect_table`) | cast tick body, reached from the module trampoline; damage roll (resist); writes HP `+0x14C`, staged `+0x1DA`, restage `+0x1DC`, `ctx+0x278`, phase `ctx+0x279` | - | **PORT** |
| `801F74E4` | 960 (`cast_plasma_strike`) | cast tick body, reached from the module trampoline; damage roll (bypass); writes `+0x0C`, HP `+0x14C`, staged `+0x1DA`, restage `+0x1DC`, `ctx+0x278`, phase `ctx+0x279` | - | **PORT** |
| `801F798C` | 957 (`summon_effect_table`) | cast tick body, reached from the module trampoline; writes `+0x0C`, HP `+0x14C`, staged `+0x1DA`, restage `+0x1DC`, phase `ctx+0x279` | - | **PORT** |
| `801F81DC` | 951 (`cast_chaos_flare`) | spawn stager, 4 spawn calls | - | **DATA** |
| `801F81DC` | 910 (`summon_swordie`) | per-slash damage applier, reached by three `jal`s from the tick; rolls `FUN_801DD0AC(0x12, 7)`, picks its cap from the module's slash counter, writes HP `+0x14C`, staged `+0x1DA`, restage `+0x1DC` | - | **PORT** |
| `801F8EAC` | 904 (`summon_theeder`) | spawn stager, 1 spawn call | also in 908,910 | **DATA** |
| `801F6A0C` | 952 (`cast_bloody_horns`) | cast tick body, 5 phase arms; writes staged `+0x1DA`, restage `+0x1DC`, anim rate `+0x21D`, phase `ctx+0x279` | - | **PORT** |
| `801F6DD8` | 958 (`cast_blazing_slash`) | cast tick body, reached from the module trampoline; damage roll (bypass); writes HP `+0x14C`, staged `+0x1DA`, restage `+0x1DC`, `ctx+0x278` | - | **PORT** |
| `801F6EDC` | 945 (`cast_water_column`) | cast tick body, reached from the module trampoline; damage roll (resist); writes HP `+0x14C`, status `+0x16E`, staged `+0x1DA`, restage `+0x1DC` | - | **PORT** |
| `801F7F2C` | 944 (`cast_guilty_cross`) | spawn stager, 3 spawn calls | also in 945 | **DATA** |
| `801F8118` | 942 (`cast_power_up`) | spawn stager, 2 spawn calls | also in 943 | **DATA** |
| `801F8504` | 948 (`cast_cross_beam`) | spawn stager, 1 spawn call | also in 949 | **DATA** |
| `801F8578` | 919 (`summon_spoon`) | spawn stager, 4 spawn calls | also in 920 | **DATA** |
| `801F86B0` | 960 (`cast_plasma_strike`) | spawn stager, 2 spawn calls | also in 961 | **DATA** |
| `801F74B4` | 939 (`cast_spore_gas`) | null stager - `jr ra` + `nop`, the whole routine | - | **SCOPE-IGNORE** |
| `801F75BC` | 949 (`cast_water_crystals`) | spawn stager, `sltiu a1, 8`, 0 spawn calls; writes the **victim**'s `+0x0C` and anim rate `+0x21D` | - | **PORT** |
| `801F769C` | 943 (`cast_curse`) | spawn stager, 3 spawn calls | - | **DATA** |
| `801F76C4` | 946 (`cast_call_wave`) | spawn stager, 5 spawn calls | - | **DATA** |
| `801F7740` | 906 (`summon_gizam`) | spawn stager, `sltiu a1, 7`, 3 spawn calls; writes `+0x0C`, phase `ctx+0x279` | - | **PORT** |
| `801F776C` | 945 (`cast_water_column`) | spawn stager, 2 spawn calls | - | **DATA** |
| `801F7820` | 924 (`stager_ultimate_rave`) | spawn stager, 1 spawn call | - | **DATA** |
| `801F7850` | 937 (`cast_hyper_lightning`) | spawn stager, 5 spawn calls | - | **DATA** |
| `801F78A4` | 961 (`cast_dead_end_crisis`) | spawn stager, 6 spawn calls | - | **DATA** |
| `801F78F8` | 947 (`cast_v_windhash`) | null stager - `jr ra` + `nop`, the whole routine | - | **SCOPE-IGNORE** |
| `801F7948` | 909 (`summon_viguro`) | framed routine no table names and nothing references | - | **SCOPE-IGNORE** |
| `801F7A80` | 914 (`summon_gola_gola`) | spawn stager, `sltiu a1, 6`, 2 spawn calls | - | **DATA** |
| `801F7AB8` | 938 (`cast_chaos_breath`) | spawn stager, 3 spawn calls | - | **DATA** |
| `801F7AE8` | 925 (`summon_spikefish`) | spawn stager, 3 spawn calls | - | **DATA** |
| `801F7AF4` | 909 (`summon_viguro`) | spawn stager, `sltiu a1, 7`, 2 spawn calls; writes `+0x0C`, staged `+0x1DA`, target `+0x1DD`, phase `ctx+0x279` | - | **PORT** |
| `801F7B74` | 965 (`cast_doomsday`) | spawn stager, 2 spawn calls | - | **DATA** |
| `801F7BA0` | 952 (`cast_bloody_horns`) | null stager - `jr ra` + `nop`, the whole routine | - | **SCOPE-IGNORE** |
| `801F7BD0` | 936 (`cast_hyper_crush`) | spawn stager, 4 spawn calls | - | **DATA** |
| `801F7DB0` | 941 (`cast_steal`) | spawn stager, `sltiu a1, 5`, 2 spawn calls | - | **DATA** |
| `801F7EA4` | 930 (`summon_horn`) | spawn stager, `sltiu a1, 7`, 8 spawn calls | - | **DATA** |
| `801F7EC4` | 956 (`cast_water_hazard`) | spawn stager, 5 spawn calls | - | **DATA** |
| `801F7FA8` | 907 (`summon_nighto`) | spawn stager, 2 spawn calls | - | **DATA** |
| `801F7FE8` | 911 (`summon_orb`) | spawn stager, 1 spawn call | - | **DATA** |
| `801F800C` | 921 (`summon_iota`) | spawn stager, 2 spawn calls | - | **DATA** |
| `801F8078` | 905 (`summon_stager_x83`) | spawn stager, 2 spawn calls | - | **DATA** |
| `801F813C` | 962 (`cast_blade_breath`) | spawn stager, 3 spawn calls | - | **DATA** |
| `801F81A0` | 963 (`cast_genocidal_cannon`) | spawn stager, 4 spawn calls | - | **DATA** |
| `801F81E8` | 920 (`summon_slippery`) | null stager - `jr ra` + `nop`, the whole routine | - | **SCOPE-IGNORE** |
| `801F8208` | 950 (`cast_rolling_flare`) | spawn stager, 1 spawn call | - | **DATA** |
| `801F8250` | 959 (`cast_megaton_press`) | spawn stager, `sltiu a1, 6`, 11 spawn calls | - | **DATA** |
| `801F82CC` | 940 (`cast_glare_divide`) | null stager - `jr ra` + `nop`, the whole routine | - | **SCOPE-IGNORE** |
| `801F82D8` | 917 (`summon_barra`) | spawn stager, `sltiu a1, 5`, 3 spawn calls | - | **DATA** |
| `801F8310` | 908 (`summon_zenoir`) | spawn stager, 5 spawn calls | - | **DATA** |
| `801F835C` | 912 (`summon_freed`) | spawn stager, 3 spawn calls | - | **DATA** |
| `801F84A4` | 932 (`summon_meta`) | spawn stager, 1 spawn call | - | **DATA** |
| `801F85A8` | 927 (`summon_juggernaut`) | spawn stager, `sltiu a1, 9`, 7 spawn calls; damage roll (shared); writes HP `+0x14C` | - | **PORT** |
| `801F85D4` | 954 (`cast_fatal_decision`) | null stager - `jr ra` + `nop`, the whole routine | - | **SCOPE-IGNORE** |
| `801F864C` | 913 (`summon_nova`) | spawn stager, `sltiu a1, 6`, 3 spawn calls | - | **DATA** |
| `801F8748` | 933 (`summon_terra`) | spawn stager, 2 spawn calls | - | **DATA** |
| `801F88F8` | 916 (`summon_aluru`) | spawn stager, `sltiu a1, 8`, 7 spawn calls | - | **DATA** |
| `801F89D4` | 910 (`summon_swordie`) | spawn stager, `sltiu a1, 5`, 1 spawn call | - | **DATA** |
| `801F8ADC` | 931 (`summon_jedo`) | spawn stager, 1 spawn call | - | **DATA** |
| `801F8B90` | 923 (`summon_gilium`) | spawn stager, 3 spawn calls; writes `ctx+0x278` | - | **PORT** |
| `801F8BF8` | 964 (`cast_element_change`) | spawn stager, 2 spawn calls | - | **DATA** |
| `801F8C30` | 929 (`summon_mule`) | spawn stager, `sltiu a1, 9`, 10 spawn calls | - | **DATA** |
| `801F8D30` | 958 (`cast_blazing_slash`) | spawn stager, 7 spawn calls | - | **DATA** |
| `801F8D64` | 966 (`cast_evil_seru_magic`) | spawn stager, `sltiu a1, 9`, 7 spawn calls; damage roll (resist); writes HP `+0x14C`, staged `+0x1DA`, restage `+0x1DC` | - | **PORT** |
| `801F8E68` | 928 (`summon_palma`) | spawn stager, `sltiu a1, 7`, 12 spawn calls | - | **DATA** |
| `801F90E4` | 922 (`summon_puera`) | spawn stager, 0 spawn calls; writes `ctx+0x278` | - | **PORT** |
| `801F92AC` | 934 (`summon_ozma`) | spawn stager, 11 spawn calls | - | **DATA** |
| `801F7F34` | 915 (`summon_mushura`) | spawn stager, two arms sharing one `jal` and two record pointers | - | **DATA** |
| `801F7FF0` | 935 (`cast_earthquake`) | spawn stager, arm 0 only, 1 spawn call | - | **DATA** |
| `801F9370` | 955 (`cast_white_shield`) | spawn stager, 1 spawn call | - | **DATA** |
| `801F99F4` | 957 (`summon_effect_table`) | spawn stager, `sltiu a1, 5`, 3 spawn calls | - | **DATA** |

### Six of the 64 stagers are a null routine

PROT 0920, 0939, 0940, 0947, 0952 and 0954 answer the move-VM's opcode-`0x20`
call with eight bytes - `jr ra` in the `0x801F6734` row's first word, `nop` in
the delay slot - and the six are byte-identical. Those spells stage nothing
from the move script; whatever they put on screen, the tick puts there. PROT
0903's row (`0x801F771C`) and PROT 0926's (`0x801F69D8`) are the same routine
and are not on the worklist only because no dump prints at those VAs.

### `0x801F7948` is reachable from nothing

PROT 0909 partitions into five framed functions. Two are the tick
(`0x801F69F4`) and the stager (`0x801F7AF4`) PROT 0898 names; the other three -
`0x801F7948`, `0x801F7CC8`, `0x801F7D30` - carry a real `addiu sp, sp, -F`
prologue under a clean epilogue and are reached by nothing. The five-form
sweep (`scripts/ghidra-analysis/find-address-word-refs.py`), re-run over
`SCUS_942.54` and all 83 mapped overlay images, reports no word, no `jal`, no
`j`, no PC-relative branch and no `lui`+`addiu` pair for any of the three, and
PROT 0909 holds no jump table that could reach them. Only `0x801F7948` is on
the worklist, because only it has a dump.

The sweep's only hits are **cross-image aliases**, and reading them as
references is the mistake the tool's `--home` flag exists to prevent: a
PC-relative branch inside PROT 0908 lands on `0x801F793C` and one inside PROT
0949 on `0x801F7CD8`, neither of which can leave its own image, and PROT
0958's 256-word head table carries `0x801F7D30` as one of its own arms. Three
independent runs of the sweep agree, which is worth saying because "no caller
found" and "no reference exists" are different claims and only the second one
closes a row.

### SCUS calls into slot B at one fixed VA - and only PROT 0920 arms it

`FUN_800480D8`, the per-actor battle draw tick, ends its scene-teardown
preamble with `jal 0x801F7B88` under `_DAT_8007BDC0 != 0`
(`lui v0,0x8008` / `lw v0,-0x4240(v0)` / `beq` at `0x8004818C..0x800481A0`,
`see ghidra/scripts/funcs/800480d8.txt`). The target is in the slot-B band, so
the `jal` alone names no image - sixty-five of the sixty-eight slot-B rows in
[`static-overlays.toml`](../../crates/asset/data/static-overlays.toml) are long
enough to hold a routine at base `+0x11B0`.

**The gate names the image.** `_DAT_8007BDC0` is `gp+0xAA8`, and a sweep for
every form that reaches it (`find-gp-relative-refs.py --va 0x8007BDC0`, which
covers the `lui`+load pair the five-form address scan cannot see) finds it in
exactly two images across `SCUS_942.54` and all 83 mapped overlays: SCUS, which
only ever stores zero to it (`0x80055BBC`), and **PROT 0920**
(`summon_slippery`, the Slippery / "Deadly Rain" evolved-Seru module), which
owns it end to end:

| Site | Instruction | Meaning |
|---|---|---|
| `0x801F6FEC` | `addiu v0,zero,0x204` ; `sw v0,-0x4240(v1)` | seeds the word to `0x204` inside the module's 64-iteration spawn loop |
| `0x801F70CC` | `sw v0,-0x4240(t0)` after `subu v0,v0,a0` | drains it per frame by `2 * byte[s0+0x7F]` |
| `0x801F7B4C` | `sw zero,-0x4240(v0)` | clears it, in the epilogue of the function that ends at `0x801F7B84` |

So the word is a live **budget**, not a boolean, and the routine SCUS calls
begins at the very next instruction after the function that clears it -
`0x801F7B88` is a 1632-byte framed routine at PROT 0920 file `+0x11B0`
(`addiu sp,sp,-0x60`, 408 instructions, reading the per-frame scalar
`_DAT_1F800393`). The reading that closes the loop: while Slippery's effect is
still draining, a battle scene that starts tearing down hands the module one
more tick before the teardown proceeds. Only the identity is measured; the
purpose is `inference`.

**Nothing in the save-state corpus has the gate up.** `_DAT_8007BDC0` reads
zero in all 176 catalogued states - every mednafen and PCSX-Redux backup in
[`scripts/scenarios.toml`](../../scripts/scenarios.toml), including
`slippery_summon_mid_cast`, the mid-cast capture of the very module that owns
the word. In that state PROT 0920 is byte-resident at slot B (8183 of 8192
bytes) and the sixteen bytes live at `0x801F7B88` are byte-equal to the image's
`+0x11B0`, so the target is pinned even though the call was not caught firing.

#### The arm is an in-battle frame path, not a teardown

The window the call needs is much wider than "a battle that ends mid-cast",
and the arm's own gates say so. `FUN_800480D8` reaches the gate only when the
teardown-request byte `gp[+0xA0C]->[+0x272]` is non-zero **and** the
battle-end signal `_DAT_8007BD71` reads `0xFF` - and `0xFF` is the *battle
running* value; `0xFE` is what the end sequence raises
([`battle-action.md`](battle-action.md)). The arm also consumes its own
request byte (`sb zero, 0x272(v0)` at `0x800481B0`), so it is a per-frame
one-shot rather than a phase.

Measured that way it is an ordinary in-battle path. An exec breakpoint on the
gate read `0x8004818C` across `rim_elm_gimard_victory`
(`scripts/pcsx-redux/autorun_slippery_budget_gate.lua`, 1800 vsyncs) enters
it **123 times**, once per rendered frame from the first captured frame to
vsync 323, every one with `_DAT_8007BD71 = 0xFF` - and then never again once
the victory raises `0xFE`. The battle end *closes* this arm instead of
opening it. `_DAT_8007BDC0` is zero at all 123 entries and is never written
in the run, so the call is blocked by the budget alone.

So the corrected residual: the call fires on any in-battle frame while PROT
0920's drain budget is still non-zero - i.e. **during** a Slippery cast, not
at a battle end during one - which makes it a per-frame tick of the module
while its effect drains rather than a teardown courtesy. What is still owed
is one PCSX-Redux state inside a Slippery cast: the corpus's only such state,
`slippery_summon_mid_cast`, is a **mednafen** backup, and mednafen has no
scriptable breakpoints, so the emulator that can watch the call cannot load
the state that has the gate up.

### A module image ends in another image's bytes

Every one of the 64 images ends in a byte-identical, same-file-offset run of
another extracted image, and the run always ends exactly at the shorter
image's own length. Nine of them end in **PROT 0899's** bytes - the menu
overlay, a slot-A image at a different base - which settles the direction:
the module inherited the bytes, mastered over a buffer that still held the
previous build. PROT 0926 is the limit case: 2040 of its 2048 bytes are PROT
0925's, and its own content is the eight-byte null stager at `0x801F69D8`.

Three consequences, and the first two have already put wrong claims in this
repo:

- **A dump's filename does not name the owner.** Seven worklist addresses were
  catalogued against an image that only holds the residue: `0x801F6A14` and
  `0x801F798C` (owner 0957, catalogued from 0964), `0x801F74E4` and
  `0x801F86B0` (0960, from 0961), `0x801F8118` (0942, from 0943), `0x801F8578`
  (0919, from 0920) and `0x801F8EAC` (0904, from 0910). The decisive test is
  the residual trampoline's own `jal` targets: PROT 0961's copy of
  `0x801F8638` calls `0x801F69D8` and
  `0x801F74E4`, and in 0961's *own* bytes `0x801F74E4` is interior to the tick
  body - so the block cannot be 0961's code.
- **The tail is not shared library code.** This page previously read the
  region past `~+0x2A00` as library code linked into several modules because
  958 and 959 hold the same words there. They hold the same words because both
  inherited them, and in the four 12288-byte images 904 / 912 / 917 / 918 the
  inherited words are PROT 0899's, at file offset `0x2A80` - a routine that
  materialises the live game-state window `0x80084140` and tail-jumps to
  `0x801D1298 + 0x8C`, addresses a cast module has no business reading. Its
  printed VA under the slot-B base, `0x801F9458`, names no routine at all.
- **The residue is what `disc-coverage.py` reports as un-dumped code.** Of the
  137 `ambiguous = no` runs of 64 bytes or more this band still carries, 103
  are the module's own data tail, 24 are byte-identical residue of a sibling
  module's real function, 4 are the PROT 0899 tail above, and 6 are interior
  to a tick body (PROT 0915, 0935 - the two images whose Ghidra import landed
  last, which is why the runs read as un-dumped rather than as interiors).
  Not one is an un-dumped function of the image it is filed under.

**A frame partition is not an own-content measure.** The partition finds a
prologue / epilogue pair wherever one exists in the bytes, and an inherited
tail carries the donor's, so an image can "contain" a function that is not
its own: PROT 0949 partitions into exactly two framed routines, and the
second, `0x801F8504`, is PROT **0948**'s move-VM stager sitting in 0949's
tail. The partition is also blind in the other direction - 0949's own stager
`0x801F75BC` is frameless (a `jr v0` table dispatch into eight leaves), so the
whole complex that spells the module's ramp is invisible to it. What bounds an
image's own content is the record chain's top, walked as move-VM programs
([slot-b-module-layout.md](../formats/slot-b-module-layout.md#bounding-the-highest-record)),
not the last frame the partition happens to match.

**The boundary is measured per image, so a module word below it is the
module's.** PROT 0912's tick reads and writes three module-local words at
`0x801F92A8` / `0x801F92AC` / `0x801F92B0`, inside the run
`0x801F8D2C..0x801F99D8` that the worklist files as inherited from PROT 0899.
There is no contradiction and nothing is mis-credited: the byte-identical run
with PROT 0899 is exactly file `+0x2908..+0x3000` (slot-B
`0x801F92E0..0x801F99D8`, 1784 bytes), which is where the run's `tail from`
column already puts the boundary. The three words sit at file `+0x28D0` /
`+0x28D4` / `+0x28D8`, `0x38` bytes **below** it, and that whole `0x38`-byte
window is zero in PROT 0912 while PROT 0899 carries code there - the image's
own zero scratch, already counted as its own data.

## What the port runs

The worklist's DATA/PORT split is also the port's split, and the engine runs
the DATA half of the whole band.

**The pool.** `legaia_asset::cast_effect_pool` indexes all 64 entries by PROT
number and parses each image's spawn records with the reader the spawn stack
already shares (`legaia_asset::summon_overlay::parse`, which scans both
`jal FUN_80021B04` and `jal FUN_80050ED4` sites and follows each one's `a2`).
The scene host builds it once (`ensure_cast_effect_pool`, PROT 0903..0966) and
installs it on the world; a host with no disc simply holds none.

**The key.** `legaia_engine_vm::battle_cast_dispatch`'s two dispatchers each
answer with the emitter VA *and* the band entry it lives in, using this page's
own arithmetic - `FUN_801F1ED4` row `id - 0x81` = PROT `903 + row`,
`FUN_801F2160` row `sub_id` = PROT `935 + sub_id`. `World::cast_module_for`
picks between them exactly as retail does, on the record's `+0` class byte:
capture class `'c'` goes to the `+1` row, anything else to the action-id row.

**The stage.** `World::spawn_cast_module_fx` seats the resolved module's
records as a move-VM scene (`SummonScene`), which is the same stand-in the
summon and move-FX paths run, so both hosts tick and draw them with no host
change. It fires at the two seams retail uses: the capture band's pager
(`load_capture_archive`, the `0x6E` arm, ahead of the `0x801E50C8` tick loop)
and the summon stager's first tick (`0x801E4B1C`).

**The code half.** The **PORT** rows - the six tick bodies and the seven
state-touching stagers - are `legaia_engine_vm::cast_module_ticks`, one
function per VA, and three sibling modules carry the rest of the band's code:
`cast_arm_ticks` (the fourteen trampoline arms of PROT 0940..0962),
`cast_seru_ticks_a` (PROT 0903..0908) and `cast_seru_ticks_b` (PROT
0909..0913, plus PROT 0910's applier `swordie_slash`). Each function carries
its routine's dispatch bound, its simulation-state writes, its damage step
(baked power, wrapper, clamp shape, `+0x10` accumulate, HP write, reaction
stage, anim-rate write) and the phase advance.
`World::run_cast_module_code` drives all four modules from the same seam
retail re-enters the paged module at - the stager tick the action SM calls at
states `0x34` / `0x35` / `0x36` - so a live cast in `play-window` or on the
browser play page reaches them.

What those functions deliberately leave out, and say so per item: the
GPU-packet arms, the camera arms, and - for the five tick bodies whose arm map
is a `beq` chain or a 256-entry table - the per-arm frame gating that decides
*when* each step fires. That gating is per-phase timing, pinned by capture and
not by the static window. Of the thirteen, four have a byte-recovered arm map
because their head is a word table: PROT 0906, 0909, 0949 (stagers) and 0952
(the Astral Slash tick, whose five arms are enumerated in the port).

The **damage** half is wired as a substitution rather than a second
application. For most of the band the engine still folds a cast's HP outcome
once, at `World::cast_spell_on_slots_prepaid`, but the magnitude that reaches
the wrapper is the module's own baked constant
(`cast_module_ticks::baked_power_for`, read by `World::baked_module_power`)
instead of the move-power table's scalar. PROT 0927 and PROT 0966 are the
exception, because their damage is not a per-target fold at all: those two
casts fold through `World::run_cast_module_aoe` and the generic path is
skipped, so the seat range and the `HP - 1` clamp are the module's.

PROT 0957 needs one more split: it carries **two** whole tick bodies, and its
trampoline `0x801F9BA8` picks between them on the caster's queued action id -
`0x76` to `0x801F798C`, `0x77` to `0x801F6A14`, anything else to the epilogue.
The port routes on the same two ids.

### The trampolines are their own port, and one cell holds six spells

The capture-class arm shape above is a routine in its own right, and **21 of
the 32** `0x801CF56C` arms are one - the other eleven (PROT 0935, 0936, 0937,
0939, 0946, 0947, 0948, 0949, 0953, 0954, 0966) point straight at a body.
Each trampoline is 88 to 204 bytes, opens `addiu sp, sp, -0x18`, materialises
the battle ctx `*0x8007BD24`, loads the caster `actor_table[ctx+0x13]` out of
`0x801C9370` and reads its queued action byte `caster[+0x1DF]`; an id the
routine does not name returns `a0 = 0`, so the module ticks nothing and the
drive loop proceeds. The port carries the whole map as data
(`cast_module_ticks::CAPTURE_TRAMPOLINES` / `capture_tick_body`), and
`crates/asset/tests/cast_module_data_rows_real.rs` re-derives each row off the
disc from PROT 0898's `0x801CF56C` arm and the trampoline's own `jal` set.

Between them the 21 trampolines name **48** `(action id -> body)` arms over 32
distinct bodies. A reader that stops at the module is one step short: eleven
cells hold more than one choreography, and a dispatcher keyed on the PROT
entry alone runs the wrong one for the cell's other ids.

| Owner | Trampoline | Action id -> tick body |
|---|---|---|
| 938 (`cast_chaos_breath`) | `0x801F7A40` | `0x4E` -> `0x801F726C`, `0xB7` -> `0x801F69EC` |
| 940 (`cast_glare_divide`) | `0x801F8228` | `0x3C` -> `0x801F69F8`, `0x50` / `0xAE` -> `0x801F78B8`, `0xAC` -> `0x801F7240` |
| 941 (`cast_steal`) | `0x801F7D38` | `0x51` -> `0x801F730C`, `0xB9` -> `0x801F6A04` |
| 942 (`cast_power_up`) | `0x801F80A0` | `0x52` -> `0x801F7D34`, `0xAA` -> `0x801F69F4` |
| 943 (`cast_curse`) | `0x801F7624` | `0x40` -> `0x801F6EF4`, `0xB5` -> `0x801F6A04` |
| 944 (`cast_guilty_cross`) | `0x801F7EBC` | `0x37` -> `0x801F6A04`, `0x53` -> `0x801F7470` |
| 945 (`cast_water_column`) | `0x801F76F4` | `0x54` -> `0x801F6EDC`, `0xBA` -> `0x801F69F8` |
| 950 (`cast_rolling_flare`) | `0x801F8190` | `0x5A` -> `0x801F79F8`, `0xAB` -> `0x801F6A24` |
| 951 (`cast_chaos_flare`) | `0x801F816C` | `0x36` -> `0x801F6A20`, `0x5B` -> `0x801F77E8` |
| 952 (`cast_bloody_horns`) | `0x801F7B28` | `0x5C` -> `0x801F7118`, `0xB8` -> `0x801F6A0C` |
| 955 (`cast_white_shield`) | `0x801F92A4` | six ids, [below](#prot-0955-is-a-six-spell-cell) |
| 956 (`cast_water_hazard`) | `0x801F7E4C` | `0x71` -> `0x801F7298`, `0x75` -> `0x801F69D8` |
| 957 (`summon_effect_table`) | `0x801F9BA8` | `0x76` -> `0x801F798C`, `0x77` -> `0x801F6A14` |
| 958 (`cast_blazing_slash`) | `0x801F8E60` | `0x79` -> `0x801F6DD8` |
| 959 (`cast_megaton_press`) | `0x801F87F4` | `0x7A` -> `0x801F69F0` |
| 960 (`cast_plasma_strike`) | `0x801F8638` | `0x7B` -> `0x801F74E4`, `0xA6` -> `0x801F69D8` |
| 961 (`cast_dead_end_crisis`) | `0x801F7A54` | `0xA1` and `0xB4` -> `0x801F69D8` |
| 962 (`cast_blade_breath`) | `0x801F8080` | `0xA2` -> `0x801F7AE4`, `0xA3` -> `0x801F74A0`, `0xA4` -> `0x801F6D54`, `0xA5` -> `0x801F69D8` |
| 963 (`cast_genocidal_cannon`) | `0x801F8438` | `0xB3` -> `0x801F6A20` |
| 964 (`cast_element_change`) | `0x801F8E3C` | `0xAF` -> `0x801F88EC`, `0xB0`..`0xB2` -> `0x801F69D8` |
| 965 (`cast_doomsday`) | `0x801F7B1C` | `0xB6` -> `0x801F69D8` |

#### A body VA is not a key - only `(entry, body)` is

Six of the trampolines send an id to **`0x801F69D8`**, which is the slot-B
load base itself: PROT 0956, 0960, 0961, 0962, 0964 and 0965. Those are six
different routines wearing one address, because each is its own image's word
0. `0x801F6A20` is likewise PROT 0951's Chaos Flare *and* PROT 0963's only
arm, and `0x801F6A04` is PROT 0941's, 0943's and 0944's second arm. Anything
that resolves a body - a dispatcher, a damage-shape lookup, a port tag - has
to carry the owning entry beside the VA.

Two of the arm maps also read against the grain of their `beq` chains. PROT
0940's `0x50` and `0xAE` both reach `0x801F78B8` (the `beq` at `0x801F825C`
and the `bne` at `0x801F8288` land on the same `jal`), and PROT 0964's second
body is reached by a **range** test rather than a compare - `slti v1, 0xaf`
then `slti v1, 0xb3` at `0x801F8E88`/`0x801F8E90` - so ids `0xB0`, `0xB1` and
`0xB2` share it.

#### PROT 0955 is a six-spell cell

`0x801F92A4` is the one trampoline in the band that dispatches through a jump
table rather than a `beq` chain: it bounds `id - 0x60` with `sltiu 0x14` and
indexes the module's **head table**, twenty words filling file `0x00..0x50`.
Fourteen of the twenty point at the shared epilogue `0x801F9360` and tick
nothing; the other six are whole choreographies - `0x60` -> `0x801F8F0C`,
`0x6E` -> `0x801F86A4`, `0x6F` -> `0x801F7FA4`, `0x70` -> `0x801F767C`,
`0x72` -> `0x801F7158`, `0x73` -> `0x801F6A28`.

That makes 0955 a **third** owner for a band head table. The rule
[above](#image-anatomy-recovered-from-the-bytes) resolves a head table as the
tick's or the stager's by reading the `sltiu` immediate; 0955's belongs to
neither, and its first function opens at file `+0x50` immediately past the
table.

### The six tick bodies PROT 0898's tables name and the worklist listed

Both arm tables reach bodies this page's verdict table did not cover, because
that table was built from the routines the dumps already printed at. Read off
each owning image's own bytes:

| Tick | Owner | Reached from | Phase arms | Damage |
|---|---|---|---|---|
| `0x801F6A00` | 925 (`summon_spikefish`) | `0x801CF4EC` row 22 | `sltiu a0, 0x0A` -> 10, table at file `+0` | none |
| `0x801F6A18` | 924 (`stager_ultimate_rave`) | row 21 | `sltiu a1, 0x0C` -> 12, table at `0x801F69E8` | none; the finale arm zeroes `+0x14C` outright |
| `0x801F6A3C` | 922 (`summon_puera`) | row 19 | `sltiu a0, 0x19` -> 25 | `FUN_801DD0AC(0x12, 7)` at `0x801F8E1C`, **shape A** |
| `0x801F6A84` | 927 (`summon_juggernaut`) | row 24 | `sltiu a1, 0x1D` -> 29 | `FUN_801DD0AC(0x12, 7)` at `0x801F7E0C`, **shape A** |
| `0x801F6C70` | 918 (`summon_kemaro`) | row 15 | `beq`/`slti` chain, literals `1 ..= 0x14` + `0xFF` | `FUN_801DD0AC` at `0x801F87A4`, **shape A** |
| `0x801F6A10` | 949 (`cast_water_crystals`) | `0x801CF56C` row 14 | `sltiu v1, 6` -> 6 | `FUN_801DD4B0(0xC0)` at `0x801F7318`, **shape A** |

Three of those refine claims elsewhere on this page.

- **`0xC0` is a baked power the table above does not carry.** PROT 0949's tick
  bakes it at `0x801F72F8`; the
  [baked-constant table](#the-baked-power-constants) was read off the routines
  already on the verdict table and this tick was not one of them.
- **PROT 0918 does not load its power as its own literal.** The arm gate
  `addiu v0, zero, 0x12; bne v1, v0` compares the module phase byte against
  `0x12` and the call then reuses the same register as `a0`
  (`move a0, v0` at `0x801F8798`), so the phase number and the baked power are
  one constant. Read `move a0, v0` at face value and the power is lost.
- **"PROT 0927 never kills" is true of its sweep only.** Its move-VM stager
  `0x801F85A8` clamps to `HP - 1` ([shape B](#the-two-clamp-shapes)), but its
  *tick* `0x801F6A84` calls the same wrapper with the same baked `0x12` and
  then clamps `sltu a0, s1` - shape A, kill-capable, and a negative wrapper
  return there kills outright. The same `FUN_801DD0AC(0x12, 7)` + shape-A pair
  is PROT 0922's, so the summon-branch wrapper is not itself a never-kill
  shape.

PROT 0918's damage arm also credits a kill: past the clamp it increments the
word at `+0x664` of the caster's per-character record in the
`0x80084140 + n * 0x414` block (`0x801F87D4..0x801F881C`).

### The twelve bodies the trampoline map names

Naming a trampoline's arms names twelve more routines, each a whole
choreography in an image whose *trampoline* the port also carries. Every one
is read off its owning image's bytes at slot-B base `0x801F69D8`, and every
one is ported (`legaia_engine_vm::cast_module_ticks`, driven from
`World::run_cast_module_code`):

| Body | Owner | Action id | Size | Phase bound | Damage |
|---|---|---|---|---|---|
| `0x801F726C` | 938 | `0x4E` Chaos Breath | 2004 B | `beq`/`slti` chain | `FUN_801DD4B0(0x274)` at `0x801F77C0`, **shape A**, party row |
| `0x801F69EC` | 938 | `0xB7` Mystic Circle | 2176 B | `sltiu 5`, table `0x801F69D8` | `FUN_801DD4B0(0x309)` at `0x801F70EC`, **shape A**, party row |
| `0x801F6A20` | 951 | `0x36` Chaos Flare | 3528 B | `sltiu 0x0C`, table `0x801F69D8` | `FUN_801DD4B0(0x3A0)` at `0x801F7414`, **shape A** |
| `0x801F77E8` | 951 | `0x5B` Scythe Wind | 2436 B | `sltiu 6`, table `0x801F6A08` | `FUN_801DD4B0(0x80)` at `0x801F7F88`, **shape A** |
| `0x801F7118` | 952 | `0x5C` Bloody Horns | 2576 B | `sltiu 7`, table `0x801F69F0` | `FUN_801DD6B4(0x1D0)` at `0x801F7948`, **shape A** |
| `0x801F69D8` | 965 | `0xB6` Doomsday | 4420 B | `beq`/`slti` chain | `FUN_801DD4B0(0x600)` at `0x801F77B4`, **shape A**, party row |
| `0x801F8F0C` | 955 | `0x60` White Shield | 920 B | `beq`/`slti` chain | none - a **defence buff**, [below](#the-four-prot-0955-bodies-that-write-no-damage) |
| `0x801F86A4` | 955 | `0x6E` Kiss of Death | 2152 B | `beq`/`slti` chain | no wrapper; a coin flip, a status mark and a literal `-1` HP |
| `0x801F7FA4` | 955 | `0x6F` Melt Spray | 1792 B | `beq`/`slti` chain | none - a **five-stat debuff** |
| `0x801F767C` | 955 | `0x70` Terror Scream | 2344 B | `beq`/`slti` chain | none - a **turn thief** |
| `0x801F7158` | 955 | `0x72` Power Charge | 1316 B | `beq`/`slti` chain | none - an **ATK buff** |
| `0x801F6A28` | 955 | `0x73` Void Accessories | 1840 B | `beq`/`slti` chain | none - it **strips an equipped accessory** |

Two of the twelve are on no `--missing-ports` row, only because no dump prints
at their VAs: `0x801F69EC` and `0x801F69D8`. Both frame-match in their owning
image - `0x801F69EC` runs `0x880` bytes to where the `0x4E` body opens, and
`0x801F69D8` runs `0x1144` bytes from the module's own load base to where PROT
0965's trampoline begins.

Three corrections fall out of reading them.

**The two AoE stagers are not the band's only party-row appliers.** Three of
the tick bodies above sweep `actor_table[0 .. ctx[+0]]` as well - PROT 0938's
both bodies and PROT 0965's - and all three clamp
[shape A](#the-two-clamp-shapes), so they **kill**. The pairing of that sweep
with the never-kill `HP - 1` clamp holds for `0x801F85A8` / `0x801F8D64` and
for nothing else. (`ctx[+0]` is the [party count](#ctx0-is-the-party-count-not-the-actor-count),
so "whole row" in earlier prose meant the party seats, not all eight.)

The sweep arms are phase `2` (Chaos Breath, `0x801F7750`), phase `3` (Mystic
Circle - table word 3 at `0x801F69E4`) and phase `0x0B` (Doomsday, the arm the
`beq v1,0x0C` / `slt` pair at `0x801F6AE8` sends to `0x801F7648`).

**One sweep has no Stone guard.** Every other loop in the band skips a victim
carrying `+0x16E & 4`; PROT 0938's `0xB7` body tests only `+0x14C == 0`
(`0x801F70D0`), so Mystic Circle hits a petrified seat.

**Out of range is busy, not done.** Each of these bodies seeds a saved
register with `1` and returns it, and only a terminal arm zeroes it - so a
phase past a `sltiu` bound still reports busy. PROT 0949's tick is the
clearest case: its out-of-bound `beqz` at `0x801F6AA4` targets `0x801F758C`,
one instruction *past* the `move s7, zero` at `0x801F7588`.

#### The four PROT 0955 bodies that write no damage

"Damage: none" is not "writes nothing". Four of the six-spell cell's bodies
write the actor **stat block** and the persistent character record, and one
of them is the setter `battle-formulas.md` records as the last status-applier
gap. They are not the band's *only* stat-block writers - see
[the rest of the band's stat writers](#the-band-has-eight-stat-block-writers-not-one).

| Body | What its working arm writes |
|---|---|
| `0x801F8F0C` White Shield | Both halves of both defence pairs (`+0x15C`/`+0x15E`, `+0x160`/`+0x162`) = the caster's **record** base `x 3/2`, read through `0x801C9348[seat - 3]`. Idempotent, because the source is the record and not the live stat. |
| `0x801F7158` Power Charge | Both halves of the ATK pair (`+0x158`/`+0x15A`) `+= x >> 2` - a `+25%` - each capped at `999` (`sltiu 0x3E8` at `0x801F74D4`). |
| `0x801F7FA4` Melt Spray | Ten halfwords: ATK, UDF, LDF, SPD and INT, working **and** base, each `x - (x + 9) / 5`. |
| `0x801F6A28` Void Accessories | `rand() % 3` picks one of the victim's three accessory slots (`record[+0x19B + slot]`); on a second `rand() & 1 == 0` and a non-empty slot it refunds the id to the bag (`FUN_800421D4`), clears the record byte and rebuilds the ability bitfield (`FUN_80042558`). |

Melt Spray's floor is worth spelling out. Each store is followed by
`bnez ...; addiu v0,v0,1`, which tests the full **32-bit** difference while
the store itself is a 16-bit `sh`. A stat of `2` lands on zero and is
corrected to `1`; a stat of `0` or `1` goes to `-1`, misses the `bnez`, and is
written back as `0xFFFF`. Retail underflows a one-point stat into 65535. It is
also a different shape from the item buffs' `x * 6/5` clamped to `0xFFFF`
([battle-formulas.md](battle-formulas.md)).

The two remaining PROT 0955 bodies share one idiom, the **turn steal**
(`0x801F8CF4..0x801F8D54` in Kiss of Death's miss arm, `0x801F7E18..0x801F7E4C`
in Terror Scream's arm 3): refund the victim's queued item when `+0x1DE == 1`
and `+0x16C != 0`, clear `+0x1DE`, then clear the initiative key `+0x16C` and
bump the turn cursor `ctx[+0x1A]`. The victim loses its turn. Kiss of Death
reaches it only on the odd half of a `FUN_80056798() & 1` coin flip, and sets
`+0x16E` bit `0x400` beside it; its even half clears `+0x16E & 0x0F80`, applies
exactly **one** point of damage and stages the victim's reaction.

**Read the delay slot when you take a baked power.** PROT 0951's `0x5B` body
sets `a0` *after* the call word - `jal 0x801DD4B0` at `0x801F7F88` with
`addiu a0, zero, 0x80` in its delay slot - so a scan that only looks backwards
from the `jal` reports no constant for it. None of the sites on the tables
above uses that form, which is exactly why it is easy to miss.

**The rows that leave the worklist without a port.** The **DATA** rows are
scope rows in `scripts/ci/port-catalog-ignore.toml` under
`[slot_b_spawn_stagers]`, because the pool above already produces their whole
output; `crates/asset/tests/cast_module_data_rows_real.rs` re-derives per row,
off the disc, that the routine frame-matches in its **owning** image, that its
spawn count is the one this page's table quotes, and that it calls no damage
wrapper. The **SCOPE-IGNORE** rows are in `[slot_b_cast_module]` - six null
stagers, one routine nothing references, and PROT 0920's per-frame effect
updater `0x801F7B88`, the one slot-B routine SCUS calls
([above](#scus-calls-into-slot-b-at-one-fixed-va---and-only-prot-0920-arms-it)):
it reads the `0x8007BDC0` budget it never writes, walks the module's own
particle records and hands them to `FUN_80021B04`, so the pool produces its
whole output too.

Two band entries carry no record at all and stage nothing: PROT 0926, the
1-sector null stub, and PROT 0952, whose two spawn sites sit in its inherited
tail (file `+0x11E8..+0x1800`, PROT 0951's bytes) and whose `lui`/`addiu` pairs
resolve to `0x801F8348` / `0x801F836C` - two of PROT 0951's own records, past
the end of 0952's `0x1800`-byte image. The old reading, that `a2` came from a
saved register no static window could see, is refuted by the pairs being right
there in front of both calls.

### Frame gating, measured

The [twelve bodies](#the-twelve-bodies-the-trampoline-map-names) and the
[eleven player-Seru ticks](#the-player-seru-bands-tick-bodies-are-code-not-data)
are decoded from their own bytes, but no static read gives *how long each arm
holds*. That is a live measurement, and the unit it has to be taken in is
**module ticks** - one per entry into the body's own routine - not VSyncs. The
battle SM does not advance once per VSync, so a VSync dwell carries host timing:
the same body measured twice differs by a few VSyncs per arm while its tick
counts are the choreography's own.

#### How the measurement is taken

A mid-cast state cannot show the arms before it, and the per-spell mid-cast
corpus is mednafen-only, so the cast is **driven** instead of resumed.
`scripts/pcsx-redux/autorun_w3a_cast_oracle.lua` resumes an ordinary pre-cast
battle state and rewrites the acting party seat's queued action into the cast
under test - `actor[+0x1DE] = 2` (Magic), `+0x1DF = <action id>`,
`+0x1DD = <target seat>` - after which retail pages the module in through the
loader-B tracker `0x8007BC4C` and runs its own tick.

Three things decide where that rewrite may happen.

- **Not at the Begin/Reselect confirm.** The command-flow SM `ctx[+0x06]`
  validates the queued actions at its confirm arm `0x6E`; a Magic action the
  caster has not learned parks it there for good. The rewrite goes in at
  `ctx[7] == 0x0A`, which is past that gate and before state `0x0C` reads the
  category ([`battle-action.md`](battle-action.md)).
- **Only into the acting seat.** `ctx[+0x13]` names it, and a monster commonly
  acts first; a rewrite into a seat that is not acting is simply ignored.
- **Live HP and displayed HP move together.** `+0x172` is the displayed HP the
  party HUD ramps towards, and the `0x51` exit gate `FUN_801E7250` holds the
  whole action band while a *party* target's `+0x14C` differs from it. Seeding
  one without the other parks the battle at `0x51` forever - the same softlock
  shape `battle-action.md` documents, reproduced here by a probe.

The body's entry is then breakpointed. Five VAs are armed at once
(`0x801F69D8`, `E8`, `EC`, `F0`, `F4`), because those are consecutive
instructions in one prologue: a single tick trips every armed VA at or after the
routine's real entry, so the **lowest VA that fires is the entry**, recovered
from execution rather than from a dump's printed address.

#### The arm table, read out of live RAM

The same probe reads the 32-slot cast-tick arm table at `0x801CF4EC` and decodes
the `jal` in each 16-byte PROT 0898 trampoline. Arms `0..=10` resolve to
`0x801F69D8`, `69D8`, `69D8`, `69F4`, `69E8`, `69D8`, `69F4`, `69EC`, `69D8`,
`69D8`, `69F0` - the pairing this page states for ids `0x81..=0x8B`, now
confirmed from RAM. Each body's measured entry VA and the `ra` its tick returns
to agree with it: PROT 0907 enters at `0x801F69E8` with `ra = 0x801F1F84`
(stub 4 + 8), PROT 0910 at `0x801F69EC` with `ra = 0x801F1FB4`, PROT 0911 at
`0x801F69D8` with `ra = 0x801F1FC4`.

#### Per-arm dwell, in module ticks

One cast per body, driven from `party_basic_attack_vs_gobu_gobu` (one party
seat, one monster seat, scripted-fight flag `ctx[+0x287] = 0`). An arm absent
from a row is one the walk never entered.

| PROT | arm dwell, phase `0` upward |
|---|---|
| 0903 Gimard | 1, 1, 64, 1, 9, 8, 99, 1, 15, 8, 24, 16, 87, then `0xFF` |
| 0904 Theeder | 1, 1, 119, 1, 6, 14, 54, 47, 1, 31, 1, 20, 16, 15, 24, then `0xFF` |
| 0905 Vera | 1, 1, 32, 2, 10, 15, **jump to 8**, 45, 28, 38, then `0xFF` |
| 0907 Nighto | 1, 1, 1, 118, 1, 34, 29, 15, 53, 1, 51, 33, 1, 41, **jump to 15**, 18, then `0xFF` |
| 0908 Zenoir | 1, 1, 1, 118, 1, 32, 41, 56, 3, 1, 14, 14, 60, then `0xFF` |
| 0910 Swordie | 1, 1, 4, 8, 17, 46, 1, 65, 1, 25, 53, then `0xFF` |
| 0911 Orb | 1, 1, 4, 32, 25, 54, **jump to 9**, 76, 66, then `0xFF` |

Three of the seven walks skip arms outright. PROT 0911's arm `5` jumps to `9`
(the `sb s7,0x279` at `0x801F7780`), which this page already records; PROT 0905's
arm `5` jumps to `8` the same way, and PROT 0907's fork arm `13` jumps to `15`.
The rest advance one arm per taken tick, which is what makes a dwell above `1` a
countdown rather than a wait on another actor.

`ctx[+0x6D8]` is the countdown those arms ride. It reads `120` at the first tick
of every cast measured here and decrements by exactly `1` per tick, so these
figures are the arms' own lengths and not a stretched frame delta.

Not every arm is a constant, though, and the one body measured in two different
fights says which. PROT 0907 run again against a scripted boss (three party
seats, `ctx[+0x287] = 4`) holds arms `0`, `1`, `4`, `7`, `9`, `11`, `12` and
`13` for exactly the same `1, 1, 1, 15, 1, 33, 1, 41` ticks, and moves on every
other arm - `2` from 1 to 5, `3` from 118 to 65, `5` from 34 to 30, `6` from 29
to 22, `8` from 53 to 41, `10` from 51 to 39, `15` from 18 to 28. So the table
above is one fight's timing: the arms that reproduce are the module's own
countdowns, and the arms that move wait on something in the scene - a travel
distance or a clip length - rather than on a literal.

#### The damage numbers the same casts produced

Every player-Seru wrapper call in these runs passed the **summon seat** `7` as
the attacker, never `ctx[+0x13]`: `addiu a1, zero, 7` is a literal at
`0x801F74A8` (PROT 0903) and at `0x801F8880` (PROT 0910), and the capture reads
`a1 = 7` with `ctx[+0x13] = 0` at every entry. All of them route through
`FUN_801DD0AC`.

| PROT | call site | `a0` | wrapper return | HP lost |
|---|---|---|---|---|
| 0903 | `0x801F74AC` | `0x12` | 247 / 417 | 76 (bar emptied) / 417 |
| 0904 | `0x801F7D38` | `0x11` | 371 | 371 |
| 0908 | `0x801F76CC` | `0x12` | 522 | 130 (a quarter) |
| 0908 | `0x801F7C14` | `0x10` | 541 | 405 (three quarters) |
| 0910 | `0x801F887C` | `0x12` | 427, 427, 380, 384 | 106, 106, 95, 96 |

PROT 0910's four slashes all come from one site, and each takes
`return >> 2`: the `srl s1, s1, 2` at `0x801F8898` sits between the two operands
of the running-total update at `0x8007BD14` and rewrites the same register the
clamp at `0x801F88F0` and both stores at `0x801F8900..0x801F8910` then use. A
reader who takes the shift as belonging to the total alone reports the slash as
unscaled.

The two heals land exactly where their arithmetic says. At magic level `3`, PROT
0905 restored `320` (`3 * 0x20 + 0xE0`) into a seat on `42` of `999` HP, and
PROT 0911 restored `640` (`(3 << 6) + 0x1C0`) into the same shape. MP costs read
off the same casts: `0x81` 10, `0x82` 24, `0x83` 6, `0x85` 13, `0x86` 36, `0x88`
32, `0x89` 18.

#### The Nighto immunity is forced, not rolled

PROT 0907's resist word has two producers, and on a boss only one of them runs.
`0x801F6BF0..0x801F6C24` reads `ctx[+0x287]`, and when it is set, indexes
`0x801C9348[victim_seat - 3]` and tests the monster record's `+0x20`: a non-zero
byte branches to `0x801F6CB8`, which stores a literal `1` into `0x801F853C` -
the `rand()` throw at `0x801F6C28` never runs at all. Both inputs hold on the
Gaza 2 fight (`ctx[+0x287] = 4`, record `+0x20 = 1`), and the cast driven there
reads `0x801F853C = 1` on the first tick after arm `0`, leaves the boss's
`+0x14C` at 15000 and its `+0x16E` at zero. The same monster record also carries
the **scripted** boost profile live - record ATK 288 / UDF 222 / INT 220 install
as 360 / 444 / 247, i.e. `x5/4`, `x2`, `x9/8` - against the random encounter's
17 / 15 / 10 installing as 17 / 25 / 12.

PROT 0907 writes no HP at all. Its fork read kill roll `0x801F8534 = 6` and
resist `0x801F853C = 1`, and retail left `+0x14C`, `+0x16E` and `+0x21C`
untouched while still walking `13 -> 15`. The phase target forks on the **kill
roll alone** (`beqz v0, 0x801F7E48` at `0x801F7E04`): a non-zero roll takes the
confuse path and its unconditional `sb 0xF, 0x279` at `0x801F7E28`, whatever the
resist word says. The resist word only suppresses the victim writes inside each
path.

#### The one SCUS call into slot B, caught firing

The same injection closes the frame question the
[budget gate](#scus-calls-into-slot-b-at-one-fixed-va---and-only-prot-0920-arms-it)
left open: driving action id `0x92` pages PROT 0920 in (loader tracker `25`) and
`jal 0x801F7B88` at `0x800481A0` fires **212 times in one cast**, on ordinary
in-battle frames with the battle-end signal `_DAT_8007BD71` reading `0xFF`
throughout, across module phases `6`, `7`, `8`, `9`, `10` and once at `0xFF`.
The callee is entered exactly as often as the call site.

The budget `_DAT_8007BDC0` behaves as a budget and is touched by **five** sites
in PROT 0920, not three: `0x801F6BBC` zeroes it before the cast, `0x801F6FEC`
seeds `0x204` (516), `0x801F70CC` drains it by `8` per frame across 64 writes,
`0x801F712C` floors it at `4`, and `0x801F7B4C` clears it at the end. Because of
that floor the budget never reaches zero on its own - the module's own clear is
what closes the arm, 1039 VSyncs after it opened.

#### One capture-class body, for contrast

The same instrument run on PROT 0959 (Megaton Press, action `0x7A`) from the
Nivora duel - a real retail cast, no injection needed beyond re-queuing the
action already staged - separates the band's two halves on three counts.

- **The attacker seat is not a literal.** Its three wrapper sites set
  `a1` from `lbu a1, 0x13(...)` (`0x801F71E4`, `0x801F7A8C`, `0x801F7EB8`), and
  the capture reads `a1 = 3`, the enemy caster's own seat. The player-Seru
  bodies bake `addiu a1, zero, 7` instead.
- **`ctx[+0x6D8]` is not its clock.** The byte holds `20` at every one of the
  333 tick entries, where a player-Seru cast seeds `120` and drains it.
- **Its tick is not reached through a PROT 0898 trampoline.** Every entry
  returns to `0x801F8838`, inside slot B itself.

Its arms, in ticks from phase `0`: 1, 3, 16, 32, 16, 22, 30, 22, 2, 56, 32, 24,
24, 8, 16, 29 - and the last of those was still running when the capture window
closed, so `15` is a floor. Damage goes through `FUN_801DD6B4`, the
resist-bypass wrapper, with baked powers `0x80` at `0x801F71E8` and
`0x801F7A90` and `0x30` at `0x801F7EBC`, the last site firing four times inside
arm `15`. The returns `67 / 71 / 26 / 25 / 30 / 25` are applied unscaled.

#### The fourteen, measured

These bodies need an **enemy** caster, so the drive differs from the player
half. `scripts/pcsx-redux/autorun_capture_arm_gating.lua` converts the monster
seat's already-rolled action on a pre-turn battle state - `actor[+0x1DE] = 2`,
`+0x1DF = <action id>`, `+0x1DD = <target>` - instead of rewriting a party
seat's queued one; retail then resolves the module through the spell record's
`+1` sub-id, pages it, and ticks it with its own caster kind.

The tick clock is firmer here, and it needs no guess about which prologue VA is
the entry. The capture-class band has exactly one tick dispatcher:
`jal 0x801F2160` occurs **once** in PROT 0898's bytes, at `0x801E50C8`, so an
Exec breakpoint on the dispatcher is one hit per module tick by construction,
and the phase byte read at that entry is the arm about to run. The twelve
distinct body VAs are armed alongside it, and in every run that ticked at all,
exactly one of them was entered exactly once per dispatcher hit - so the body
column below is read off **execution**, not off a dump's printed address, and
it agrees with the table this page already carries in all twelve cases.

| PROT | action | body entered | arms walked | dwell, ticks per arm |
|---|---|---|---|---|
| 940 | `0xAC` | `0x801F7240` | `0..7` | 1, 30, 64, 16, 16, 48, 18, 27 |
| 940 | `0x50` | `0x801F78B8` | `0..3`, `0xFF` | 1, 3, 64, 19, 1 |
| 940 | `0xAE` | `0x801F78B8` | `0..3`, `0xFF` | 1, 4, 64, 16, 1 |
| 941 | `0x51` | `0x801F730C` | `0..3`, `0xFF` | 1, 21, 16, 32, 1 |
| 941 | `0xB9` | `0x801F6A04` | `0..4` | 1, 65, 32, 64, 25 |
| 943 | `0x40` | `0x801F6EF4` | `0..4` | 1, 9, 40, 8, 32 (Che Delilas' turn, see below) |
| 943 | `0xB5` | `0x801F6A04` | `0..4` | 1, 65, 32, 64, 32 |
| 944 | `0x37` | `0x801F6A04` | `0..5` | 1, 33, 32, 32, 64, 82 |
| 944 | `0x53` | `0x801F7470` | `0..4` | 1, 9, 40, 8, 32 (Che Delilas' turn, see below) |
| 950 | `0x5A` | `0x801F79F8` | `0..4` | 1, 13, 1, 32, 13 |
| 950 | `0xAB` | `0x801F6A24` | `0..6` of 14 | 1, 65, 21, 64, 8, 40, 15+ |
| 956 | `0x71` | `0x801F7298` | `0..3`, `0xFF` | 1, 33, 32, 32, 1 |
| 962 | `0xA2` | `0x801F7AE4` | `0..3` | 1, 21, 1, 1862+ |
| 962 | `0xA3` | `0x801F74A0` | `0..4` | 1, 21, 1, 42, 684+ |
| 962 | `0xA4` | `0x801F6D54` | `0..4`, `0xFF` | 1, 21, 1, 18, 42, 1 |

A `+` marks a floor: the arm was still running when the capture window closed.
Three of the fourteen **park** rather than finish in this fight. PROT 0950's
`0xAB` is the band's fourteen-arm body and stops at arm `6`; PROT 0962's `0xA2`
and `0xA3` each sit in their last listed arm for hundreds of ticks without
advancing. So those arms have an exit gate the fight does not satisfy, not a
long countdown.

`0xAB` also carries the reproducibility evidence. Driven twice, with capture
windows of 300 and 600 seconds, it returned the **same** six dwells
`1, 65, 21, 64, 8, 40` for arms `0..5` and parked in arm `6` both times - so
those six are the module's own countdowns rather than a wait on the scene.

Twelve rows come from one fight, `party_basic_attack_vs_gobu_gobu` - one party
seat, one monster seat - and the two Curse rows from `nivora_duel_pre_megaton_press`
(Gala against Che Delilas, one seat each; why that fight is
[below](#the-two-curse-arms-fault-on-a-caster-with-too-few-spell-entries)).
The same caveat the player half carries applies: an arm that reproduces across
fights is the module's own countdown, and an arm that moves waits on the scene.

The arm **sets** are a stronger result than the dwells, because they are the
dispatch bound walked rather than read. Each table-dispatched body walks
exactly the arms its `sltiu` bound allows and stops on the terminal arm with no
`0xFF` - `0..7` for PROT 0940's `0xAC`, `0..4` for the three `sltiu 5` bodies,
`0..5` for PROT 0944's `0x37`. Each chain-dispatched body walks `0..3` (`0..4`
for PROT 0962's) and then latches `0xFF` for exactly one tick.

##### The countdown gates most arms, and its drain is per-arm

Each of these modules keeps its own countdown word, and for most arms it is the
gate: the dwell in ticks is the seed divided by what the arm draws the word
down by per tick. PROT 0941's `0x51` is the clearest - arm `2` seeds `0x100`
and arm `3` seeds `0x200`, both drain `16` per tick, and the arms run `16` and
`32` ticks.

What is **not** uniform is that per-tick amount, and the disassembly says why:
the arms differ in a baked **multiplier**, not in the quantity. Every counted
arm subtracts a multiple of the frame-delta byte `*(0x1F800393)`, and the
multiplier is part of the arm's own code. Three forms appear, two of them
inside a single body:

| form | arm | the instructions |
|---|---|---|
| `*(0x1F800393) * *(0x1F80037D)` | PROT 0940 `0x50` arm 1; PROT 0941 `0x51`'s last arm | `lbu 0x69(v0)` / `lbu 0x7f(v0)` off `0x1F800314`, `mult`, `mflo`, `subu` - `0x801F7A88..0x801F7AA4`, `0x801F7CCC..0x801F7CE8` |
| `*(0x1F800393) << 1` | PROT 0940 `0x50` arm 2 | `lbu 0x393(v1)`, `sll v1,v1,1`, `subu a0,a0,v1` - `0x801F7B6C..0x801F7B7C` |
| `*(0x1F800393)` | PROT 0943 `0xB5` | `lbu 0x7f(a1)` off `0x1F800314`, `subu a2,v0,v1` - `0x801F6B88..0x801F6B94` |

That resolves the two arms the capture reported as unexplained constants.
PROT 0943's `0xB5` draws `4` per tick against a product of `16..32` because it
subtracts the bare byte and the byte read `4`; PROT 0940's `0x50` draws `8` on
arm `2` for the same reason, doubled - the same word, `0x801F864C`, drained by
two different expressions in two arms of one body. So "a constant the step does
not explain" is a measurement of the multiplier, not of a literal: the frame
step is one arm's decrement, not the word's, and none of the three forms is a
constant.

The step itself is adaptive and changes **inside** a single cast - the audio
frame driver rewrites `DAT_1F800393` per frame
([`audio.md`](audio.md)) - so a dwell predicted from an arm's first observed
step reports a countdown-gated arm as ungated. The reducer
`scripts/pcsx-redux/analyze_capture_arm_gating.py` sums the per-tick steps
instead of scaling the first one.

Two of the module-resident words that list names are not the gate for the
bodies measured here. PROT 0962's `0x801F89AC` holds a constant `1024` across
all three of its bodies' walks, and PROT 0950's `0x801F86B0` goes **negative**
(`0xFFFFFF80`) inside `0x5A`'s last arm, so whatever ends those arms is a
different word or a different test.

##### The two Curse arms fault on a caster with too few spell entries

Driving PROT 0943's `0x40` (Curse) or PROT 0944's `0x53` (Curse All) from the
Gobu Gobu fight reaches battle phase `0x70` with the module paged - the
loader-B tracker reads `48` and `49`, and slot-B word `0` changes to the
module's - and the emulator then reports an 8-bit read at the **same** garbage
address for both, `0x626F4797`. Under `-debugger` that read pauses the whole
emulator with no PC, which is why the first runs read as "faulted before the
first tick". Installing the emulator's `UnknownMemoryRead` hook instead
(`autorun_capture_arm_gating.lua`, `LEGAIA_TRAP_UNMAPPED=1`) names the
instruction and shows the body **was** entered: `0x801F6EF4` ticks once, arm
`0` runs, and the 57 unmapped reads that follow are all in SCUS - the anim
commit `FUN_8004AD80` (`pc 0x8004AF24`), the actor tick and the keyframe
decoders (`FUN_80047430`, `FUN_80048A08`, `FUN_800495C8`) - dereferencing one
pointer, `0x626F4720`.

The pointer is the monster's **name**. Arm `0` of the Curse body stages clip
`0x0B` on the caster (`sb 0x0B, 0x1DA(s1)` at `0x801F6FBC`; Curse All's arm
`0` stages the same literal `0x0B` at `0x801F758C`), and the
anim commit resolves a
staged clip by indexing the monster record's spell-entry offset array with
it: `lw v0, 0x4C(block + clip*4)` at `0x8004AF08..0x8004AF18`, then
`lbu 0x77(v0)`. Gobu Gobu's record has **ten** entries (`+0x4C..+0x74`), so
index `0x0B` reads word `+0x78`, which is the name text `" Gob"` -
`0x626F4720` byte for byte. Their siblings in the same two images (`0xB5`,
`0x37`) stage clips inside the ten, which is why they complete from the same
state.

Twenty-seven records carry twelve or more entries, and on one of them the
same two casts run clean: forced on Che Delilas' turn
(`nivora_duel_pre_megaton_press`, twelve entries), `0x40` enters
`0x801F6EF4` and `0x53` enters `0x801F7470`, each walks arms `0..4` in
`1, 9, 40, 8, 32` ticks with `ctx[+0x6D8]` holding `20` throughout and
**zero** unmapped accesses, and the battle phase runs `0x51 -> 0x5A -> 0xFF`.
The two bodies seed the same countdown (`0x100`, `0x500`, `0x100`, `0x400`
against a step of 32) so the identical dwells are the modules' own, not a
coincidence of the fight. Those are the two rows in the table above.

What the measurement cannot claim is a retail timing. **Neither id has a
retail caster**: no monster record's `+0x21..+0x23` magic slots name `0x40`
or `0x53` (disc-gated assertion in
`crates/engine-core/tests/cast_arm_retail_gating.rs`, with Cort's `0x37` as
the positive control), and the AI picker's formation switch queues neither
([`spell-table.md`](../formats/spell-table.md)). The gating is measured; the
cast is one retail never performs.

#### What actually stops PROT 0950 at arm 6

Arm 6 has no exit condition of its own to satisfy. Its tail is the band's
ordinary countdown expiry - `lw` the module countdown `0x801F86B0`, subtract
the scratchpad frame-step product, `bgtz` back to the return, and on the
fall-through reseed it (`stepA << 5`) and bump `ctx[+0x279]` through the shared
tail at `0x801F7928`. The `0xAB` capture's last logged tick leaves the
countdown at `48` against a step of `32`, so the arm was two ticks from
advancing; what ends the run is the emulator reporting an unmapped 8-bit read,
not a gate that never opens.

The address is not arm 6's and not arm 7's: arm 7 reads only the target record
`s4` and the globals, and both arms form every pointer from a register the
prologue loaded. The shape that matches an unmapped read is arm **10**, which
materialises the actor pointer table (`addiu s2, v0, -0x6c90`) and walks it
with `lw ($s2)` / `addiu s2, s2, 4` - an AoE sweep whose loop bound is the
seat count, over a table a forced enemy cast on a one-monster fight does not
fill. So the ladder these arms need is a fight with the seat count the sweep
expects, not a state whose arm-6 gate passes.

The dwell itself does **not** reduce to one law across the arms. Each arm's
fall-through reseeds a per-arm multiple of the scratchpad step byte alone -
`<< 8` at arm 0, `<< 6` at arms 1 and 5 and 8..12, `<< 5` at arms 3 and 6,
`* 24` at arm 7, `* 160` at arm 4 - but predicting a dwell from those
multipliers and one step product reproduces arm 6's measured 16 ticks and
misses arm 1's measured 65 by a factor of two, because several arms subtract a
*multiple* of the product rather than the product. The per-arm drain has to be
read per arm, which is the same conclusion the band's other module reached.

### The fourteen trampoline arms that are the band's other tick bodies

<a id="the-fourteen-trampoline-arms-that-are-unported-tick-bodies"></a>

The twelve bodies above are one part of the band's code. The trampoline table
[above](#the-trampolines-are-their-own-port-and-one-cell-holds-six-spells)
names fourteen more arms, tick bodies of exactly the same class as the eleven
player-Seru ones below: whole choreographies, none small. All fourteen are
ported as `legaia_engine_vm::cast_arm_ticks`, keyed on `(entry, body)`, and
driven from `World::run_cast_module_code`; the per-arm behaviour rows are on
[`functions/battle.md`](../reference/functions/battle.md#slot-b-summon--cast-modules-prot-09030966).

| Body | Owner | Action id | Size | Damage wrapper |
|---|---|---|---|---|
| `0x801F7240` | 940 `cast_glare_divide` | `0xAC` | 1656 B | none |
| `0x801F78B8` | 940 `cast_glare_divide` | `0x50` / `0xAE` | 2416 B | none |
| `0x801F730C` | 941 `cast_steal` | `0x51` | 2604 B | **none** |
| `0x801F6A04` | 941 `cast_steal` | `0xB9` | 2312 B | one |
| `0x801F6EF4` | 943 `cast_curse` | `0x40` | 1840 B | none |
| `0x801F6A04` | 943 `cast_curse` | `0xB5` | 1264 B | none |
| `0x801F6A04` | 944 `cast_guilty_cross` | `0x37` | 2668 B | one |
| `0x801F7470` | 944 `cast_guilty_cross` | `0x53` | 2636 B | none |
| `0x801F79F8` | 950 `cast_rolling_flare` | `0x5A` | 1944 B | one |
| `0x801F6A24` | 950 `cast_rolling_flare` | `0xAB` | 4052 B | one |
| `0x801F7298` | 956 `cast_water_hazard` | `0x71` | 2996 B | two |
| `0x801F7AE4` | 962 `cast_blade_breath` | `0xA2` | 1436 B | one |
| `0x801F74A0` | 962 `cast_blade_breath` | `0xA3` | 1604 B | one |
| `0x801F6D54` | 962 `cast_blade_breath` | `0xA4` | 1868 B | one |

Sizes are the frame-matched extent in the **owning** image; "damage wrapper"
counts `jal` to `FUN_801DD0AC` / `FUN_801DD4B0` / `FUN_801DD6B4`.

PROT 0941's `0x51` row **corrects** an earlier "one" in this table. The enemy
Steal arm reaches no damage wrapper at all: its ten distinct `jal` targets are
`0x80019B28`, `0x8003CA78`, `0x8003CAC4`, `0x80042310`, `0x8004E2F0`,
`0x8004FE5C`, `0x80050E2C`, `0x80056798`, `0x801D5854` and `0x801D8DE8`, and
the outcome is an inventory consume (`FUN_80042310`) or a roll against the
static steal table, not HP
([`functions/cast-modules.md`](../reference/functions/cast-modules.md)).
The wrapper in PROT 0941 belongs to its **other** body, the `0xB9` row.

`0x801F6A04` is the clearest case yet that a body VA is not a key. It is an
arm in three different images and frame-matches at **three different sizes** -
1264 B in PROT 0943, 2312 B in 0941, 2668 B in 0944. One `--missing-ports`
row therefore names three routines, and a port keyed on the address alone
would run whichever one it happened to be written from for all three, which is
the defect `capture_tick_body` already keys `(entry, body)` to avoid.

Nothing but a port could have removed these rows: they are choreography, not
data, so neither the spawn pool nor any other engine mechanism produces their
output, and a scope row in `port-catalog-ignore.toml` would have been a false
claim. Three of the fourteen also fix a shape this page had wrong. PROT 0943's
`0xB5` body is the one that drains MP, and it sits at `0x801F6A04`, not at
`0x801F69D8`: that address is the module's **head table**, eleven words
holding two stacked five-arm tables (the `0xB5` body's at `0x801F69D8`, the
`0x40` body's at `0x801F69F0`, one zero word between them), with the image's
first prologue at file `+0x2C`. PROT 0940's `0x50` / `0xAE` body
is the only routine in PROT 0903..0966 that **allocates a battle seat**, and
its `0xAC` sibling blanks `actor_table[3]`'s reaction-clip run through a
reassigned `s0` rather than the caster's `+0x0C`.

### The player Seru band's tick bodies are code, not data

The verdict table above answers for each module's **stager** - the
`0x801F6734` row the move VM's opcode `0x20` calls. It does not answer for
the module's *tick*, and for the eleven player Seru-magic ids (`0x81..=0x8B`
= PROT 0903..0913) those are two different routines. Reading a **DATA**
verdict there as "the whole module is data" is a category error: the stager
hands spawn records to the pool, and the tick is the choreography.

Every one of the eleven `0x801CF4EC` arms is a full tick body, and none of
them is small:

| Id | PROT (spell) | `0x801CF4EC` arm | Size | Wrapper calls | HP `+0x14C` | Stage `+0x1DA` | Phase `+0x279` |
|---|---|---|---|---|---|---|---|
| `0x81` | 903 `summon_gimard` (Gimard) | `0x801F69D8` | 3396 B | 1 | 1 | 3 | 4 (2 + 2) |
| `0x82` | 904 `summon_theeder` (Theeder) | `0x801F69D8` | 6020 B | 1 | 1 | 3 | 6 (4 + 2) |
| `0x83` | 905 `summon_stager_x83` (**Vera**) | `0x801F69D8` | 5792 B | 0 | 1 | 3 | 4 (3 + 1) |
| `0x84` | 906 `summon_gizam` (Gizam) | `0x801F69F4` | 3404 B | 1 | 1 | 4 | 8 (5 + 3) |
| `0x85` | 907 `summon_nighto` (Nighto) | `0x801F69E8` | 5568 B | 0 | 1 | 2 | 10 (9 + 1) |
| `0x86` | 908 `summon_zenoir` (Zenoir) | `0x801F69D8` | 6456 B | 3 | 3 | 10 | 6 (0 + 6) |
| `0x87` | 909 `summon_viguro` (Viguro) | `0x801F69F4` | 3924 B | 1 | 1 | 4 | 6 (3 + 3) |
| `0x88` | 910 `summon_swordie` (Swordie) | `0x801F69EC` | 4652 B | 0 (+1) | 0 (+1) | 3 (+2) | 5 (3 + 2) |
| `0x89` | 911 `summon_orb` (Orb) | `0x801F69D8` | 5648 B | 0 | 1 | 1 | 3 (3 + 0) |
| `0x8A` | 912 `summon_freed` (Freed) | `0x801F69D8` | 6532 B | 1 | 1 | 3 | 6 (4 + 2) |
| `0x8B` | 913 `summon_nova` (Nova) | `0x801F69F0` | 7260 B | 1 | 1 | 4 | 5 (1 + 4) |

Sizes are the frame-matched extent in the **owning** image and are unchanged.
"Wrapper calls" counts `jal` to any of the three damage wrappers
`FUN_801DD0AC` / `FUN_801DD4B0` / `FUN_801DD6B4`; the store columns count
`sb`/`sh`/`sw` at that displacement. `(+n)` on PROT 0910's row is what the
tick's callee adds, below. The image labels are the
[`static-overlays.toml`](../../crates/asset/data/static-overlays.toml) ones,
kept for filename stability; `summon_stager_x83` is Vera
([spell-table.md](../formats/spell-table.md)), and its tick restores HP and
cures status rather than dealing damage, which is why its wrapper column is
zero.

### Where a cure tier comes from

Three ticks in the band switch on a **cure tier** `1..=4` and `and` a keep-mask
into the target's `+0x16E`: PROT 0905 (Vera) at `0x801F7D68`, PROT 0911 (Orb)
at `0x801F7BE4` and PROT 0919 (Spoon) at `0x801F8168`. The masks are
`0xFFFC` / `0xFF84` / `0xFB84` / `0xFB84`, and tier `4` additionally doubles
`+0x170` under a `0x64` clamp (`0x801F7F24..0x801F7F48`).

The tier is **not** module data. All three read the same battle-overlay word
`0x801F6960`, which sits below the slot-B base and is the Seru side-effect
stager's output latch: `FUN_801F3D3C` selects an 8-byte record out of the
`[element][level band]` table at `0x801F6870`
(`0x801F6870 + ((level - 3) >> 1) * 8 + element * 0x20`, built at
`0x801F4420..0x801F4440`) and stores its first byte there
(`sw v1,0x6960(v0)` at `0x801F4480`). On the **light** row that byte is the
cure class `1, 2, 3, 4` by magic-level band; on the six damaging rows it is a
percent `5 / 10 / 15 / 20`, which matches none of the four arms. So the
element gate is the latch's own value - a non-light summon leaves a percent
there and cures nothing, with no second test. Below magic level `3` the stager
returns before staging anything, the latch holds `0`, and each module's own
`sltiu v0,v0,0x3` skips the ladder as well.

The table itself, including the light row's `1 / 2 / 3 / 4` ladder, is
tabulated in
[`battle-formulas.md`](battle-formulas.md#seru-magic-side-effects---the-element-debuffs-fun_801f3d3c--the-finisher-switch).
What this section adds is the consumer side: which three ticks read the latch,
what each tier does to `+0x16E`, and that the element gate is the latch's
value rather than a test.

Parser: [`legaia_asset::seru_side_effect`](../../crates/asset/src/seru_side_effect.rs);
masks and constants at `legaia_engine_vm::cast_seru_ticks_a`; the latch is
`BattleActionCtx::follow_up_pending` and `World::cure_selector` is what feeds
the two ported ticks.

**The phase column counts two store forms, and it used to count one.** Every
one of the eleven materialises a pointer to `ctx + 0x279` in its prologue -
`addiu s6, s1, 0x279` at `0x801F6A78` in PROT 0903, `addiu s5, v1, 0x279` at
`0x801F6A54` in 0908, and nine more - and then writes the phase byte through
that register at displacement zero. A census that reads only
`sb rX, 0x279(rY)` misses those stores, and it misses them exactly where they
decide the choreography: **eight of the eleven write their terminal `0xFF`**
through the register form, and PROT 0908 writes the phase byte six times with
no literal-displacement store at all, which is why the column read `0` for a
tick that drives a ten-arm machine. The re-measured numbers above are
`total (literal + register)`.

**PROT 0910 does damage; it just does not do it in the tick.** The row's `0`
wrapper calls and `0` HP writes are a property of the frame, not of the
module. The tick `0x801F69EC` calls `0x801F81DC` from three sites
(`0x801F78E8`, `0x801F7928`, `0x801F7A08`), and that callee is the per-slash
applier: `addiu a0, zero, 0x12` / `addiu a1, zero, 7` /
`jal 0x801DD0AC` at `0x801F8874`..`0x801F887C`, the run-time cap
[above](#the-three-clamp-shapes), the `+0x10` accumulate and
`sh v1, 0x14C(s2)` at `0x801F8910`. A per-function census reads a caller's
damage as absent; only the call closure sees it.

All eleven are ported, one function per body:
`legaia_engine_vm::cast_seru_ticks_a` carries PROT 0903..0908
(`gimard_tick`, `theeder_tick`, `vera_tick`, `gizam_tick`, `nighto_tick`,
`zenoir_tick`) and `cast_seru_ticks_b` carries PROT 0909..0913
(`viguro_tick`, `swordie_tick` with `swordie_slash`, `orb_tick`, `freed_tick`,
`nova_tick`). `World::run_cast_module_code` drives them from the same seam it
drives the capture-class bodies from, so a player summon in `play-window` or
on the browser play page runs the module's own phase machine. What they leave
out is what the static window cannot answer: the GPU-packet and camera arms,
and the per-arm frame gating. The damage half still folds once, at
`World::cast_spell_on_slots_prepaid`, with the module's magnitudes routed into
the fold - Vera's `level * 0x20 + 0xE0` and Orb's `(level << 6) + 0x1C0` - so
no body applies HP twice.

Five VAs cover the eleven arms, because a module whose image opens with code
puts its tick at the load base. `0x801F69D8` alone is the arm for **six** of
the eleven - PROT 0903, 0904, 0905, 0908, 0911 and 0912 - and a capture-class
body in six more, which is the same reason the trampoline map has to be keyed
on `(entry, body)`
[above](#a-body-va-is-not-a-key---only-entry-body-is).

### The band has eight stat-block writers, not one

The four PROT 0955 bodies above were once read as the band's only writers of
the actor stat block. They are not. The decisive measurement is an exhaustive
sweep of all 64 band images for `sh` with an immediate in `+0x150..+0x16D` -
the HP/MP/AGL triplet plus the five `(working, base)` stat pairs and the
initiative key - which finds stores in **eight** images:

| Image | routine | what it does to the block |
|---|---|---|
| 0940 `cast_glare_divide` | `0x801F78B8` | the `0xAE` coin flip: `+0x14C` / `+0x150` / `+0x154` / `+0x156` / `+0x158` on **one** of the two halves, plus `+0x16C` at `0x801F8064`. The "nine stores" are two exclusive branches of five, not one pass of nine |
| 0942 `cast_power_up` | `0x801F7D34` | one store: `+0x156` (AGL base) `= record[+0x0E] * 3 / 2` |
| 0943 `cast_curse` | `0x801F6A04` (the `0xB5` body) | `+0x150` / `+0x152` (the MP pair) at `0x801F6D08` / `0x801F6D1C`, over `0 .. ctx[+0]` with no liveness guard |
| 0945 `cast_water_column` | `0x801F69F8` | all ten stat halfwords `x + (x >> 2)`, then the same `+0x156` write as 0942 |
| 0954 `cast_fatal_decision` | `0x801F6A58` | halves stat halfwords with a floor of `1`, and ORs status bits into `+0x16E` |
| 0955 `cast_white_shield` | six bodies | the four rows in the table above, plus the two turn-steal `+0x16C` clears |
| 0925 `summon_spikefish` | `0x801F6A00` | `+0x16C` only at `0x801F7A70`..`0x801F7A88` - the initiative key, the turn-steal idiom |
| 0956 `cast_water_hazard` | `0x801F69D8` | `+0x16C` only at `0x801F7098`, same idiom |

Two of those are worth reading before assuming a shape from a name.

**PROT 0945's `0xBA` body is the band's widest buff.** Its arm `2`
(`0x801F6DA8..0x801F6E44`) raises **all ten** halfwords of the five pairs by
`x + (x >> 2)` - the same `+25%` PROT 0955's Power Charge applies, but over
the whole block instead of the ATK pair - and then writes `+0x156` off the
monster record exactly as PROT 0942's Power Up does. Every operand is an
`lhu` and the shift is `srl`, so nothing here saturates: a stat near `0xFFFF`
wraps.

**PROT 0954's is the mirror image.** Each store is `srl 1` followed by a
`bnez` / `addiu +1` pair, so a stat halves with a floor of `1` rather than
reaching zero - the opposite floor rule from PROT 0955's Melt Spray, which
lets a one-point stat underflow to `0xFFFF`.

The AGL write is the one formula two modules share verbatim: `+0x156` takes
`record[+0x0E] * 3 / 2` through `0x801C9348[ctx[+0x13] - 3]`, in PROT 0942 at
`0x801F8060..0x801F8074` and in PROT 0945 at `0x801F6E38..0x801F6E44`.
Neither writes `+0x154`, so the working gauge only picks the buff up at the
next round reset. `battle-formulas.md` names that pair as the one the "Power
Up" buff moves, and PROT 0942 is the module the spell pages.

Ports: `legaia_engine_vm::cast_module_ticks::power_up_tick` and
`all_stats_surge_tick`.

## Provenance

Disassembly of the PROT 958/959/960 images (offsets above); the commit
mirror via `see ghidra/scripts/funcs/8004ad80.txt`; natural-cast playouts
captured per-frame under PCSX-Redux (`autorun_delilas_enemy_cast_watch.lua`)
pin the staging walks, the loop counter, and the phase-5 confirm gate.
Patcher mirror: `legaia_patcher::delilas_cast` (expect-verified word edits
against these images); staged player rows `legaia_asset::party_swap::cast_stage`.

The entry tables and the image partition come from disassembly of the 0898
image (`FUN_801F1ED4` at file `+0x236BC`, the tables at `0x801CF4EC` and
`0x801F6734`) plus per-image dumps of the thirteen extracted slot-B modules,
taken with `ghidra/scripts/dump_static_overlay.py` against one Ghidra program
per PROT entry - `see ghidra/scripts/funcs/overlay_summon_ozma_0934_801f6a40.txt`
and its siblings, and `see ghidra/scripts/funcs/80023070.txt` for the
opcode-`0x20` call site.

<!-- W1-A appendix -->

## Appendix: the band names no routine the directory was missing

An independent byte-level re-derivation of the seventeen `ambiguous = no`
worklist runs this band still carries - the ones large enough to reach
`disc-coverage.py`'s 64-byte floor - names **no new routine**, in any module.
Every run is one of the two shapes
[above](#a-module-image-ends-in-another-images-bytes): the image's own data
tail, or a byte-identical same-file-offset run of a neighbour's image. Run by
run, with the boundary between the two halves and where the inherited half is
already dumped, on
[`functions/cast-modules.md`](../reference/functions/cast-modules.md).

Two checks make that a measurement rather than a reading. Force-disassembled,
each run's own half decodes 9-34% implausible opcodes against 0% for a real
body in the same band. And a prologue scan over the seventeen images finds
five `addiu sp, sp, -F` words outside the frame-matched partition -
`0x801F8078`, `0x801F816C`, `0x801F88EC`, `0x801F89D4`, `0x801F9458` - every
one of them inside an inherited tail and a function head of the image the tail
came from. Four are already named on this page at their owner - `0x801F8078`
and `0x801F89D4` in the worklist table, `0x801F816C` in the trampoline map,
`0x801F9458` in the residue section - and only `0x801F88EC` (a frame-matched
head in PROT 0964) was not.

The corresponding entry in `ghidra/scripts/dump_static_overlay.py`'s
`NOT_CODE` record now carries the band, so the runs are recorded as answered
rather than regenerating as work.
