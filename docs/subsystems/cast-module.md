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
resident **the word at `0x801F69D8` is the module's own word-0 entry VA** -
a third meaning for an address the corpus already maps twice (PROT 0900's
jump-table head, and the world-map band's `FUN_801F69D8`). A probe watching
that word sees `0x001000E2` (empty), then the module's entry VA at paging,
then - for a tableless module - its first instruction word.

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

## Provenance

Disassembly of the PROT 958/959/960 images (offsets above); the commit
mirror via `see ghidra/scripts/funcs/8004ad80.txt`; natural-cast playouts
captured per-frame under PCSX-Redux (`autorun_delilas_enemy_cast_watch.lua`)
pin the staging walks, the loop counter, and the phase-5 confirm gate.
Patcher mirror: `legaia_patcher::delilas_cast` (expect-verified word edits
against these images); staged player rows `legaia_asset::party_swap::cast_stage`.
