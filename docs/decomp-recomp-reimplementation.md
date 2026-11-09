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
| **Game code at play time** | The logic as hand-rewritten C, compiled for the host | The machine code translated to C; registers and addresses survive | None; new Rust follows the rules read from the disassembly |
| **Beneath it** | A platform layer for each host | A PlayStation hardware runtime (in PSXRecomp, the BIOS is recompiled too) | The engine's own renderer, audio and saves |
| **Game data** | Assets, often from the user's copy | The disc, through a modelled CD drive | The disc, including script and effect bytecode run on new VMs |

## One game, two ports, the same months

PSXRecomp and this project both started in spring 2026, the Legaia recomp builds on PSXRecomp, and all three are still going. Three threads run through them, with dates from each project's commit log.

| Thread | PSXRecomp (framework) | Legaia recomp (built on PSXRecomp) | Legaia engine (this project) |
|---|---|---|---|
| **Playing the game** | *Tomba!* gameplay (May) | Legaia field, battles and summons (July) | Walkable overworld (May); a pad-only run reaches the credits (October) |
| **Fidelity** | Hardware accuracy (May onward) | Legaia's CD and XA sequencing (July) | Audio oracles (May); ladder, retail corpus, soak harness (September) |
| **Modding** | Widescreen and renderer backends (June, July) | A modding SDK, in development since October | Disc patcher and randomizer (May, June); language packs (July); expansion packs (October) |

The two come at the same goals in different orders. A recomp starts with the original's code, so retail behaviour is there from the first frame and the work goes into the hardware model; it is the quickest way to play a game as it was. The engine started with nothing running and built up both the playthrough and its fidelity one mechanism at a time. Modding differs in kind: disc patches change the retail game's bytes and arrived early, and expansion packs need the engine. The recomp's README credits this project's docs with having "materially shortened the investigation", and the recomp is one of this project's best references.

## What each route gives you and what takes extra work

**Built in** is part of taking the route, **extra work** is reachable at a cost, and a **poor fit** works against the route.

| You want | Decompilation | Static recompilation | Reimplementation |
|---|---|---|---|
| The whole game running | Extra work: all the code plus a platform layer | Built in, once the runtime exists and code is found | Extra work, and arrives last |
| Retail quirks and timing | Built in, with timing up to the platform layer | Built in, if the hardware model is right | Extra work, one at a time |
| Assurance the code is right | Built in when matching | Built in; the runtime carries the risk | Poor fit: measured, never proved |
| Rules a person can read | Extra work after the match | Poor fit | Built in |
| New rendering | Extra work | Extra work, well served at draw-call level | Built in |
| New hosts | Extra work | Extra work | Built in |
| Modding by game concept | Extra work | Extra work, through hooks | Built in |
| Targeting another game on the same console | Extra work: tools carry, work does not | Built in: the framework carries | Poor fit: low layers and method carry |
| No original code executing | Poor fit | Poor fit | Built in |

The last row is not a legal claim. No court has settled any of this; the nearest test, Take-Two's case over re3 and reVC, [ended in dismissal](https://torrentfreak.com/take-two-dismisses-claims-against-lead-defendants-in-gta-mods-lawsuit-230405/) with no ruling on the merits.

## The same problems, paid for differently

- **Battle items and CD audio.** Legaia's CD callback can wait for a vblank inside an exception handler; handling that in the recomp's hardware model fixed battle items, Spirit actions, Hyper Arts and summons together. The engine runs no libcd, so that pattern never arises, and it also lacked the timing that comes with it: battle frames that [run long during a CD-XA clip](subsystems/battle-stage-camera.md), which had to be measured and modelled.
- **Code that is not in the executable.** The translator sees the boot executable, so overlays are captured as they load, then compiled or interpreted. Here they are [extracted from the disc](tooling/static-overlay-pipeline.md) and read once, at the price of [address aliasing](reference/overlay-va-aliases.md).
- **A bug no local test could see.** The GTE model passed a sweep that agreed with it. The recomp's real input and output pairs exposed the signed 11-bit saturation of the screen-coordinate FIFO, through the [recomp differential](tooling/recomp-differential.md).

## How a mistake shows up

In a matching decompilation, a misreading fails the byte match at once. In a recompilation, a mistake is usually a crash, freeze or visible glitch. In a reimplementation the game runs and is quietly different, which is the historic objection to the route and the reason for [the retail-oracle method](tooling/retail-oracle-method.md). A score speaks only for what it sampled; a matching decompilation's proof is stronger.

## Why reimplementation is more viable now

The standing problems were years of labour before the first reward, and differences from the original that go unnoticed. The labour (read a routine, write the equivalent, wire it, repeat) is the kind coding agents do well. The instruments that catch those differences (state readers, scoring channels, ratchets, replay) are software an agent can also write. An agent is a fast, confident writer and needs something external to answer to, which a measured reimplementation provides. Retail captures, the decision about what counts as right, and judgment over a green test remain with people.

This does not make AI specific to one route. This project is written with Claude Code, as was the [PSXRecomp prototype](https://1379.tech/i-built-a-ps1-static-recompiler-with-no-prior-experience-and-claude-code/). Matching decompilation has an unambiguous score for an agent: [Chris Lewis](https://blog.chrislewis.au/decompiling-a-nintendo-64-game-in-84-days/) matched one N64 game in 84 days against 596 for its sequel, with caveats he states himself. The claim is narrower: all three got cheaper, and reimplementation had the most to gain, because labour was what ruled it out for small teams.

The community cost is real. [decomp.dev](https://decomp.dev/) is sunsetting its main index, writing that "the community aspect of decompilation has been broken", and [DarioSamo](https://www.videogameschronicle.com/news/banjo-kazooie-pc-port-decomp-developer-says-ai-generated-pc-ports-are-a-race-to-the-bottom-and-disrespectful/) of Banjo: Recompiled declined "a race to the bottom".

## Choosing a route

- The game on modern machines as it was: static recompilation, if a framework for the console exists.
- Exactly what the code says: matching decompilation, if the compiler can be pinned down.
- The game becoming more than it was: reimplementation, especially when much of it is data and the original can be measured against.
- Playing it tonight: an emulator or an official release.

The routes combine. This project uses a recomp as an oracle, Ghidra's output as a pointer and an emulator as the judge.

## What this case cannot show

It is one heavily data-driven game, which flatters this route. PSXRecomp is a general framework and this repository serves one game, and neither logged hours, so no labour comparison is possible. As far as this project knows there is no public decompilation of Legaia. Reaching the credits is not being identical to retail. Both projects are moving, so this is a snapshot. And this page is written by the project it argues for.

## See also

- [`porting-by-reimplementation.md`](porting-by-reimplementation.md)
- [`tooling/retail-oracle-method.md`](tooling/retail-oracle-method.md)
- [`tooling/reusing-for-other-ps1-games.md`](tooling/reusing-for-other-ps1-games.md)
- [`tooling/recomp-differential.md`](tooling/recomp-differential.md)
