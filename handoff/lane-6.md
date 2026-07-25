# Lane 6 handoff

Two tasks: the six SCUS-band worklist rows, then the SsAPI sequencer cluster.
The sequencer half is at the bottom.

## Part 1 - the six SCUS-band worklist rows

All six are **game logic**, not PsyQ / BIOS / libgte vendor infrastructure. None
belongs in `scripts/ci/port-catalog-ignore.toml`. `0x80056208` is the one worth
saying that about explicitly: it sits between the PsyQ veneers
(`0x80056798` = `rand`, `0x800567A8` = `printf`, `0x80058104` = `DrawSync`,
`0x800583C8` = `LoadImage`) and `re-settled-threads.md` calls it a "libgpu-band
SCUS→overlay bridge", but its body reads the battle context `_DAT_8007BD24`,
dispatches into three battle overlay hooks and points a caption pointer at a game
string. It is a battle state machine that happens to live in that address band.

Everything below is disassembly-grounded unless flagged otherwise.

## Where each landed

| addr | port | wired? |
|---|---|---|
| `80016230` | `legaia_engine_render::mode_transition` | disclosed `NOT WIRED` |
| `80020f88` | `legaia_engine_render::actor_bind` | disclosed `NOT WIRED` |
| `800480d8` | `legaia_engine_render::battle_actor_tick` | disclosed `NOT WIRED` |
| `8004ccd4` | `legaia_engine_render::attach_swap` | disclosed `NOT WIRED` |
| `80056208` | `legaia_engine_render::battle_sideband` | disclosed `NOT WIRED` |
| `800508dc` | `legaia_engine_audio::anim_cue` | disclosed `NOT WIRED` |

## Ports whose natural home is a sibling's file scope

Each of the six is a decision / arithmetic kernel whose retail host is the battle
context, the actor pool or the mode dispatcher - all `engine-core` or
`engine-shell`. Per the lane brief they landed in `engine-render` /
`engine-audio` with an honest disclosure rather than reaching into a sibling's
files. The five natural homes, if a later wave wants to move or wire them:

- **`800508dc` → `legaia_engine_core::sfx_cue`.** This is the closest to being
  wired: it is the *producer* that feeds `FUN_8004FE5C`, and `sfx_cue` is already
  that function's port. Wiring needs two fields the engine's battle actor does not
  carry - the playing entry's cue track (`entry + 0x54`) and the cursor
  (`actor + 0x1F6`). `legaia_engine_audio::AnimCueState` owns the cursor already,
  so the missing half is the battle-form assembler keeping the cue track next to
  the mesh it already splices.
- **`8004ccd4` → the battle draw path.** It writes into a per-channel model table
  (`*(node+0x44) + 4`). The engine draws an assembled whole-character mesh, so
  there is no channel table. Keeping `battle_char_assembly`'s pieces as channels
  rather than merging them is what makes the swap expressible.
- **`80020f88` → `legaia_engine_core`'s scene host.** Wants `bind_actor_render`
  called at actor spawn, honouring `ActorBind::render_node`.
- **`800480d8` → the battle render loop.** The port is the *schedule*
  (`Vec<BattleDrawStep>`); the five passes it sequences (`FUN_8004A908`,
  `FUN_80048A08`, `FUN_80049348`, `FUN_8005112C`, `FUN_801F7B88`) are unported.
- **`80056208` → the battle host.** Pure transition function; needs an owner for
  `BattleSidebandState` and an applier for `BattleSidebandEffects`.
- **`80016230` → `legaia_engine_core`'s mode dispatcher.** Note the engine does
  not need the VRAM stash at all: it keeps the actor pool in host memory across a
  mode change instead of parking it in spare VRAM, so `ACTOR_POOL_STASH_RECT` is
  documented rather than executed.

## Doc rows for pages outside this lane's scope

### `docs/subsystems/level-up.md:503` and `crates/engine-core/src/seru_stats.rs:41`

Both describe `FUN_800480D8`'s `actor+0x74` write as a `0x80808080` battle-state
flag. The instruction pair they quote is right, but the value it builds is
**`0x00808080`**: `lui v0,0x80` loads immediate `0x80` into the upper halfword, so
`v0 = 0x00800000`, and `ori v0,v0,0x8080` gives `0x00808080`. The mask the same
function tests it under is built the same way - `lui v1,0xff ; ori v1,v1,0xffff` =
`0x00FFFFFF` - which pins the field as a 24-bit RGB colour word holding mid-grey,
the same `0x808080` `FUN_801E1AB0` uses for the after-image ghost.
`docs/subsystems/battle.md:2024` already calls it "the death / `0x808080`
greyscale path" and is correct. Sites at `0x80048238`/`0x8004823C` and
`0x800482D4`/`0x800482D8`; mask at `0x800481BC`/`0x800481C4`.

### `docs/reference/functions/battle.md:152` (`800508DC`)

The existing row is broadly right but incomplete in two ways worth folding in.
Proposed replacement:

> | `800508DC` | **Battle animation cue-track walker.** `(actor_id, entry, key)`.
> Walks the playing entry's 8-slot `(u16 frame, u16 cue)` track at `entry+0x54`
> from the persistent cursor `actor+0x1F6` (actor via `DAT_801C9370[actor_id]`),
> firing every cue the clip has reached. On a party seat the `0xC8..=0xFF` band
> (minus the `0xFA` hole) re-bases by `+0x38` into the `>= 0x100` arts-voice
> namespace `FUN_8004FE5C` routes to CD-XA; ids `0xD7`/`0xE7`/`0xF7` are the
> Vahn / Noa / Gala **shout**, taking a `rand()%2` two-take coin flip (channels
> 7/6 of clip 26/27/28), a tally at record `+0x98` and a mute bit at record
> `+0xF8 & 0x2000`. While the CD is busy (`_DAT_8007BC20 != 0`) the shout degrades
> to a ring cue through `FUN_8004FCC8` - Vahn `0x56`, Noa `0x62`, Gala `0x5C`.
> Sub-band ids take a `+1` nudge on staged anim id `0x12`, or a suppression at
> `>= 0x4D` under the mute bit. Ported as `legaia_engine_audio::anim_cue`. Full
> decode: [`audio.md`](../../subsystems/audio.md). `800508dc.txt`. |

Two corrections inside that: the row currently names only `FUN_8004FCC8`, but the
body's **primary** exit is `FUN_8004FE5C` (`jal 0x8004fe5c` at `0x80050B3C`, on
every path except the CD-busy fallback); and the "RNG tiebreak" is not a tiebreak
- it is the two-take channel pick, and it is drawn only for the three shout ids.

### `docs/reference/functions/battle.md` - no row for `80056208`

Proposed:

> | `80056208` | **Battle sideband tick.** `(void)`. Three submodes off
> `DAT_8007B64A`. `1` = the intro sequence, four phases on `ctx+0x289`: arm the
> tutorial caption (`ctx+0x6AE = 0xB40`, `ctx+0x1B = 1`, `ctx+0x1C = 0x10`,
> `DAT_80077494` → the tutorial string, `FUN_801D8DE8(0x5A, 0)`) and aim the
> camera at actor-table slot 3 through `FUN_801D829C`; wait out the caption
> (`8 * DAT_1F800393` per frame, cancelled by any pad edge, gated on
> `DAT_8007BD71 == 0xFF`) then `FUN_800355F0`; `FUN_801F6B70`; then
> `FUN_80025358` plus a `0x800840C0` ramp. `2` = in-battle: `FUN_801F69F4` once
> `DAT_8007BD71 >= 0x12`, else clear the pad masks and ramp `0x800840BC` (`+4`) /
> `0x800840C0` (`+14`) per frame step under a `< 0xC00` pre-test. `3` = outro:
> burn `ctx+0x6D8`, then `FUN_801F69D8` once `FUN_8003DE7C(1)` reports idle. The
> tail always publishes the hold flag `ctx+0x6B0`, `1` only in intro phases 0/1.
> Ported as `legaia_engine_render::battle_sideband`. `80056208.txt`. |

### `docs/reference/functions/game-modes.md` - no row for `80016230`

Proposed:

> | `80016230` | **Mode-entry prologue.** `(void)`. Clears the 16-halfword hblank
> ring `DAT_80084098` and the byte `0x800915DC`, so `FUN_80016B6C`'s adaptive
> frame-skip restarts at `1`. On mode `0x14` (`BATTLE INIT`) word-sums the cached
> overlay-A image (`_DAT_8007B9AC`, `_DAT_8007B9DC` bytes) against
> `_DAT_8007B9A8`: a hit sets `_DAT_8007BC3C = 3` and `memcpy`s the cache into
> `*DAT_8001038C`, skipping the CD; a miss sets `-1` and re-streams overlay `3`
> via `FUN_8003EBE4`, `FUN_8003DE7C(0)`-polled either side. Dev
> (`_DAT_8007B8C2 == 0`) substitutes the expected sum, so dev always hits. Modes
> outside `{2,3}` clear `_DAT_8007B9C4`. For modes `{0x14, 8, 0x1A, 0x18}` **and
> only when `_DAT_8007B7AC == 3`** it snapshots the field: player X/Z from
> `_DAT_8007C364 + 0x14/+0x18` into `0x80084568`/`0x8008456C`,
> `_DAT_8007B8B8 = 1`, and a `DrawSync`-bracketed `LoadImage` parking the
> `0x7B0C`-byte actor-pool block at `0x8007C348` into VRAM `(960, 0, 64, 256)`.
> Ported as `legaia_engine_render::mode_transition`. `80016230.txt`. |

### `docs/reference/memory-map.md:60` (`0x8007B7AC`)

The row says the semantics are **open** ("Used as a boolean gate here; exact
semantics open"). `FUN_80016230` narrows it: the field-state snapshot is gated on
`_DAT_8007B7AC == 3`, and `3` is `MAIN MODE`, the field / town loop. A guard whose
whole job is "only preserve field state when the mode being left is the field"
reads as an **outgoing / previous game mode** register. That is an inference from
the guard's purpose, not a pinned writer - it needs a write-watchpoint to close,
but it is a sharper hypothesis than "boolean gate".

## Dump-corpus notes for the coordinator

- `800508dc.txt` was re-dumped mid-lane (L1's pass). The new disassembly is
  **instruction-for-instruction identical** to the version this lane ported from,
  which already carried a `size=` header and a full disassembly section - so the
  port is unaffected. The header gained the `[SCUS_942.54]` tag.
- `800480d8.txt` is the one of the six still **missing its `size=` header** while
  carrying a complete `--- DISASSEMBLY ---` section (142 instructions,
  `0x800480D8..0x8004830C`). If the coverage counter keys on the header it will
  score this dump as C-only, which it is not. Cheap re-dump.
- The other four (`80016230`, `80020f88`, `8004ccd4`, `80056208`) all carry
  `size=` headers.

## The Ghidra-C artifact this lane hit

`FUN_80016230`'s decompiled C renders the cache-restore `memcpy` as
`raw_memcpy(DAT_8001038C, _DAT_8007b9ac)` - **two** arguments. The real call is
three: `a2` holds `_DAT_8007B9DC`, left in the register by the checksum loop's
last `lw a2,-0x4624(a2)` at `0x800162E8`, and the decompiler drops it. Taking the
C at face value would have produced a port that copies a length it never read.
This is the dropped-register-argument artifact from
`docs/tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims`,
caught only by reading the disassembly.

## Part 2 - the SsAPI per-frame sequencer cluster

### Which five rows these are

The wave brief said "five are the SsAPI per-frame sequencer cluster" without
naming them. They are the members of Lane 1's new calc-tier section that were
**already documented before** Lane 1's dump pass - i.e. the ones cited in
`docs/subsystems/audio.md`'s per-frame call-graph paragraph at the wave base:

`80062F98`, `8006320C`, `8006352C`, `80063CEC`, `80063AA8`.

The other eight addresses Lane 1 dumped in the same pass (`800639A0`, `80063974`,
`80064090`, `8006418C`, `800638D8`, `800649B0`, `800648F0`, `8006497C`) were
newly documented by Lane 1 itself, so they are not worklist rows - though
`800649B0` is ported here anyway, because it is the tempo kernel and it matters
more than any of the five.

### Port-vs-scope verdict: port, do not ignore

**None of this cluster should go to the ignore file.** The reasoning, since the
address band is genuinely PsyQ libsnd and the reflex answer would be "vendor":

The catalogue already ignores the tier *below* this one - `80066308`'s ignore
entry reads "Sits entirely below the SsAPI surface ... engine-audio's
sequencer.rs is the clean-room replacement." That is the right line, and it is
drawn at the **SsAPI surface**. `FUN_80062F98` *is* the surface (`SsSeqCalc`).
Everything at this tier changes what a player hears:

- `800649B0` is the only place wall-clock tempo becomes an integer tick step;
- `8006320C` / `8006352C` are audible volume envelopes;
- `80063AA8` decides whether and where BGM loops;
- `80062F98`'s dispatch order and its re-read semantics decide which of those
  run, and how many times.

An engine that gets any of them wrong is audibly wrong while staying internally
consistent - the failure mode the `music_01` base-990 correction is a standing
warning about. So they are ported as pure reference kernels rather than ignored,
even though `sequencer.rs` remains the thing that actually drives playback.

### Ported

`crates/engine-audio/src/seq_calc.rs`, `NOT WIRED` (disclosed): `dispatch_channel`
/ `seq_calc` (`80062F98`), `tempo_slide_tick` + `tick_budget` (`800649B0`),
`volume_slide_tick` (`8006320C` / `8006352C`), `track_end` (`80063AA8`).
31 unit tests.

The `NOT WIRED` reason is specific: `Sequencer` is the engine's replacement for
this tier and clocks on integer SPU samples, so wiring would mean `Sequencer`
adopting the retail `0xB0`-byte channel record wholesale - a larger change than
one lane. The kernels are pure functions over a `SeqChannel` precisely so a
future retail-vs-engine divergence can be localised to one of them.

### Where this stops

**`FUN_80063CEC` (the SEQ event decoder) is not ported.** It dispatches the
event's high nibble through five *installed* handler pointers, so a faithful port
needs that handler table decoded first - a bigger surface than the four kernels
here, and `sequencer.rs` already decodes the SEQ event stream independently. It
is the obvious next pick-up in this cluster, and it is the only one of the five
worklist rows left open.

### Findings worth carrying

1. **The flag word is re-read before every test.** `FUN_80062F98` reloads `+0x98`
   from memory ahead of each `andi` rather than snapshotting it (`lw v0,0x0(s2)`
   / `addu` / `lw v0,0x98(v0)` repeats before every single bit test). The Ghidra C
   renders this as a plain sequence of `if`s on the same expression, which reads
   as a decoded mask and hides the ordering dependency.
2. **`FUN_800649B0` can run twice in one frame.** Bits `0x40` and `0x80` both
   dispatch to it, and it clears *both* on its settling paths. So a tempo tick
   that settles runs once; one that does not leaves both bits standing and runs a
   second time in the same frame, burning `+0xA8` twice. Whether both bits are
   ever set together is not determined here - it needs a capture.
3. **The `0x200` "finished" flag cannot outlive its own frame.** `FUN_80063AA8`
   sets `0x200` and `0x4` together on a track's last repeat; `FUN_80062F98`'s
   bit-`0x4` arm runs later in that same frame and zeroes the whole flag word.
   Anything reading `0x200` on a later frame is reading a bit that was already
   wiped.
4. **The step fields' sign selects rate mode, not direction.** `+0x4C` (volume)
   and `+0x4E` (tempo) both branch on `blez`: positive = one unit every `step`
   ticks (gated on `remaining % step == 0`), non-positive = `|step|` units per
   tick with a clamp at the target. `0` lands in the second arm and moves
   nothing. Direction comes from the function or the target, never the sign.
5. **`FUN_80063AA8` clears bits `0x1`, `0x8`, `0x2`** - the literals are
   `li a0,-0x2` (`~0x1`), `li a0,-0x9` (`~0x8`), `li a0,-0x3` (`~0x2`). `-0x2` is
   `~0x1`, not `~0x2`; easy to misread as clearing `0x2` twice.

### For Lane 2 (ignore file owner)

The `80066308` ignore entry's rationale text says its "only non-libsnd input is
the sequencer event already decoded by `8006320c`/`8006352c`". Those two are the
**volume slides**, not event decoders - the stale label this lane's first task
corrected in `audio.md`. The ignore *verdict* is unaffected; only the sentence is
wrong. The event decoder it means is `FUN_80063CEC`.
