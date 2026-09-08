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

The apply shape is **two** shapes, not one, and which one a module uses
decides whether its hit can kill - see
[the two clamp shapes](#the-two-clamp-shapes). Both are ported at
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
| 966 (Evil Seru Magic) | `FUN_801DD4B0` | `0x100` |

This page previously gave 958's run as "`0x30, 0x38, 0x38, 0x38, 0x40, ..`".
The sixth site (`0x801F88D8`) is `0x30`, so the escalation does not continue -
the run ends where it started.

Every one of these is an immediate compiled into the module image, so a
capture-class cast never reads the move-power table for its magnitude. The
engine seeds the wrappers from this table instead
(`World::baked_module_power`, feeding `capture_bypass_predamage` /
`capture_respect_predamage`).

### The two clamp shapes

**Shape A - clamp to HP, floor 0.** PROT 0945, 0957 (both tick bodies), 0958,
0960:

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

**Shape B - clamp to `HP - 1`, floor 1.** PROT 0927 and 0966, the band's two
AoE sweeps:

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

### The two AoE sweeps

Those same two routines are the band's only whole-row appliers, and they are
`0x801F6734` **stagers**, not tick bodies - the move script drives them
through move-VM opcode `0x20`, so the damage lands from the spawn stager and
not from the `ctx+0x279` machine:

| | PROT 0927 (Juggernaut) | PROT 0966 (Evil Seru Magic) |
|---|---|---|
| seats swept | `actor_table[3 ..]`, the enemy row | `actor_table[0 ..]`, the whole table |
| bound | `ctx[+1]` (monster count) | `ctx[+0]` (actor count) |
| skips | `+0x14C == 0`, `+0x16E & 4` | the same two |
| wrapper | `FUN_801DD0AC(0x12, 7, seat)` | `FUN_801DD4B0(0x100, ctx[+0x13], seat)` |
| also writes | - | `+0x1DA = +0x1F1`, `+0x1DC += 1`, `+0x21D = 2` |

So Cort's ESM hits the party *and* the monsters, stages each victim's own
knockdown reaction, and drops every hit seat into slow motion.

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

## What of the choreography is data, and what is code

A signature cast is **half data**. Its particle layer is a record in exactly the
format a player art already names by id; its lift and its camera are the
module's own instructions and nothing outside the module can reach them.

**The spawn layer is data, in the art path's own format.** Each module reaches
the pool spawner `FUN_80050ED4` from hardcoded `jal` sites - 15 in PROT `0958`,
41 in `0959`, 24 in `0960` - and at every one of them `a2` is a **constant
module-resident pointer** and `a3` is a scale literal (`0x1000` at every 958 and
960 site; 959 also uses `0x0C00` four times and `0x0800` once). The pointers land
in each module's data band and nowhere else: `0x801F8EB8..0x801F9348` in 958 (13
distinct records for 15 sites), `0x801F884C..0x801F95CC` in 959 (41 for 41),
`0x801F8768..0x801F8E0C` in 960 (21 for 24). What they point at is the **summon
part-record shape** the whole spawn stack shares - `[i16 model_sel][u16 flags]
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

| VA | owner | what the routine is | also present in | verdict |
|---|---|---|---|---|
| `801F6A14` | 957 (`summon_effect_table`) | cast tick body, reached from the module trampoline; damage roll (resist); writes HP `+0x14C`, staged `+0x1DA`, restage `+0x1DC`, `ctx+0x278`, phase `ctx+0x279` | - | **PORT** |
| `801F74E4` | 960 (`cast_plasma_strike`) | cast tick body, reached from the module trampoline; damage roll (bypass); writes `+0x0C`, HP `+0x14C`, staged `+0x1DA`, restage `+0x1DC`, `ctx+0x278`, phase `ctx+0x279` | - | **PORT** |
| `801F798C` | 957 (`summon_effect_table`) | cast tick body, reached from the module trampoline; writes `+0x0C`, HP `+0x14C`, staged `+0x1DA`, restage `+0x1DC`, phase `ctx+0x279` | - | **PORT** |
| `801F81DC` | 951 (`cast_chaos_flare`) | spawn stager, 4 spawn calls | also in 910 | **DATA** |
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
sweep (`scripts/ghidra-analysis/find-address-word-refs.py`) reports no word, no
`jal`, no `j`, no PC-relative branch and no `lui`+`addiu` pair for any of them
in any image, and PROT 0909 holds no jump table that could reach them. Only
`0x801F7948` is on the worklist, because only it has a dump.

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
A live hit needs a battle that ends while a Slippery cast is still animating;
no state in the library is at that point.

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
function per VA. Each carries its routine's dispatch bound, its
simulation-state writes, its damage step (baked power, wrapper, clamp shape,
`+0x10` accumulate, HP write, reaction stage, anim-rate write) and the phase
advance. `World::run_cast_module_code` drives them from the same seam retail
re-enters the paged module at - the stager tick the action SM calls at states
`0x34` / `0x35` / `0x36` - so a live cast in `play-window` or on the browser
play page reaches them.

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

The capture-class arm shape above is a routine in its own right, and six of
them are named by nothing else in the corpus. Each is 88 to 204 bytes, opens
`addiu sp, sp, -0x18`, materialises the battle ctx `*0x8007BD24`, loads the
caster `actor_table[ctx+0x13]` out of `0x801C9370` and reads its queued action
byte `caster[+0x1DF]`; an id the routine does not name returns `a0 = 0`, so the
module ticks nothing and the drive loop proceeds. The port carries the map as
data (`cast_module_ticks::CAPTURE_TRAMPOLINES` / `capture_tick_body`), and
`crates/asset/tests/cast_module_data_rows_real.rs` re-derives each row off the
disc from PROT 0898's `0x801CF56C` arm and the trampoline's own `jal` set.

| Owner | Trampoline | Action id -> tick body |
|---|---|---|
| 938 (`cast_chaos_breath`) | `0x801F7A40` | `0x4E` -> `0x801F726C`, `0xB7` -> `0x801F69EC` |
| 951 (`cast_chaos_flare`) | `0x801F816C` | `0x36` -> `0x801F6A20`, `0x5B` -> `0x801F77E8` |
| 952 (`cast_bloody_horns`) | `0x801F7B28` | `0x5C` -> `0x801F7118`, `0xB8` -> `0x801F6A0C` |
| 955 (`cast_white_shield`) | `0x801F92A4` | six ids, [below](#prot-0955-is-a-six-spell-cell) |
| 958 (`cast_blazing_slash`) | `0x801F8E60` | `0x79` -> `0x801F6DD8` |
| 965 (`cast_doomsday`) | `0x801F7B1C` | `0xB6` -> `0x801F69D8` |

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

### The ten bodies the trampoline map names and nothing ports

Naming a trampoline's arms names ten more routines, each a whole choreography
in an image whose *trampoline* is now ported. They are real, un-ported work,
and these are the facts a port needs, read off each owning image's bytes:

| Body | Owner | Action id | Size | Phase bound | Damage |
|---|---|---|---|---|---|
| `0x801F726C` | 938 | `0x4E` Chaos Breath | 2004 B | `beq`/`slti` chain | `FUN_801DD4B0(0x274)` at `0x801F77C0` |
| `0x801F6A20` | 951 | `0x36` Chaos Flare | 3528 B | `sltiu 0x0C` | `FUN_801DD4B0(0x3A0)` at `0x801F7414` |
| `0x801F77E8` | 951 | `0x5B` Scythe Wind | 2436 B | `sltiu 6` | `FUN_801DD4B0(0x80)` at `0x801F7F88` |
| `0x801F7118` | 952 | `0x5C` Bloody Horns | 2576 B | `sltiu 7` | `FUN_801DD6B4(0x1D0)` at `0x801F7948` |
| `0x801F8F0C` | 955 | `0x60` White Shield | 920 B | `beq`/`slti` chain | none |
| `0x801F86A4` | 955 | `0x6E` Kiss of Death | 2152 B | `beq`/`slti` chain | none |
| `0x801F7FA4` | 955 | `0x6F` Melt Spray | 1792 B | `beq`/`slti` chain | none |
| `0x801F767C` | 955 | `0x70` Terror Scream | 2344 B | `beq`/`slti` chain | none |
| `0x801F7158` | 955 | `0x72` Power Charge | 1316 B | `beq`/`slti` chain | none |
| `0x801F6A28` | 955 | `0x73` Void Accessories | 1840 B | `beq`/`slti` chain | none |

Two more bodies the same maps name are not on the port worklist only because
no dump prints at their VAs: `0x801F69EC` (938, `0xB7` Mystic Circle,
`FUN_801DD4B0(0x309)`) and `0x801F69D8` (965, `0xB6` Doomsday,
`FUN_801DD4B0(0x600)` at `0x801F77B4`).

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
1-sector null stub, and PROT 0952, whose two spawn sites both load `a2` out of
a saved register no static window can see.

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
