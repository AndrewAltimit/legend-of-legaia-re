# Reference

Cross-cutting reference: function directory, memory map, curated game data, and the registers of open, settled and falsified RE questions.

Two pairs carry most of the lookups. For an address, [`functions.md`](functions.md) names the routine and [`memory-map.md`](memory-map.md) the global. For a question, read [`open-rev-eng-threads.md`](open-rev-eng-threads.md) for what is still being hunted, [`re-settled-threads.md`](re-settled-threads.md) for what has been answered and on what evidence, and [`re-do-not-re-walk.md`](re-do-not-re-walk.md) before chasing a reading that looks obvious: it may already be falsified.

| Doc | Covers |
|---|---|
| [`functions.md`](functions.md) | Notable Ghidra-traced function entry points (the canonical directory) - index page over the per-subsystem tables in `docs/reference/functions/`. |
| [`memory-map.md`](memory-map.md) | RAM map + key globals. |
| [`builds.md`](builds.md) | Region data; known builds. |
| [`cheats.md`](cheats.md) | GameShark / Mednafen cheat database parser + classifier; pinned RAM offsets for character record, inventory, battle actor, story flags. |
| [`gamedata.md`](gamedata.md) | Curated arts/magic/items/weapons/armor/accessories/enemies/shops/casino/fishing tables mined from public walkthroughs, each field with a disc source pinned to it by a disc-gated test. Labels for binary records under reverse engineering. |
| [`music-tracks.md`](music-tracks.md) | Every BGM cue across its four naming spaces (sound-test id, in-game context, OST title, relocalization), joined to the disc. The `music_01` bank is **piecewise** in extraction space. Resolver `engine-system::music_labels`. |
| [`scene-names.md`](scene-names.md) | The one scene-name table (`data/gamedata/scenes.toml`, `legaia_gamedata::scene_names`): display name, disc banner, in-game reading - and the labels that mislead. |
| [`open-rev-eng-threads.md`](open-rev-eng-threads.md) | The live RE hunts (`open` / `partial` / `mostly resolved`), plus what an evidence grade means. Question-level companion to `port-catalog.py --dashboard`. |
| [`re-settled-threads.md`](re-settled-threads.md) | Answered RE questions, each graded `disassembly` / `capture` / `decompiled-C` / `inference` by what its own evidence rests on. Index page over the per-area pages in `docs/reference/re-settled-threads/`. `decompiled-C` = the re-audit bucket. |
| [`glitches.md`](glitches.md) | Community-reported glitches and oversights traced to their cause, each with a status, addresses and a debugger check. Index page over the per-area pages in `docs/reference/glitches/`. Each claim is graded; `capture` entries say whether the state was played, poked or an edited save. |
| [`re-do-not-re-walk.md`](re-do-not-re-walk.md) | Falsified hypotheses with their reasoning intact - the plausible readings of the bytes that turned out wrong. |
| [`overlay-va-aliases.md`](overlay-va-aliases.md) | Phantom virtual addresses in `ghidra/scripts/funcs/`: real bytes and real disassembly under a VA that belongs to no runtime image. |
