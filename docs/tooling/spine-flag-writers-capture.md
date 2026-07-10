# Capturing the chapter-1 spine story-flag writers

Three chapter-1 story-progression writes live in un-imported RAM overlays, so
static analysis can't name the code that performs them. This runbook drives one
interactive play-forward session that arms all three watches at once and logs
the writer PC (`ra`) of each:

| Target | What it gates | Watch method |
|---|---|---|
| `DAT_8007b7fc = 0x4B` | ~~Zeto battle-id~~ **FALSIFIED - see banner** | Write-watch `0x8007b7fc`, width 1 |
| system flag `0x142` | dolk-dungeon clear | Exec-bp `0x8003CE08`, `a0 == 322` |
| system flag `0x482` | Drake mist walls | Exec-bp `0x8003CE08`, `a0 == 1154` |

> **Correction - the `DAT_8007b7fc` Zeto leg is moot.** A live firehose from
> `chapter2_garmel_pre_zeto` (interpreter, this exact write-watch armed width-1
> **and** width-4) stayed **silent** across three reproducible Zeto fights.
> Zeto (`0x4B`, byte-confirmed name in PROT 867) fights in scene **`garmel`**,
> and its formation `[0x4B,0,0,0]` is installed by the `FUN_801DA51C`
> `actor[+0x94]` **record path** - not the `DAT_8007b7fc` battle-id path (which
> reads `0` everywhere and may be vestigial). The `jou` scoping below is
> superseded. The **flag** legs (`0x142` / `0x482` / `549`) are unaffected;
> keep the runbook for them. Full detail:
> [`formats/encounter.md`](../formats/encounter.md#scripted-battle-id-path-fun_8005567c).

The probe is
[`autorun_spine_flag_writers.lua`](../../scripts/pcsx-redux/autorun_spine_flag_writers.lua).
It installs a bare Vsync listener with **no self-quit** (this is a
human-at-the-controls session), arms the watches once the game settles into
field mode, and streams every hit to a CSV.

> **Localise first with the fast poll tier.** These exec-bp watches force the
> interpreter (~10 fps). Before a targeted session, a `--fast` dynarec run of
> [`autorun_state_poll.lua`](../../scripts/pcsx-redux/autorun_state_poll.lua)
> (the [two-tier model](pcsx-redux-automation.md#fast-whole-playthrough-capture-two-tier-model))
> logs *which scene* each flag changes in at full speed - point the exec-bp
> firehose there for the writer `ra`. Both probes share the USA-build version
> guard (`lib/probe/version.lua`).

## Derived watch addresses

Flag-bank geometry (SCUS-resident): base `0x80085758`,
`byte = base + (idx >> 3)`, `mask = 0x80 >> (idx & 7)`.

| Flag | idx (dec) | Bank byte | Mask | Fallback Write-watch |
|---|---|---|---|---|
| `0x142` (dolk clear) | 322 | `0x80085780` | `0x20` | byte `0x80085780` bit `0x20` |
| `0x482` (mist walls) | 1154 | `0x800857E8` | `0x20` | byte `0x800857E8` bit `0x20` |

The Exec-bp path is preferred: it isolates the exact flag AND names the writer
`ra` directly, which a raw byte watch can't (eight flags share one bank byte).
Set `LEGAIA_FLAG_FALLBACK=1` to additionally arm the raw byte watches if a flag
write is somehow missed by the setter breakpoint (then decode the `ra` by hand).
The Zeto byte watch is width 1; if it never fires, set `LEGAIA_ZETO_WIDTH=4` to
widen it to the full word.

## Which emulator

**PCSX-Redux**, not mednafen. These are live watchpoints, which need the
interpreter + debugger. `run_probe.sh` sets `-interpreter -debugger` by default;
**do not** pass `--fast` - Lua breakpoints do not fire under the recompiler.

## Which card slot for which beat

The chapter-1 ladder card `saves/library/cards/playthrough-ladder-pro00-14.mcr`
holds 15 active slots (PRO-00..14, blocks 1..15). Story-flag state was read
directly from each SC block's bitmap (block offset `0x14C0`, mirroring RAM
`0x80085600`). Load the slot nearest the beat you want, then play forward:

| Beat / write | Load save | Why | Fallback |
|---|---|---|---|
| Zeto write (`0x8007b7fc=0x4B`) | **PRO-01** | keikoku + dolk done, Zeto not beaten; tightest pre-trigger | PRO-05 |
| flag `0x142` (dolk clear) | **PRO-00** | everything unset; play through the dolk dungeon to its clear beat | - |

Load saves **by name** on the in-game load screen, never by block position: the
card's physical block order does not follow the PRO numbering (verify with
`save-tool saves <card>`; e.g. PRO-01 sits mid-card, after PRO-03/04/05).
| flag `0x482` (mist walls) | (not on this card) | `0x482` is unset in all 15 slots; no catalogued save reaches it | see note below |

PRO-00 is also a viable single start for a forward run past the Caruban beat
(`0x142`). (The old `jou`-side Zeto battle-id write it once targeted was
falsified live - see the correction banner.)

**Mist-wall caveat.** Flag `0x482` is unset across every slot of this card (it
matches the library-wide zero the backlog notes). It is **not bracketable** from
this card: the operator must keep the watch armed and play forward *past* the
Zeto beat to the Drake mist-wall story event, or source a different card that
sits nearer it. Treat this as the one leg that needs fresh forward play - the
other two are one short walk from their load slot.

## The firehose variant - capture every flag write, not just the spine

[`autorun_flag_firehose.lua`](../../scripts/pcsx-redux/autorun_flag_firehose.lua)
generalizes this session: instead of filtering to the three spine targets it
logs **every** story-flag SET (`FUN_8003CE08`) and CLEAR (`FUN_8003CE34`)
with the writer `ra`, every battle-id staging write (`0x8007B7FC`), and every
scene-name / game-mode transition as a context timeline - one CSV
(`flag_firehose.csv`: `tick,kind,value,pc,ra,mode,scene,count`). Per-key
repeat suppression (first 8 of each `(kind,flag,ra)`, then every 64th with a
running count) keeps hot per-tick callers from flooding the file; a long
session stays in the hundreds of KB. Prefer it for any play-forward longer
than a single beat: the same session that catches the spine writers also
banks the full flag-provenance stream for later analysis, so nothing needs
re-capturing. Same launch shape, same emulator constraints, same
`attribute_overlay_hits.py` post-pass per row.

## What the captures settled

- **Flag `0x142` - CAUGHT, then closed statically.** The SET fires at the
  **rikuroa post-Caruban beat** ("dolk clear" was a mislabel),
  `ra 0x801E3598` = the field-VM dispatcher's own `0x5x` SET arm. The source
  is script bytes `51 42` in the scene's **streaming variant MAN** (PROT
  `0157_rikuroa`, records `P1[10..12]` + the self-latching post-victory
  `P2[50]`; carrier + census in
  [script-vm.md](../subsystems/script-vm.md#a-second-script-byte-carrier-the-streaming-variant-man)).
  Save-state bracket catalogued as `rikuroa_pre_caruban` / `rikuroa_post_caruban`.
- **Story-flag provenance model (capture-proven).** Across every chapter-1
  scene traversed, story flags are written exclusively by the `0x5x`/`0x6x`
  script ops; every other setter caller is an engine system touching low
  indices (`0`/`3` entity-SM staging, `0x35` battle-end victory in
  `FUN_8004E568`, `0xB`/`0xC`/`0x18` interaction locks, `0xE` dispatcher
  spawn ops).
- **Attribution caveat:** the static field-overlay image is WRONG at the
  writer VAs (over-read/alias) - attribute callers by disassembling the
  **resident bytes from a same-mode save state**, not from static overlay
  dumps.
- The carrier-complete census closed `0x1BE` statically (geremi's
  Jeremi-arrival one-shot - never a Zeto gate). Its `0x482` closure did NOT
  hold: every census site for `0x482` is a desynced-walker alias of SJIS
  text (hand-disasm-falsified per class; the census now flags decode
  coherence per site - `GFlagSite::clean`). The `DAT_8007B7FC` battle-id
  write was falsified live (see the correction banner: silent at the Zeto
  trigger, Zeto is a `garmel` record-path fight) and is no longer a standing
  target. TWO capture targets stand: flag `549` (`0x225`) and flag `0x482`
  (Drake mist walls) - both direct code paths; for `0x482` arm a write-watch
  across the post-Zeto Drake-revival beat (`chapter2_garmel_pre_zeto`
  brackets it).

## Running the probe

Wrap the launch in `timeout --kill-after` - the probe never exits on its own,
so an unwrapped run would hang the emulator open indefinitely.

Card-save play (the primary mode). Configure PCSX-Redux to use the ladder
`.mcr` as memory card 1 (persisted in its GUI settings, or copied into the
emulator's memcards dir before launch), then cold-boot and load the slot from
the in-game load screen:

```bash
LEGAIA_NO_SSTATE=1 \
xvfb-run -a timeout --kill-after=15s 1800s \
bash scripts/pcsx-redux/run_probe.sh \
    --lua scripts/pcsx-redux/autorun_spine_flag_writers.lua
```

(Drop `xvfb-run -a` if you want to see the window and drive it yourself; the
`timeout` window should comfortably cover the play session - raise `1800s` for a
long forward run.)

Save-state seed (alternative). If you already hold a `.sstate` near a beat,
skip `LEGAIA_NO_SSTATE` and point `LEGAIA_SSTATE` at it:

```bash
LEGAIA_SSTATE=/path/to/pre-zeto.sstate \
xvfb-run -a timeout --kill-after=15s 900s \
bash scripts/pcsx-redux/run_probe.sh \
    --lua scripts/pcsx-redux/autorun_spine_flag_writers.lua
```

## Beat order for one full session

With PRO-00 loaded (or the tightest slot per beat), the flags fall in story
order, so a single armed session can sweep several:

1. **keikoku** - walk the Kikoku Cliff leg (sets `0x193`; not watched, but the
   marker that you're on the spine).
2. **rikuroa (Caruban trigger)** - enter Mt. Rikuroa; Caruban is `0x142`-gated.
   (The `zeto_battle_id` watch on `DAT_8007b7fc` was falsified live and stays
   silent - see the correction banner; Caruban's own formation mechanism is
   still un-captured and worth re-checking with the firehose.)
3. **Caruban victory** - win the fight; the post-victory record `P2[50]` sets
   `0x142` (the write the firehose caught).
4. **dolk clear** - clear the dolk dungeon to its clear beat; the
   `flag_0x142_dolk_clear` row fires.
5. **mist-wall event** - continue to the Drake mist-wall story event; the
   `flag_0x482_mist_walls` row fires (this leg needs fresh forward play; see the
   caveat above).

## What a "caught" hit looks like

Output lands under `captures/spine_flag_writers/<run-ts>/`:

- `spine_flag_writers.csv` - one row per hit: `tick,label,addr,pc,ra,value`.
- `spine_flag_writers.detail.txt` - call-context (GPRs + code + stack) for the
  first N hits of each label (`LEGAIA_MAX_DETAIL`, default 8).

A **caught** write is a CSV row whose `ra` column is **non-zero** - that is the
writer's return address, the whole point of the hunt. Concretely:

- `zeto_battle_id` - `value` should read `75` (`0x4B`); `pc` is the store site
  and `ra` its caller.
- `flag_0x142_dolk_clear` / `flag_0x482_mist_walls` - `pc` is the setter
  `0x8003CE08`, `value` is the flag index (`322` / `1154`), and `ra` is the
  game-logic caller that requested the set. That `ra` is what to attribute by
  containment (`attribute_overlay_hits.py`) to find the owning overlay.

The `[spine] HIT ...` lines in `pcsx.log` mirror each CSV row live, so you can
watch the hunt land without opening the CSV.

## Catalogue the missing pre-write states

Each beat boundary is also a chance to close a gap in the state library: no
catalogued state brackets any of these three writes. At each boundary
(pre-Zeto rikuroa, pre-dolk-clear, pre/post-mist), save a PCSX-Redux state,
then fingerprint and catalogue it so the library gains the missing brackets:

```bash
scripts/manage-states.py fingerprint     # compute the RAM fingerprint
scripts/manage-states.py library         # confirm it's backed up + catalogued
```

Load states by **fingerprint**, not slot number (slot indices are ephemeral and
get overwritten). These additions are valuable regardless of what the watches
catch this run.
