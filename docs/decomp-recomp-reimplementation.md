# Decomp, recomp, reimplementation: one game as a case study

Legend of Legaia has been ported twice over the same months by two unrelated efforts: a static recompilation (the Legaia fork of PSXRecomp) and this from-scratch engine. This page sets out what each route gets up front, what it has to earn, and why building a new engine is more practical than it used to be. The method itself is on [`porting-by-reimplementation.md`](porting-by-reimplementation.md).

The [site page](https://andrewaltimit.github.io/legend-of-legaia-re/tooling/decomp-recomp-reimplementation.html) carries the same argument with diagrams.

## Three established routes

There are three established ways to carry a console game's logic onto new machines, and each makes something different. [Matthew Stanley](https://1379.tech/recomp-vs-decomp-wrong-question/), whose PSXRecomp is the recompiler here, writes of decompilation and recompilation that "They produce different things — infrastructure and understanding". Reimplementation produces a third thing: an engine of its own, running the original's data.

Reimplementation has a long record: [OpenMW](https://openmw.org/faq/), [OpenLoco](https://github.com/OpenLoco/OpenLoco), [OpenRCT2](https://github.com/OpenRCT2/OpenRCT2), ScummVM and, on PlayStation, [Severed Chains](https://github.com/Legend-of-Dragoon-Modding/Severed-Chains). Legaia now lets the route be compared directly with a recompilation of the same game.

A port is a result, not a method: a PC port can be built on a decompilation, a recompilation or a new engine. Engine recreations are often divided into [top-down and bottom-up](https://en.wikipedia.org/wiki/Game_engine_recreation); this project is bottom-up in what it reads and new in what it writes.

## What actually runs

| | Decompilation | Static recompilation | Reimplementation |
|---|---|---|---|
| **Game code at play time** | The logic as hand-rewritten C, compiled for the host | The machine code translated to C; registers and addresses survive | None; new code follows rules taken from the original (this project uses Rust) |
| **Beneath it** | A platform layer for each host | A PlayStation hardware runtime (in PSXRecomp, the BIOS is recompiled too) | The engine's own renderer, audio and saves |
| **Game data** | Assets, often from the user's copy | The disc, through a modelled CD drive | The disc, including any scripts, run on new interpreters |

## One game, two ports, the same months

PSXRecomp and this project both started in spring 2026, the Legaia recomp builds on PSXRecomp, and all three are still going. Three threads run through them, with dates from each project's commit log.

| Thread | PSXRecomp (framework) | Legaia recomp (built on PSXRecomp) | Legaia engine (this project) |
|---|---|---|---|
| **Playing the game** | *Tomba!* gameplay (May) | Legaia field, battles and summons (July); public playtest builds (July, August) | Walkable overworld (May); a pad-only run reaches the credits (October) |
| **Fidelity** | Hardware accuracy (May onward) | Legaia's CD and XA sequencing (July) | Audio oracles (May); ladder, retail corpus, soak harness (September) |
| **Modding** | Widescreen and renderer backends (June, July) | A modding SDK, on a public branch since October | Disc patcher and randomizer (May, June); language packs (July); expansion packs (October) |

The two come at the same goals in different orders. A recomp starts with the original's code, so retail behaviour is there from the first frame and the work goes into the hardware model; it is the quickest way to play a game as it was. The engine started with nothing running and built up both the playthrough and its fidelity one mechanism at a time. Modding differs in kind: disc patches change the retail game's bytes and arrived early, and expansion packs need the engine. The recomp's README credits this project's docs with having "materially shortened the investigation", and the recomp is one of this project's best references.

## What each route gives you and what takes extra work

**Built in** is part of taking the route, **extra work** is reachable at a cost, and a **poor fit** works against the route.

| You want | Decompilation | Static recompilation | Reimplementation |
|---|---|---|---|
| The whole game running | Extra work: all the code plus a platform layer | Built in, once the runtime exists and code is found | Extra work: every routine ported or handed to a library |
| Retail quirks and timing | Built in, with timing up to the platform layer | Built in, if the hardware model is right | Extra work, one at a time; full visual parity comes last |
| Assurance the code is right | Built in when matching | Built in; the runtime carries the risk | Poor fit: measured per routine and per state, never proved |
| Rules a person can read | Extra work after the match | Poor fit | Built in |
| New rendering | Extra work | Extra work, well served at draw-call level | Built in |
| Running on new systems | Extra work | Extra work | Built in |
| Modding by game concept | Extra work | Extra work, through hooks | Built in |
| Targeting another game on the same console | Extra work: tools carry, work does not | Built in: the framework carries | Poor fit: low layers and method carry |
| No original code executing | Poor fit | Poor fit | Built in |

The last row is not a legal claim. No court has settled any of this; the nearest test, Take-Two's case over re3 and reVC, [ended in dismissal](https://torrentfreak.com/take-two-dismisses-claims-against-lead-defendants-in-gta-mods-lawsuit-230405/) with no ruling on the merits.

## The same problems, paid for differently

- **Battle items and CD audio.** Legaia's CD callback can wait for a vblank inside an exception handler; handling that in the recomp's hardware model fixed battle items, Spirit actions, Hyper Arts and summons together. The engine runs no libcd, so that pattern never arises, and it also lacked the timing that comes with it: battle frames that [run long during a CD-XA clip](subsystems/battle-stage-camera.md), which had to be measured and modelled.
- **Code that is not in the executable.** The translator sees the boot executable, so overlays are captured as they load, then compiled or interpreted. Here they are [extracted from the disc](tooling/static-overlay-pipeline.md) and read once, at the price of [address aliasing](reference/overlay-va-aliases.md).
- **A bug no local test could see.** The GTE model passed a sweep that agreed with it. The recomp's real input and output pairs exposed the signed 11-bit saturation of the screen-coordinate FIFO, through the [recomp differential](tooling/recomp-differential.md). The lesson is kept in the engine's own tooling: the interpreter that runs retail routines beside their ports carries a GTE written from the hardware documentation, not from the port, because a reference copied from the code under test shares its defects.

## How a mistake shows up

In a matching decompilation, a misreading fails the byte match at once. In a recompilation, a mistake is usually a crash, freeze or visible glitch. In a reimplementation the game runs and is quietly different, which is the historic objection to the route and the reason for [the retail-oracle method](tooling/retail-oracle-method.md). A score speaks only for what it sampled; a matching decompilation's proof is stronger.

What the measurement consists of matters to the comparison, because one part of it borrows the other routes' strength. The [retail kernel differential](tooling/retail-kernel-differential.md) runs the original's routines off the user's disc in an R3000 interpreter (`crates/retail-exec`), beside the Rust that ports them, on generated inputs. That is the original's code executing, as in a recompilation, but only inside a test and one routine at a time. It asks the question a matching decompilation answers by construction, whether this function computes what the original computes, and answers it per sampled input instead of by proof, including inputs no capture ever held.

One routine on its own cannot show when the engine calls it, or with what. Its shadow walks go part of the way, running a whole cast module beside the engine's own state and comparing every pass; the rest is measured where a player would notice it: stills seeded from retail save states, films of a cast or a walk paired frame by frame, sound scored against the emulator's recorded output, and a pad-only run from New Game to the credits. [The coverage map](porting-by-reimplementation.md#what-is-tested-and-the-gap-each-test-closes) lists every instrument with the gap it closes and what it stays blind to.

## Three quiet defects, and what found them

Each of these left the game running and playable, which is the failure the section above describes, and each was found a different way. [One routine, end to end](porting-by-reimplementation.md#one-routine-end-to-end) adds a fourth, which the routine differential found on its own. Addresses are from the disassembly; the visible effects are from captures and play.

**A caster facing the wrong way: a film showed it, the disassembly explained it.** Water Hazard is a
monster spell whose presentation is the cast module in PROT 0956. Its arm 0 (the first branch of the
module's state machine) turns the caster onto its target before the first cut: for a group target it
measures the group's centroid (`jal 0x801DCEAC` at `0x801F73A0`) and takes the bearing to it, plus a
half turn (`0x801F73D4..0x801F73E4`). Every later shot is framed from that facing. The port handed
the centroid to four other modules but not to this one, so the caster turned onto the victim's seat
instead: facing `2533` against retail's `2364`, 169 units of a 4096-unit turn (about fifteen
degrees), and every shot of the cast framed off by that much. The [cast-sequence
oracle](tooling/cast-sequence-oracle.md) showed the offset on every paired frame, and for a while
these docs blamed the instrument for it. Reading arm 0 found the cause, and two residuals written up
separately (a brighter water column and a ring two frames early) turned out to be the same camera.
Checking every other module that calls `FUN_801DCEAC` found the same missing turn in five more
casts, and a disc-gated test now derives that list from the module bytes
([`cast-module.md`](subsystems/cast-module.md)).

**A villager drawn as a prop: a player saw it, and no oracle could have.** A field script can swap
the model a placement draws (`CC <id> 50`). Retail allocates every placement's actor fresh on each
map install from the placement header (`FUN_8003A1E4`: the header's model byte plus the bank base,
stored at `0x8003A324`, then `jal 0x80024C88` at `0x8003A398`), so a swap ends with its scene. The
port keeps one long-lived table of live models by placement slot and never cleared it. After a beat
in `uru` that swaps the models of slots 7 to 14, the next scene's villagers in those slots resolved
the old id through the new scene's bank, and in `bubu1` one walked its loop drawn as a one-object
prop. It showed only after that route, and the project owner found it playing. The stills seed each
scene fresh from a retail state, and no captured state stands in `bubu1` after that beat, so the
corpus had nothing to compare. Scene entry now clears the table, and a disc-gated test drives that
route ([`script-vm-menuctrl.md`](subsystems/script-vm-menuctrl.md)).

**A second fight that remembers the first: an audit against retail's allocations found it.**
Retail's battle init allocates the battle block per fight and zeroes all of it (`FUN_80055B6C`: a
`sw zero` loop at `0x80055F14..0x80055F24` bounded by `sltiu a1,0x1E8D`; the context, block base
plus `0x37B4`, is stored to `0x8007BD24` at `0x80055EC8`). The engine keeps that context's mirrors
on a structure that outlives a fight, so a second fight opened on the first one's values: its first
hit was priced with the last strike's guard ramp, its first prompt held one pass on the last fight's
glide gate, and its frame-cost history began where the last fight ended rather than where a first
fight begins. A still seeds one fight from a state and cannot see it; the full-game ladder plays
many fights and compares nothing with retail. It was found by listing what retail re-allocates or
zeroes at each scene and mode entry and checking the engine field by field ([state a scene change
carries](tooling/live-audit-triage.md#state-a-scene-change-carries)). A test now ends a fight by a
flee and by a win, enters a second, and holds every field of the battle state to a fresh first
entry's, with each allowed difference named.

The last two are one defect class, and it belongs to this route. A decompilation or a recompilation runs retail's own allocations and zeroing loops, so it inherits every lifetime they imply. An engine that keeps one long-lived game state has to reset each field where retail re-creates it, and nothing fails when it does not. The countermeasure is the audit above, run against retail's allocation and clear sites rather than against the engine's own idea of a scene. The first case shows the other half of the method: an oracle can point at a difference for a long time before anyone reads the right instructions, and the reading, not the score, settles it.

## Why reimplementation is more viable now

The standing problems were years of labour before the first reward, and differences from the original that go unnoticed. The labour (read a routine, write the equivalent, wire it, repeat) is the kind coding agents can do at volume, provided each result is checked against something outside the agent. The instruments that catch those differences (state readers, scoring channels, ratchets, replay) are software an agent can also write. An agent is a fast, confident writer and needs something external to answer to, which a measured reimplementation provides. Retail captures, the decision about what counts as right, and judgment over a green test remain with people.

This does not make AI specific to one route. This project is written with Claude Code, as was the [PSXRecomp prototype](https://1379.tech/i-built-a-ps1-static-recompiler-with-no-prior-experience-and-claude-code/). Matching decompilation has an unambiguous score for an agent: [Chris Lewis](https://blog.chrislewis.au/decompiling-a-nintendo-64-game-in-84-days/) matched one N64 game in 84 days against 596 for its sequel, with caveats he states himself. The claim is narrower: all three got cheaper, and reimplementation had the most to gain, because labour was what ruled it out for small teams.

The community cost is real. [decomp.dev](https://decomp.dev/) is sunsetting its main index, writing that "the community aspect of decompilation has been broken", and [DarioSamo](https://www.videogameschronicle.com/news/banjo-kazooie-pc-port-decomp-developer-says-ai-generated-pc-ports-are-a-race-to-the-bottom-and-disrespectful/) of Banjo: Recompiled declined "a race to the bottom".

## Choosing a route

- The game on modern machines as it was: static recompilation, if a framework for the console exists.
- Exactly what the code says: matching decompilation, if the compiler can be pinned down.
- The game becoming more than it was: reimplementation, especially when much of it is data and the original can be measured against.
- Playing it tonight: an emulator or an official release.

The routes combine. This project uses a recomp as an oracle, Ghidra's output as a pointer, an emulator as the judge, and the original's own routines, run in an interpreter, as the check on each port.

## What this case cannot show

It is one heavily data-driven game, which flatters this route. PSXRecomp is a general framework and this repository serves one game, and neither logged hours, so no labour comparison is possible. As far as this project knows there is no public decompilation of Legaia. Reaching the credits is not being identical to retail. Both projects are moving, so this is a snapshot. And this page is written by the project it argues for.

## See also

- [`porting-by-reimplementation.md`](porting-by-reimplementation.md)
- [`tooling/retail-oracle-method.md`](tooling/retail-oracle-method.md)
- [`tooling/verification-ladder.md`](tooling/verification-ladder.md)
- [`tooling/retail-kernel-differential.md`](tooling/retail-kernel-differential.md)
- [`tooling/reusing-for-other-ps1-games.md`](tooling/reusing-for-other-ps1-games.md)
- [`tooling/recomp-differential.md`](tooling/recomp-differential.md)
