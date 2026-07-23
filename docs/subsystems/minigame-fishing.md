# Fishing minigame

The fishing minigame is one mode of the shared minigame-hub overlay (the same binary that hosts the slot machine, Baka Fighter, and the dance game). The fishing-specific code occupies the lower address band of that overlay (roughly `0x801cf000`..`0x801d8000`); the higher band re-uses the shared field / actor / move VMs documented elsewhere ([`move-vm.md`](move-vm.md), [`actor-vm.md`](actor-vm.md), [`script-vm.md`](script-vm.md)) and is not redescribed here. Each frame the minigame ticks a small numeric-keyed state machine that walks the player through rod selection, casting, waiting, reeling against a per-fish AI, and the catch / score payout; the persistent fishing-point score lives in the save block and survives between sessions.

The per-frame driver is `FUN_801cf3bc` (`overlay_fishing_801cf3bc.txt`). It is dispatched indirectly as the active mode handler (no static caller inside the overlay dump), in the same "mode handler reached by an indirect dispatch table" pattern as the other minigame and field modes.

**BGM.** Fishing loads **no BGM track of its own** - the overlay has no streaming-loader call (`8001fc00`). The `func_0x80026478(&DAT_8007056c)` calls in the fishing state machine are the **actor sound-source attach / re-pan** primitive (`FUN_80026478` in [`functions.md`](../reference/functions.md)) - the positional reel / water / cast **SFX** voice, not a BGM stream. So the music is whatever the **field / town scene the fishing spot lives in** was already playing (its op-`0x35` BGM), inherited unchanged - a spot in one town sounds different from a spot in another. This is the same host-scene-inherited shape as the [slot machine](minigame-slot-machine.md); there is no single "fishing theme" to pin.

## State machine

`FUN_801cf3bc` switches on the mode-state word `DAT_801d926c` through a jump table, then runs a shared tail (`LAB_801d01a4`) that drives auxiliary animation timers, the HUD, and the global "press confirm to leave" check. The state values are sparse (the designers left gaps), and many states `+1` to advance to the next. Confirmed states:

| State | Role |
|---|---|
| `0` | Rod / type select: queues a small menu, reads a select edge, and on confirm grants the inventory rod + lure items (`func_0x800421d4` ids `0x9d`..`0xa2` - the SCUS item table names them Light/Normal/Heavy Lure + Old/Deluxe/Legendary Rod) and advances to `1`. |
| `1` | Scene / actor setup: spawns the fishing actors (`func_0x80020de0`), picks the location variant from `DAT_801d90d0`, initialises camera-tint bytes, then falls through to `0x32`. |
| `0x32` | Sets state to `10` (the run-loop entry). |
| `10` (`0xa`) | Run-loop init: zeroes the per-cast working set, including tension `DAT_801d9168`, depth/line `DAT_801d9298`, casting-power `DAT_801d9274` (seeded `0x40`) and its direction `DAT_801d9278`, then advances. |
| `0xb` | Fade-in: ramps the screen-fade level `DAT_801d905c` down to 0, then advances (or jumps to the "no rod" state `0x96` if `FUN_801d712c` reports no rod owned). |
| `0xc` | Idle / "press to cast": a button edge starts the cast (sets a sound cue and advances) or opens the shop branch (`0x64`). |
| `0xd` | Cast wind-up: advances a small counter, pans the camera, and after ~12 frames jumps to the casting-power state `0x14`. |
| `0x14` | Casting-power oscillator: bounces `DAT_801d9274` between `0x20` and `0x1000` (direction `DAT_801d9278`); on a button edge it locks the power, spawns the lure / line actors, and computes the line-projection vector from the locked power. |
| `0x19` | Transient hold state (sets the "allow leave" flag only). |
| `0x1e` | Lure-travel settle: waits for the line animation counter `DAT_801d91ac` to reach `0x14`, then jumps to `0x20`. |
| `0x20`..`0x22` | Lure-landing / line-sink sequence (camera + line-actor position setup), each advancing to the next. |
| `0x28` | Auxiliary animation wait keyed on `DAT_801d9164`; returns to `10` when the helper `FUN_801d7528` completes. |
| `0x2d` | Miss / retry bookkeeping (`DAT_801d9268` countdown via `FUN_801d6f10`), then back to `0x32`. |
| `0x64`..`0x66` | Shop / point-exchange branch: confirm prompts (`FUN_801d72a0`, but see [VA aliasing](#va-aliasing-in-this-band)) gating the buy / sell helpers `FUN_801d06c8` / `FUN_801d092c` / `FUN_801d0c3c`. |
| `0x6e`,`0x78`..`0x7a` | Shop sub-flows that call the same buy / sell helpers and the picker `FUN_801d0474` (static extract: the five-row main-menu picker - snap-wrap cursor, confirm jump table to states `0x0A`/`0x65`/`0x6E`/`0x78`/`0xC8`, rows 2/3 snapshot the points bank, row 4 arms the venue exit; port `engine-core::fishing::FishingMenu`). |
| `0x96` | "You have lost the lure" / no-rod end screen; a button edge advances to `200`. |
| `200` (`0xc8`) | Exit / fade-out: ramps a fade value to white and, once full, plays the leaving XA cue and tears the mode down. |

The shared tail also services three auxiliary one-shot animation timers (`DAT_801d9160` / `DAT_801d915c` / `DAT_801d90f0`, each advanced through `FUN_801d78ec` / `FUN_801d75dc` / `FUN_801d71d4` - see [HUD and banner animations](#hud-and-banner-animations)), applies the screen fade, draws the persistent HUD (`FUN_801d13f0`) and - while a fish is hooked (`DAT_801d9058`) - the catch HUD (`FUN_801d1580`), and honours a global "confirm-to-leave" edge that returns to state `10`. Each timer is idle at `0`, seeded to `1` by its trigger event, passed to its animator as the frame count, advanced by the frame step `DAT_1f800393` while the animator reports active, and zeroed when it expires; while the `FUN_801d75dc` timer runs, the tail forces the `FUN_801d78ec` timer back to zero.

The reeling / fish-AI tick `FUN_801d4004` and the per-fish actor handler `FUN_801d26cc` run from the actor side of the run loop (`FUN_801d26cc` calls `FUN_801d4004` while the fish is engaged), not directly from the mode switch.

## Tension / reeling mechanic

The hooked-fight is a tug-of-war between the player's reel input and the fish's pull, mediated by the tension gauge `DAT_801d9168` (range `0`..`0x1000`). The whole update lives in `FUN_801d4004` (`overlay_fishing_801d4004.txt`); the gauge math at its tail is:

- **Reel held** (`_DAT_8007b850 & 0x40` = **Cross** = reel A, or `& 0x80` = **Square** = reel B): tension *increases* by a per-frame step derived from a base pull, divided by a rod-dependent divisor (`_DAT_80084454 * 9 + 0x23` for reel A / Cross, `* 6 + 0x19` for reel B / Square) and scaled by the frame-step `DAT_1f800393`. Holding reel also nudges the line / depth value `DAT_801d9298` down by a small per-state amount.
- **Reel released** (`(_DAT_8007b850 & 0xc0) == 0`, neither Cross nor Square held): tension *decreases* by `(_DAT_80084454 * 0x40 + 0x4a) * DAT_1f800393`.
- **Mirror mode** (`_DAT_8007b850 & 0x2` = **R2**): the reel-direction mirror toggle read alongside the two reel buttons.
- The gauge is then clamped to `[0, 0x1000]`. (Confirmed: clamp at `0x1000` high, `0` low.)

The reel buttons are pinned from the pad-mask packer `FUN_8001822C` (both its digital and analog paths cross-confirm the map), which builds `_DAT_8007b850` from pad-1's low 16 bits: `0x10` Triangle, `0x20` Circle, `0x40` Cross, `0x80` Square, `0x1000` Up, `0x2000` Right, `0x4000` Down, `0x8000` Left, `0x0001` L2, `0x0002` R2, `0x0004` L1, `0x0008` R1, `0x0100` Select, `0x0200` L3, `0x0400` R3, `0x0800` Start. So reel A = Cross and reel B = **Square** (not Circle); Circle (`0x20`) is instead the cast / hook-state input.

`_DAT_80084454` is a persistent rod / upgrade stat read from the save block; a higher value softens the per-frame tension change. The fish's own behaviour is a sub-state machine on `DAT_801d910c` (run / dart-left / dart-right / dive, selected pseudo-randomly via the BIOS `rand` `func_0x80056798`), which moves the fish actor and modulates the pull; the timer `DAT_801d9110` counts down each behaviour and re-rolls the next. Per-fish parameters (pull magnitudes, dart push, behaviour-selection cutoffs, scoring value) come from the per-species table documented in [Per-species parameter table](#per-species-parameter-table) below, indexed by `DAT_801d91cc * 0x28` based at `&DAT_801d81a4`.

The catch HUD `FUN_801d1580` (`overlay_fishing_801d1580.txt`) renders the live state: the line length / record number `DAT_801d927c`, the casting-power bar `DAT_801d9274`, the depth `DAT_801d9298`, and - gated on `DAT_801d91b4` - the tension bar `DAT_801d9168` itself (drawn via `FUN_801d1870`). It uses the digit / glyph blitters `FUN_801d76e0` (number) and `FUN_801d63b0` (single sprite-quad).

The catch-HUD readout arithmetic: the length total is `max(record - 300, 0) * 100 >> 9` plus the `DAT_801d9178 >> 9` extent term (each clamped at zero), split as `/10` (whole) and `%10` (tenths digit) - the same `300` base as the hook check; the cast-power percent is `power * 100 >> 12` (percent of the `0x1000` meter ceiling); `FUN_801d1a90` draws the power bar. A debug length print sits behind the global print flag `_DAT_8007b9b0`.

A landed catch is resolved in `FUN_801d5298` (`overlay_fishing_801d5298.txt`). The awarded points are
`points = (fish_base_value * (DAT_801d91b8 + 0x9c0)) / 0x32000`,
where `fish_base_value` is the species record's `+0x04` field (`&DAT_801d81a8 + DAT_801d91cc*0x28`) and `DAT_801d91b8` is the accumulated pull / strength for the fight. The points are added to the persistent counter `_DAT_8008444c` (clamped to `999999`), guarded by a per-catch latch at actor `+0x2a` so a single fish is scored once. If the catch beats the current best (`_DAT_80084458`), the best value and its fish id (`_DAT_8008445c`) are updated.

## Reel-button decode and cadence

The held pad-mask `_DAT_8007b850` reaches the reel logic through a tiny decoder, `FUN_801d7450` (`overlay_fishing_801d7450.txt`): **Cross (`0x40`) takes priority and returns reel A (`1`); Square (`0x80`) without Cross returns reel B (`2`); neither returns idle (`0`)**. The whole body is the three-way branch `if (m & 0x40) return 1; else return (m >> 6) & 2;`, which is why holding both reel buttons resolves to reel A rather than a blend. This is the same reel-A = Cross / reel-B = Square mapping the [tension mechanic](#tension--reeling-mechanic) integrates.

Above the raw reel state sits a **cadence recogniser**, `FUN_801d3db4`
(`overlay_fishing_801d3db4.txt`). Each frame it decodes the current reel button
and compares it to the previous decode `DAT_801d9064`: while unchanged it
accumulates the frame-step `DAT_1f800393` into the current slot of a 16-entry
`{button, held-frames}` ring buffer `DAT_801d91e4` (write index `DAT_801d91dc`,
wrapped mod 16); on a change it advances the index and opens a fresh slot. It
then walks a window of that history backwards against the rodata gesture
templates at `DAT_801d87d4` - each template a sequence of `{button, duration}`
pairs matched with a **±10-frame tolerance** - and on a full match resets the
buffer through `FUN_801d746c` and reports the matched gesture. `FUN_801d746c`
(`overlay_fishing_801d746c.txt`) is that reset: it zeroes `DAT_801d91dc` and
clears all sixteen 8-byte entries of `DAT_801d91e4`.

(VA aliasing: `undoc` attributes `801d3db4`'s VA to `overlay_0971`; the *fishing* occupant of that VA is the recogniser here, pinned by its reads of the fishing globals `DAT_801d9064` / `DAT_801d91dc` / `DAT_801d91e4` and its calls to `FUN_801d7450` / `FUN_801d746c`. See [VA aliasing](#va-aliasing-in-this-band).)

## Fishing actors and scene render

The run loop drives a small pool of per-frame actor handlers reached through the actor table (each takes the actor-struct pointer), plus the select screen and the scene render pass:

- `FUN_801d0f5c` (`overlay_fishing_801d0f5c.txt`) - the **rod / lure select screen**: input *and* render. It counts owned rods (item ids `0xa0`..`0xa2`), moves the select cursor `DAT_801d90dc` on the D-pad edge (`0x1000` / `0x4000`), wraps it against `owned+2`, and on the accept edge (`0x44`) equips the highlighted entry - a lure (`DAT_801d90dc < 3`, item `0x9d`+cursor) writes the persistent lure index `_DAT_80084450`, a rod (cursor `>= 3`, item `0xa0`+) writes the persistent rod stat `_DAT_80084454` (the value that scales the [tension change](#tension--reeling-mechanic)). Cancel / confirm (`0x21`) sets the leave SFX and jumps `DAT_801d926c` to `100`. The tail renders each rod / lure row with its owned count and a highlight (`_DAT_8007b454 = 7`).
- `FUN_801d1c5c` (`overlay_fishing_801d1c5c.txt`) - the **cast-line / bobber actor**: a small animation SM on `DAT_801d91ac` (state `1` sinks the bob `DAT_801d9134` at `-0x40 * step` down to `-700`, state `10` raises it at `+0x40 * step` up to `0x400` then latches `0x14`), then a GTE transform + draw of the lure at the computed position / spin (`DAT_801d9140`).
- `FUN_801d2050` (`overlay_fishing_801d2050.txt`) - the **pre-hook swimming-fish tick + fish-sprite spawn**: an init-once latch installs the per-frame callback `FUN_801d7c30` into `_DAT_8007ba2c` and records the actor pointer in `DAT_801d928c`; when `DAT_801d9294` steps it spawns the fish sprite keyed on species `DAT_801d91cc` (special-casing id `8`). It delegates motion to `FUN_801d2278` and `FUN_801d6028`.
- `FUN_801d2278` (`overlay_fishing_801d2278.txt`) - the **free-swim wander**: decrements the re-target timer `DAT_801d9060`, and on expiry re-rolls a random destination / duration (BIOS `rand`) and spawns a ripple effect; in the idle / cast state (`DAT_801d926c == 0xc`) it also reads the D-pad (`0x8000` / `0x2000`) to nudge the fish-facing angle clamped `0x700`..`0x900`.
- `FUN_801d70ec` (`overlay_fishing_801d70ec.txt`) - a **minimal swimming-fish idle tick** (the reduced sibling of `FUN_801d2050`'s non-spawn path: refresh `+0x16` from `FUN_801d6028`, clear the draw-skip bit, submit).
- `FUN_801d4948` (`overlay_fishing_801d4948.txt`) - the **reeling-line / hooked-lure actor**: a sub-state machine on `DAT_801d91c8` (`0`→arm, `1`→attach to the hooked-fish actor `+0x48`, `2`→track) that positions the line end from `DAT_801d9174` / `DAT_801d9178` / `DAT_801d917c`, applies an orbit offset via `FUN_801d7bb8`, and raises the hook SFX cue `_DAT_8007b6da = 0x3a`.
- `FUN_801d67bc` (`overlay_fishing_801d67bc.txt`) - the **caught-fish 3D mesh render**: GTE rotate (`+0x24` / `+0x26` / `+0x28` Euler angles), matrix push, per-fish tint from the actor `+0x72` colour word, and a subdivided-primitive draw. Render-track - documented, not ported.
- `FUN_801d24ec` (`overlay_fishing_801d24ec.txt`) - the **water reflection / shadow strip**: a run of textured quads written straight into the GTE packet list `_DAT_1f8003a0`. Render-track - documented, not ported.
- `FUN_801d6bbc` (`overlay_fishing_801d6bbc.txt`) - the **fishing scene render pass** invoked from the driver tail (`FUN_801cf3bc`): it walks the live actor list (transform + submit + free) and emits the shared tile-board grid at `_DAT_1f8003ec` (the `+0x4000` / `+0x8000` cell + walkability arrays - see [`tile-board.md`](tile-board.md)). Render-track - documented, not ported.
- `FUN_801d78c0` (`overlay_fishing_801d78c0.txt`) - the **fishing camera-scroll reset**: `_DAT_800840b8 = 0`, `_DAT_800840c0 = 0x974`, and the scroll trio `_DAT_8007b790` / `_DAT_8007b792` / `_DAT_8007b794 = 0`.
- `FUN_801d79e0` (`overlay_fishing_801d79e0.txt`) - a **cell-record lookup** used by the fish-placement path (reached through `FUN_801d6028`): a linear scan of a per-cell record table matching a `(x, y)` cell against each record's first two bytes, returning the record pointer or null.

## HUD and banner animations

The persistent HUD `FUN_801d13f0` (`overlay_fishing_801d13f0.txt`) is drawn every frame by the driver tail: the best-catch row (`_DAT_80084458`, glyph `0x1a`), the point-total row (`_DAT_8008444c` rendered capped at `999999`, glyph `0x1c`), the selected rod/lure label (three overlay strings picked by `_DAT_80084450` = 0/1/2; any other index draws no label), and a lures-remaining line: a caption plus the live inventory count of item `_DAT_80084450 + 0x9d` (`func_0x80042f4c`) as a 4-digit number, plus a trailing caption. All rows draw at brightness `0x80`.

The three one-shot animators the tail's timers drive (each takes the timer value as its frame count and returns whether it is still active; all expire by returning 0, which zeroes the timer):

- `FUN_801d78ec` (`overlay_fishing_801d78ec.txt`, timer `DAT_801d9160`) - a banner (glyph `7`, `y = 0x78`) sliding in from the **left** at 8 px/frame, holding at `x = 0xa0` from frame `0x14`, sliding off from frame `0x8c` (`x = frame*8 - 0x3c0`, joining the hold continuously), active while `frame < 0xc8`. Seeded at the moment the fish hooks (`FUN_801d26cc`, alongside `DAT_801d91b4 = 1`).
- `FUN_801d75dc` (`overlay_fishing_801d75dc.txt`, timer `DAT_801d915c`) - the mirrored banner (glyph `0xd`) sliding in from the **right**: same ramp, `x = 0x140 -` ramp, holding at the same `x = 0xa0`; same `0xc8` lifetime. Seeded on the hooked fight's reel-in-complete path (`FUN_801d26cc`: `DAT_801d927c` below `0x136` while `DAT_801d91b4` set); while it runs the tail cancels the `FUN_801d78ec` timer.
- `FUN_801d71d4` (`overlay_fishing_801d71d4.txt`, timer `DAT_801d90f0`) - the strike splash: a two-glyph pair (`0x416` / `0x816`) at `x = 0xa0` rising one pixel every 32 frames from `y = 0x50`, brightness ramping `frame*8` up to a `0x80` hold (frame `0x10`), holding until frame `0x88`, then fading `0x80 - (frame-0x88)*8` to expire at frame `0x98`. Seeded at the strike / hit event before the fish hooks (`FUN_801d26cc`, gated on `DAT_801d91b4 == 0`).

Two further animators ride the same ramp as the pair above, and expire on the
same `0xc8` lifetime:

- `FUN_801d6f10` (timer `DAT_801d9268`) - the miss / retry banner: a single glyph `0x19` on the mirrored trajectory (`x = 0x140 -` ramp). This is what parks state `0x2d` before it returns to `0x32`.
- `FUN_801d7528` (timer `DAT_801d9164`) - the auxiliary banner, which emits the *same* glyph `0xc` twice, at the ramp and at its mirror, so the pair converges on the `0xa0` hold from both screen edges and parts again on the way out. State `0x28` waits on it.

## The bar and digit primitives

`FUN_801d1870` and `FUN_801d1a90` are the same gauge bar on the two axes. Each
emits a three-glyph frame - a start cap, a body stretched by `segments << 12`
along the bar's axis, and an end cap - at a fixed brightness `0x80`, then
overlays the fill quad itself. The fill is `segments * value * 8 / 0x1000`
pixels long and its brightness ramps `value * 0xff / 0x1000`, so the bar
brightens as it fills. `FUN_801d1870` runs horizontally with glyphs `3`/`4`/`5`
and fills left-to-right; `FUN_801d1a90` runs vertically with glyphs `0`/`1`/`2`
and fills *upward* from the bottom cap.

`FUN_801d1870`'s first argument selects the fill quad's colour ramp only and
moves no geometry. It branches three ways over the four vertex-colour triples,
where `g` is the brightness byte `value * 0xff >> 12`:

| `param_1` | Fill RGB | Used by |
|---|---|---|
| `0` | `(0xbc, g, 0)` - constant red against the ramp | depth gauge |
| `1` | `(g, ~g, 0)` - the ramp against its own complement | tension gauge |
| other | colour stores jumped entirely; the buffer keeps its previous contents | no call site |

`FUN_801d1a90` takes **no** style argument - it is a four-argument function
that stores `0xbc` into red unconditionally, so the vertical bar is
permanently the `0` ramp.

`FUN_801d76e0` lays a number out in a fixed **eight-slot** field: slot `i` holds
`value / 10^(7-i)` and is emitted only once that quotient is non-zero, so
leading zeros are blank slots and the number ends up right-aligned. Retail
seeds the last slot with `0` before the fill loop, which is what makes a value
of zero draw a single `0` rather than nothing. Its first argument picks the slot
pitch: `0` = 8 px, anything else = 16 px.

## Additional HUD draw helpers

Three more fishing-overlay draw helpers sit in the same band. Each is pinned to
PROT entry 0972 by content (the arbiter byte-matches all five minigame-overlay
captures at each VA back to `fishing(972)`; see [VA aliasing](#va-aliasing-in-this-band)),
and each is reached from a fishing-overlay caller, not a sibling minigame.

- `FUN_801d74b0` `(cx, y, w, val)` - centered bar-widget draw. Skips entirely when `y > 0xF0`; otherwise stages widget kind `0x44` via `FUN_80034b6c` and emits the bar through the bar-widget dispatcher `FUN_8002c69c` at `(cx - w/2 - 2, y + 6)` with width `w` and fill `val`. Called by the state machine `FUN_801cf3bc` and the shop/help helpers. `see ghidra/scripts/funcs/overlay_fishing_801d74b0.txt`.
- `FUN_801d7964` `(x, rgb0, rgb1, y, arg4, arg5)` - colored screen-fade spawn wrapper. Unpacks the two packed 24-bit colours (`rgb0`/`rgb1`, three bytes each) and the coordinate params into an on-stack fade template, then spawns the fade actor via `FUN_80024e80(template, 1)` (the screen-fade primitive spawn). `see ghidra/scripts/funcs/overlay_fishing_801d7964.txt`.
- `FUN_801d7c84` `(row)` - species-name list drawer. Reads up to four species ids from the venue spawn table `PTR_DAT_801d9114` at index `row*8 + i` (`i = 0..3`), and for each non-`-1` id draws the name pointer at `&DAT_801d81a4 + id*0x28` (the per-species table) via the glyph renderer `FUN_80036888` at `x = 0`, stacking rows 16 px apart from `y = 0x10` at palette `0xa0`. `see ghidra/scripts/funcs/overlay_fishing_801d7c84.txt`.

## VA aliasing in this band

The dumps covering `0x801d1xxx` and `0x801d6f00`..`0x801d78ff` are runtime
captures whose overlay labels are unreliable - a save-state slice can retain
bytes from a previously-resident overlay, so a file labelled for one minigame
can hold another's code at some VAs. Attribute by *content*, not by the dump's
filename: the functions above are pinned by their own reads (the lure item ids
`0x9d`..`0x9f`, the `DAT_801d9xxx` globals, the shared emitter `FUN_801d63b0`).

`FUN_801d72a0` is settled by the clean static extract (PROT entry 0972 file
offset `0x8A88` at base `0x801CE818`): the fishing overlay's own occupant of
that VA is a **two-page help-panel renderer** `(x, y, page)`. Page 0 draws 14
text lines from the string-pointer table at `0x801D8130`, page 1 draws 15
lines from the sibling table at `0x801D8168` (which starts exactly 14 words
later, bounding table 0); both use a 13 px line pitch through the glyph
renderer `FUN_80036888`, draw a per-page footer string (`0x801CF048` /
`0x801CF050`) at `(0xE0, 0xCA)`, emit the widget frame
`FUN_8002C69C(x, y, 0x119, 0xC3)`, and store the field-subsystem mode byte
`DAT_80073F20 = 0x10` on entry. So the state table's "confirm prompt" naming
described the call site, not this body - the confirm gating lives in the
buy / sell helpers. Layout port: `engine-core::fishing::help_panel_layout`.

## Per-species parameter table

The species table is **static `.rodata`** in the fishing overlay (PROT entry 0972, `data\OTHER1`; base `0x801CE818`, table head `0x801D81A4` = file offset `0x998C`). Record `N` lives at `0x801D81A4 + N*0x28`; the decompiler resolves the head as `(&PTR_s_Spikefish_801d81a4)[DAT_801d91cc * 10]`, so `+0x00` is a pointer to the fish-name string (also in this overlay). The structure runs for **10 records** (`Spikefish` = id 0 .. the rarest catch = id 9); record 10's `+0x00` is no longer an in-overlay pointer, which bounds the table.

Each record is 10 words (stride `0x28`). Every field has a *confirmed reader* in `FUN_801d4004` (fish-AI tick) or `FUN_801d5298` (scoring); the designer-level meaning is the consuming formula:

| Off | Field | Consuming site / formula |
|---|---|---|
| `+0x00` | name pointer | `FUN_801d4004` - the hooked-fish name banner |
| `+0x04` | score base value | `FUN_801d5298` - `points = value * (strength + 0x9c0) / 0x32000` |
| `+0x08` | pull factor | `FUN_801d4004` - per-frame pull `((rand & 0xff) + bias) * f / 150` (also a `/0xc8000` term) |
| `+0x0c` | dart push factor | `FUN_801d4004` - dart-state lateral push `((step >> 2) + 0x20) * f / 100` |
| `+0x10` | depth-sink factor | `FUN_801d4004` - run-state line-sink `(pull * f) / 150` |
| `+0x14` | depth gate | `FUN_801d4004` - behaviour pick when `f < line-depth` |
| `+0x18` | behaviour-roll cutoff A | `FUN_801d4004` - `f <= rand & 0xfff` |
| `+0x1c` | behaviour-roll cutoff B | `FUN_801d4004` - `rand & 0xfff < f` |
| `+0x20` | behaviour-roll cutoff C | `FUN_801d4004` - `rand & 0xfff < f` |
| `+0x24` | strike / record gate | `FUN_801d4004` - hook check `record < f + 300` |

The `+0x04` score value and `+0x08` pull factor both climb monotonically with rarity (the rarest catch carries the largest of each), so a higher-value fish is also the harder fight. Parser: [`legaia_asset::fishing_species`] (`parse` decodes the 10 records from the overlay image; `FishingSpecies::score_for` reproduces the award formula; `name` resolves the `+0x00` pointer). No Sony bytes are committed - the values + names decode from the user's disc (disc-gated `fishing_species_real`).

## RAM state

Fishing-specific globals (overlay-resident unless noted; `_DAT_8008xxxx` live in the persistent save block):

| Address | Type | Meaning |
|---|---|---|
| `0x801d926c` | `u32` | Mode-state word for `FUN_801cf3bc` (the values in the table above). |
| `0x801d9168` | `s32` | **Tension gauge**, `0`..`0x1000`. Raised by held reel input, lowered when released. |
| `0x801d9274` | `s32` | Casting-power meter; oscillates `0x20`..`0x1000` in state `0x14` and is locked on cast. |
| `0x801d9278` | `s32` | Casting-power oscillation direction (`+1` / `-1`). |
| `0x801d9298` | `s32` | Line depth / sink value during the fight (clamped against the cast power). |
| `0x801d91cc` | `u32` | Hooked-fish species id; indexes the per-species table at `&DAT_801d81a8` (stride `0x28`). |
| `0x801d910c` | `u32` | Fish behaviour sub-state (run / dart / dive). |
| `0x801d9110` | `s32` | Frame countdown until the next fish-behaviour re-roll. |
| `0x801d91b8` | `s32` | Accumulated pull / strength for the current fight; feeds the score formula. |
| `0x801d927c` | `s32` | Line length / catch record value shown on the HUD. |
| `0x801d9280` | `s32` | HUD length term `max(record - 300, 0)`, written back by `FUN_801d1580` each frame. |
| `0x801d9178` | `s32` | Second length-readout term (`>>9` scale); drawn alone as the lower readout and added into the length total. |
| `0x801d91b4` | `u32` | Set at the hook; gates the catch HUD's depth + tension gauge block and the strike-splash seed. |
| `0x801d9160` | `u32` | One-shot timer for the from-left banner `FUN_801d78ec`; seeded to 1 at the hook. |
| `0x801d915c` | `u32` | One-shot timer for the from-right banner `FUN_801d75dc`; seeded on the reel-in-complete path (cancels `0x801d9160` while running). |
| `0x801d90f0` | `u32` | One-shot timer for the strike splash `FUN_801d71d4`; seeded at the strike event. |
| `0x801d9058` | `u32` | "Fish hooked" flag; gates the catch HUD (`FUN_801d1580`). |
| `0x801d905c` | `s32` | Screen-fade level (down-ramped on fade-in, up-ramped on exit). |
| `0x801d90d0` | `u32` | Fishing-location variant selected at setup. |
| `0x801d90dc` | `u32` | Rod / lure select cursor for `FUN_801d0f5c` (`0`..`2` = lures, `3`+ = rods). |
| `0x801d9064` | `s32` | Last decoded reel button (`0` idle / `1` reel A / `2` reel B), for the `FUN_801d3db4` cadence recogniser. |
| `0x801d91dc` | `u32` | Reel-cadence ring-buffer write index (mod 16); reset by `FUN_801d746c`. |
| `0x801d91e4` | `u64[16]` | Reel-cadence ring buffer - sixteen `{button, held-frames}` entries walked against the templates at `DAT_801d87d4`. |
| `0x801d9060` | `s32` | Free-swim fish re-target timer (`FUN_801d2278`). |
| `0x801d9294` | `u32` | Fish-sprite spawn step latch (`FUN_801d2050`). |
| `0x801d928c` | `u32` | Hooked-fish actor pointer, saved by `FUN_801d2050`, read by `FUN_801d4948`. |
| `0x801d91c8` | `u32` | Reeling-line actor sub-state (`FUN_801d4948`, `0`/`1`/`2`). |
| `0x8008444c` | `s32` | **Persistent fishing-point score** (save block), capped at `999999`. |
| `0x80084450` | `u32` | Persistent selected-rod index (HUD label + SFX base). |
| `0x80084454` | `s32` | Persistent rod / upgrade stat; scales the per-frame tension change. |
| `0x80084458` | `s32` | Persistent best-catch point value. |
| `0x8008445c` | `u32` | Persistent best-catch fish id. |

(Pad-input globals `_DAT_8007b850` held-mask and `_DAT_8007b874` edge-mask, and the frame-step `DAT_1f800393`, are the shared field-VM globals; see [`field-locomotion.md`](field-locomotion.md) / [`script-vm.md`](script-vm.md).)

## Key functions

- `FUN_801cf3bc` (`overlay_fishing_801cf3bc.txt`) - per-frame fishing mode driver; the `DAT_801d926c` state machine plus the HUD / fade / leave tail.
- `FUN_801d4004` (`overlay_fishing_801d4004.txt`) - fish-AI + tension-gauge tick: reel-input integration into `DAT_801d9168`, the `DAT_801d910c` behaviour sub-state, and the next-behaviour roll.
- `FUN_801d26cc` (`overlay_fishing_801d26cc.txt`) - hooked-fish actor handler; positions the fish / lure / line actors and calls `FUN_801d4004` while engaged.
- `FUN_801d5298` (`overlay_fishing_801d5298.txt`) - catch resolution + scoring: computes the point award, credits `_DAT_8008444c`, and updates the best-catch record.
- `FUN_801d1580` (`overlay_fishing_801d1580.txt`) - catch HUD: draws tension, casting power, depth, and record values.
- `FUN_801d13f0` (`overlay_fishing_801d13f0.txt`) - persistent HUD: draws the best-catch value, the fishing-point total (`_DAT_8008444c`, capped), the rod-type label, and the lures-remaining count (item `_DAT_80084450 + 0x9d`).
- `FUN_801d78ec` / `FUN_801d75dc` / `FUN_801d71d4` (`overlay_fishing_801d78ec.txt` / `..75dc.txt` / `..71d4.txt`) - the three one-shot banner/splash animators (see [HUD and banner animations](#hud-and-banner-animations)).
- `FUN_801d712c` (`overlay_fishing_801d712c.txt`) - rod-ownership gate; queries inventory item ids `0x9d`..`0x9f` (`func_0x80042f4c`) and re-points the persistent rod index `_DAT_80084450` onto an owned one.
- `FUN_801d6f10` / `FUN_801d7528` - the miss-retry and auxiliary banner animators (see [HUD and banner animations](#hud-and-banner-animations)).
- `FUN_801d1870` / `FUN_801d1a90` / `FUN_801d76e0` - the horizontal bar, vertical bar and digit-field primitives (see [The bar and digit primitives](#the-bar-and-digit-primitives)).
- `FUN_801d7450` / `FUN_801d3db4` / `FUN_801d746c` - the reel-button decoder, the reel-cadence recogniser, and its buffer reset (see [Reel-button decode and cadence](#reel-button-decode-and-cadence)).
- `FUN_801d0f5c` / `FUN_801d1c5c` / `FUN_801d2050` / `FUN_801d2278` / `FUN_801d70ec` / `FUN_801d4948` / `FUN_801d67bc` / `FUN_801d24ec` / `FUN_801d6bbc` / `FUN_801d78c0` / `FUN_801d79e0` - the rod/lure select screen, the actor handlers, and the scene render pass (see [Fishing actors and scene render](#fishing-actors-and-scene-render)).

Parser: [`legaia_asset::fishing_species`](../../crates/asset/src/fishing_species.rs) decodes the [per-species table](#per-species-parameter-table) from the disc.

Engine port: [`legaia_engine_core::fishing`](../../crates/engine-core/src/fishing.rs) is the clean-room rules engine over that table. The **Confirmed** numeric kernels are ported directly: the casting-power oscillator (`CastPower`, bounds `0x20..=0x1000`, seed `0x40`; `FUN_801cf3bc` state `0x14`), the tension-gauge tug-of-war (`TensionGauge`, reel divisors `rod*9+0x23` / `rod*6+0x19`, release `(rod*0x40+0x4a)*frame_step`, clamp `[0, 0x1000]`; `FUN_801d4004`), and the catch award + persistent-record credit (`FishingRecord`,
`value*(strength+0x9c0)/0x32000`, `999999` cap, best-catch; `FUN_801d5298`),
and the reel-button decoder (`ReelInput::from_pad_mask`, `0x40 -> ReelA` /
`0x80 -> ReelB` / else `Idle`; `FUN_801d7450`). The reel-cadence recogniser
(`FUN_801d3db4`) stays documented-not-ported: its match is pure integer logic
but is driven by the Sony gesture-template rodata `DAT_801d87d4`.

The HUD / banner cluster is ported as a draw-list layer (`HudDraw`) in [`legaia_engine_ui::ui_fishing`](../../crates/engine-ui/src/ui_fishing.rs), beside the consumer that renders it: `persistent_hud_draws` (`FUN_801d13f0`), `catch_hud_draws` plus the `length_display` / `extent_display` / `cast_power_percent` kernels (`FUN_801d1580`), the five animators `banner_from_left_draw` / `banner_from_right_draw` / `strike_splash_draws` / `banner_miss_draw` / `banner_converge_draws` (`FUN_801d78ec` / `FUN_801d75dc` / `FUN_801d71d4` / `FUN_801d6f10` / `FUN_801d7528`), and `BannerTimer` (the tail's timer-service loop).

The bar and digit primitives are ported as layout builders over that same draw list: `bar_frame` / `power_bar_frame` (`FUN_801d1870` / `FUN_801d1a90`) return the cap/body/cap glyph frame plus the fill extent and its brightness, and `number_digit_cells` (`FUN_801d76e0`) expands a value into its eight-slot digit field. `select_owned_rod` (`FUN_801d712c`) stays with the rules half in `legaia_engine_core::fishing`; note it is not read-only - it advances the persistent rod index onto an owned lure, which is why the HUD's rod label can change without the player touching the menu.

`fishing_hud_draws_for` is the consumer for that draw list - the fishing sibling of `battle_hud_draws_for`. It renders `Number` and `Count` items through the ported digit field as font-atlas text, resolves `Caption` items against host-supplied strings (the retail captions are overlay rodata), resolves `Glyph` ids and gauge fills through a host-supplied atlas lookup, and routes `Bar` / `PowerBar` through `bar_frame` / `power_bar_frame` into cap/body/cap quads plus a fill quad on the frame's own axis. An id the host cannot place is dropped rather than guessed at.

What the consumer does **not** supply is the fishing sprite page itself. `FUN_801d63b0` is a bare VRAM quad emitter whose glyph ids index a page no host uploads yet, so a host that passes no atlas gets the number / caption rows and none of the icon or gauge geometry. The play window is in exactly that state: it renders the persistent HUD's rows at their traced stage pens and keeps a text line for the live tension / cast readouts the gauges would otherwise carry.

The `FishingSession` composes those kernels into a cast → fight → score loop. The win/lose glue (line-snaps-at-max-tension, reel-progress land, the locked-cast species pick, and the steady per-frame fish pull) is an **engine-side reconstruction** of the [Open](#open) items below and is marked as such at each call site - no Sony bytes are baked in.

**Retail entry.** A fishing-pond door hands off through the ordinary **game mode 24** (`OTHER INIT`, `sub_id = 0`) path - the same scene-backup → overlay-load → return-to-field sequence any mode-24 minigame takes, with PROT 0972 as the loaded overlay. There is no bespoke fishing entry: the pond door is a normal door whose target mode is 24, which is why the minigame inherits the host scene's BGM (above) and returns to the exact field state it suspended. The engine's `GameMode::OtherInit`/`OtherMode` pair (`crates/engine-core/src/mode.rs`) is that mode.

Runtime wiring: installed as a suspending scene mode (`SceneMode::Fishing`; `World::enter_fishing` / `tick_fishing` / `exit_fishing`). The `play-window` viewer starts it from the `L` key (loads the fishing overlay PROT 0972, `fishing_species::parse`); Cross locks the cast and reels (reel A), Square is reel B (retail: `0x80`), and the HUD shows the cast-power / tension / catch-result line plus the running point total. `P` opens the [point exchange](#point-exchange-prize-shop) (Up/Down move, Left/Right switch venue, Enter trades).

## Point exchange (prize shop)

The shop branch of the mode SM (states `0x64`..`0x7a`) is a **point exchange**: it spends the persistent fishing-point pool `_DAT_8008444C` on in-game items. The screens are:

- `FUN_801d0c3c` (state `0x64` family) - the 6-row prize list. Each row prints its item name through the MES `0xC2` item-name token fed with the record's item id, plus the per-unit price; the running point total renders capped at `999999`. **Row 0 is hidden until strictly affordable** (`price0 < points` - the cursor floor is `(price0 < points) ^ 1`), which is why each venue's top prize only "appears" once the pool is big enough. Row availability (white vs grey, `FUN_801d6f90`): affordable, inventory count `!= 99`, and - for a one-time row - its purchased bit not yet latched.
- `FUN_801d092c` (state `0x7a`) - the "Trade how many?" quantity picker. Max quantity = `min(points / price, limit − owned)` where `owned` is the live inventory count (`func_0x80042f4c`) and a not-yet-purchased one-time row treats `owned` as 0.
- `FUN_801d06c8` (state `0x79`) - the "Are you sure?" confirm. Yes grants `func_0x800421d4(item_id, qty)`, deducts `price * qty` from `_DAT_8008444C`, and for a `limit == 1` row latches bit `row + venue*8` of the persistent purchased bitmask `_DAT_8008446C`.

**Record layout** (12-byte stride, 6 rows per venue, read through `PTR_DAT_801d90b8`):

| Off | Field | Meaning |
|---|---|---|
| `+0x00` | `limit` | Max obtainable count: `1` = one-time prize (latched in `_DAT_8008446C`), `99` = repeatable |
| `+0x04` | `price` | Fishing points per unit |
| `+0x08` | `item_id` | Granted item id (SCUS item-name-table space) |

**Venue pages.** Two consecutive 6-row tables live in the overlay rodata at VA `0x801D8088` / `0x801D80D0`; `FUN_801cf3bc` state `1` selects the page from the venue global `_DAT_8007BAC4` (`0x187` → page 0, the Buma pond; `0xF4` → page 1, the Vidna pond - the selector values equal the Karisto / Sebucus kingdom-bundle extraction indices). Both venues spend and latch against the same globals; venue 1's one-time bits occupy `8..`. Cross-validated row-for-row against the curated walkthrough prize lists ([`gamedata.md`](../reference/gamedata.md)) - including one entry the walkthroughs miss: **Vidna's row 0 is a 50,000-point one-time War God Icon**, invisible until the pool exceeds its price.

The same state-1 page select also pages the venue's **species-spawn table** into `PTR_DAT_801d9114` (rodata `0x801D8334` / `0x801D8434`, directly after the species table): `8 × 8` u32 species ids read by the hooked-fish handler as `species = table[rod*8 + band]` (`FUN_801d26cc`), where `rod` is the equipped-rod index `_DAT_80084450` (rows 3..8 are zero padding - three rods exist) and `band` is the cast band `DAT_801d90e8` (0..4; band 4 is the venue's rare band, entered by a venue-specific roll - 1/16 at Buma with rod 1 + lure 2 after `0x32` even-count catches, 1/4 at Vidna with rod 2 + lure 2 - or directly on a deep cast).

Parsers: [`legaia_asset::fishing_exchange`](../../crates/asset/src/fishing_exchange.rs) (exchange pages) and `fishing_species::parse_spawn_tables` (spawn pages); disc-gated `fishing_exchange_real` pins the structural invariants. Engine port: `legaia_engine_core::fishing::PrizeExchange` (list-floor / availability / quantity-cap / confirm kernels) with the grant committed by `World::fishing_exchange_buy` against the persistent `World::fishing_points` pool + `World::fishing_prizes_purchased` mask (the retail `_DAT_8008444C` / `_DAT_8008446C` pair); disc-free runtime oracle `fishing_exchange_runtime`.

## Open

No open items. The reel-button bit assignment within `_DAT_8007b850` - the last remaining question - is now pinned from `FUN_8001822C`: reel A = `0x40` = Cross, reel B = `0x80` = Square, mirror = `0x2` = R2 (see [Tension / reeling mechanic](#tension--reeling-mechanic)).
