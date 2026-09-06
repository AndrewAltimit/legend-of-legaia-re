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
from outside the image - see [the two entries](#the-two-entries-and-where-their-addresses-live).

| Module | Size | Head shape | Entry |
|---|---|---|---|
| PROT 958 | `0x3000` B | 256-entry VA dispatch table fills file `0x0..0x400` (default arm `0x801F8CFC`) | `0x801F6E9C` = file `+0x4C4` |
| PROT 959 | `0x3000` B | 6-entry head table (`0x801F8290, 82C4, 8478, 850C, 8600, 878C`), its own dispatcher at `0x801F8250` | file `+0x18B8` |
| PROT 960 | `0x2800` B | none - code at file `+0` | `0x801F69D8` (= base) |

Module code cites below are given as file offsets with their VA
(`0x801F69D8 + off`); the three images share the base, so an offset is
meaningless without naming its module. The ~936-byte tail region past
`~+0x2A00` is **shared library code** across the modules (the same words in
958 and 959), not per-move logic.

## The two entries, and where their addresses live

A module is a **library, not a program**: nothing inside it names its own
entry points. Neither entry VA appears anywhere in any extracted image - not
as a word, not as a `lui`+`addiu` pair, not as a `jal`/`j` target (five-form
sweep, `scripts/ghidra-analysis/find-address-word-refs.py`) - except inside
**PROT 0898**, where both are fixed at link time. A module therefore has two
callable routines, reached by two different mechanisms.

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
move-table instruction's two signed halfwords. Every module's routine here
opens `sltiu vX, a1, N`, so the operand is an arm index: this entry is the
per-spell **spawn stager** the move script drives, not the choreography.
(The corpus previously called `0x8007BA2C` "the per-summon effect-data
pointer"; it is a code pointer, and `0x801F6734` is a table of entry VAs.)

Both indexings land on the same entry: **table row `i` is extraction entry
`903 + i`** for `i = 0..0x3F`, i.e. PROT 0903..0966. The `move_id - 0x81`
band `0x81..0xA0` covers 0903..0934, the capture-class `sub_id + 0x20` band
covers 0935..0966, and the pager's own `id - 0x79` (`FUN_8003EC70`, `+0x381`
into TOC space) agrees with both.

The `0x801CF4EC` switch reaches only PROT 0903..0934. How a **capture-class**
module's tick is entered is not settled by this: that band is driven through
battle phase `0x70` as described above, and no hard-coded `jal` into it was
found.

That rule is checkable against the disc, and it checks out on every
extracted module: for the thirteen slot-B images in
`crates/asset/data/static-overlays.toml`, all thirteen `0x801F6734` rows and
all twelve `0x801CF4EC` arms that have an extracted image land exactly on a
function prologue recovered from that image's own bytes. It is also the
cheapest identity test available for the fifty-one modules nobody has
extracted yet.

## Image anatomy, recovered from the bytes

These images have **no internal `jal` at all** (0957 is the single
exception, two): every call leaves for SCUS or for the resident battle
overlay. Ghidra's auto-analysis therefore finds at most the routine some
other reference reaches, and the dumps have to be driven from an address
list rather than from the call graph.

The bytes supply that list. In every one of the thirteen images the count of
`addiu sp, sp, -X` words equals the count of `jr ra` words, and they
interleave: function `i` runs from prologue `i` to prologue `i + 1`, and the
last ends 8 bytes past the last `jr ra`. That partition is exact - every
range so cut ends on a `jr ra` plus its delay slot - and it is what the
entry tables then confirm. So a module is **two functions** (0903 has one
real function plus a `jr ra; nop` stub at its `0x801F6734` row; 0957 has
four), laid out as:

| Region | Contents |
|---|---|
| head | the jump table of **one** of the two switches, 0 to 256 words; the first function's prologue is the first word past it |
| tick | the `ctx+0x279` phase machine (function A), reached from `0x801CF4EC` |
| stager | the `sltiu a1, N` spawn switch (function B), reached from `0x801F6734` |
| tail | data: the spawn/emitter records the module hands to `FUN_80050ED4` and `FUN_80021B04`, plus its own scratch words |

Which switch owns the head table varies, and the arm count settles it: PROT
0934's table is 26 words and its tick bounds on `sltiu a0, 0x1A`, while PROT
0929's is 9 words and its **stager** bounds on `sltiu a1, 9` (0928: 7 and 7).
Reading the head table as the tick's is therefore wrong half the time; read
the `sltiu` immediate.

The tail is the bulk of the residue `disc-coverage.py` still reports on
these images, and it is data: PROT 0934's is `0x801F9C08..0x801FA9D8`, and
the `lui 0x8020` + negative-displacement operands its stager passes as `a2`
resolve into exactly that span.

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

Each hit is one call into the guard-bypassing roll wrapper `FUN_801DD6B4`
with a **baked per-hit power constant** in `a0` (958 escalates
`0x30, 0x38, 0x38, 0x38, 0x40, ..`; 960 lands one `0x1C0` burst), then the
same apply shape every time: load the victim, clamp the roll against HP
`+0x14C`, accumulate into the victim's damage-popup word `+0x10`, load the
victim again, write HP back. Which wrapper a module calls - and the
per-module call census - is
[battle-formulas.md](battle-formulas.md)'s table.

**The victim load is hardcoded to seat 0.** Every apply site loads
`actor_table[0]` (`lw rX, 0x9370(base)`) instead of the derived victim:
twelve sites in 958 (six clamp/write pairs), five in 959, two in 960
(`+0x17AC`/`+0x17DC`). Retail never notices because a boss cinematic's
victim is always the party - seat 0 - but any reuse that points the cast at
a monster (or any multi-target future) inherits friendly fire from these
sites. The same seat-0 assumption shapes the finale: a dead-victim arm
declares game over on the spot, correct only while the victim is a hero.

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
