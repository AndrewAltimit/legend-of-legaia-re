# Monster (enemy) battle animation

Per-object rigid-transform keyframe animation for battle monsters. Distinct
from the [ANM container](anm.md) (which drives player / field actors): monster
animation lives **inside the monster's archive block** (extraction PROT entry
867 - retail-space CDNAME block `monster_data` under the
[−2 numbering correction](cdname.md#numbering-space); see
[monster stat archive](../subsystems/battle.md) and `legaia_asset::monster_archive`).
The archive is **not** the [player battle files](battle-data-pack.md)
(extraction 863..866, retail `battle_data`) whose extended extraction windows
historically over-read into it.

Implementation: `legaia_asset::monster_archive` (`MonsterAnimation`, `PartPose`,
`animations`, `idle_animation`).

## Where it lives

Each monster's decoded archive block is `[stat record + +0x4C action-offset
array][name][TMD mesh @ +0x04][per-action entries][texture pool @ +0x08]`. The
`magic_count` (`+0x4A`) **action entries** the `+0x4C` u32 array points at are
not just "spells" - each is an action descriptor whose head holds the action id
(`+0x00`), AGL (action) cost (`+0x74`), and a sub-id (`+0x77`), and whose **`+0x8c`**
field begins a packed transform-keyframe stream. (The runtime keyframe pointers
at entry `+0x04`/`+0x08` and the self-pointer at `+0x88` are zero on disc; the
loader reconstructs them, with `+0x88` pointing at the `+0x8c` stream.)

Action **index 0** (id `0x00`) is the neutral **idle** animation the engine
loops when the monster isn't acting; index 1 (id `0x01`) is the **move**
cycle played while the monster advances on a target (a walk for grounded
enemies, a flight cycle for fliers), and the rest correspond to the
monster's spell / special actions.

## Action tags and the `+0x1EF` reaction map

The entry's first byte (`+0x00`) is a semantic **tag**, not just an index:

| tag | meaning |
|---|---|
| `0` | idle loop |
| `1` | walk / approach cycle |
| `2`, `3` | light hit reactions (flinch variants) |
| `4` | knockdown (heavy hit / death fall) |
| `5` | get-up |
| `7`, `8`, `9` | ready / recover / defeat poses (player files) |
| `0x0B` | block |
| `0x0C..0x1F` | castable spell / special actions (monster AI roll space); within it `0x0D`, `0x0E`, `0x0F` are the monster's **attack moves** (each a distinct move per monster, gameplay-verified across the archive) |
| `0x20`, `0x21`, `0x22` | attack pre-approach / close-in / victory (monster files) |

At battle init the monster installer `FUN_80054CB0` scans the entry table and
caches the **entry index** of each tag in `{2,3,4,5,0x0B}` into battle-actor
bytes `+0x1EF..+0x1F3`; the party installer `FUN_80053CB8` hardcodes
`[2,3,4,5,0xB]` because the player files store the family identity-ordered.

The scan is a **single forward pass with no `break`**: one loop over the entry
array (`0x80055338`..`0x80055408`), five independent tag compares per entry, and
every match stores unconditionally. A monster carrying the same reaction tag
twice therefore resolves to the **last** matching entry, not the first. Do not
build this out of `FUN_80050E2C` (the entry search below), which returns on its
first match - the two routines are different mechanisms, and the shared `0xFF`
sentinel belongs only to the search.

The "no entry claimed this slot" value here is **zero**, not `0xFF`: nothing
pre-initialises `+0x1EF..+0x1F3` (the actor block arrives zeroed) and the
knockdown fallback at `0x80055428` tests `+0x1F1` against zero before copying
`+0x1EF` over it. So a monster with no knockdown entry reuses its light flinch -
and a monster whose knockdown entry sits at index `0` is indistinguishable from
"absent" and takes the fallback anyway. Entry `0` is the idle loop for every
monster in the archive, so that second case never fires on retail data.

Consumers:

- the damage primitive `FUN_800402F4` stages the target's reaction from the
  map - a surviving target with no get-up entry queues `+0x1EF` (light
  flinch, with the exit-to-idle flag), any other hit queues `+0x1F1`
  (knockdown);
- the anim commit `FUN_8004AD80` chains a finished knockdown (record tag 4)
  into `+0x1F2` (get-up) while the actor lives, or anim id 7 for a downed
  party member, and tests the queued id against `+0x1EF/+0x1F0/+0x1F3` for
  the counter/guard window;
- the battle-action SM (`FUN_801E295C`) resolves monster attack anims by
  **first-byte search** over the entry table (`FUN_80050E2C` with tags
  `0x20`/`1`/`0x21`/`0x22`), staging the returned *index*.

## Anim selection (`actor +0x1D9/+0x1DA` → entry)

The per-actor anim state is a pair of bytes: `+0x1DA` = queued anim id,
`+0x1D9` = current. The id **is the entry index** - the commit function
`FUN_8004AD80` installs `node+0x4C = *(record_ptr + 0x4C + id*4)` for
monsters (record pointers at `0x801C9348 + (slot-3)*4`) and
`node+0x4C = *(table + id*4)` for party (per-character record[0] tables at
`0x801C9360 + slot*4`), then snaps `+0x1D9 = +0x1DA`. There is no remap
table and no special case for id 6: retail's idle id is **0**, and the
battle SM's `FUN_801D5854(actor, 6..9)` "pose" calls are a separate
camera/presentation program space that never touches the anim fields.
Party ids `≥ 0x10` (basic swings staged as direction bytes `0x0C..0x0F` are
still table-direct; art starters `0x19`/`0x1A` and art constants `0x1B+` are
not) trigger the dynamic-slot path instead - see
[`battle-data-pack.md` § Battle animations](battle-data-pack.md#battle-animations-record0).

The **player battle files** carry the same per-action entry family for the
party's assembled meshes - action-offset table at the head of `record[0]`,
packed stream at entry `+0xAC` instead of `+0x8C`, `parts` = the
character's skeleton bone count. See
[`battle-data-pack.md` § Battle animations](battle-data-pack.md#battle-animations-record0).

### A special attack can be a chain of entries

An action id is one entry, but a **move** need not be. The per-spell cast
modules paged into the slot-B overlay window for a capture-class cast
([`spell-table.md` § Capture-class module index](spell-table.md#capture-class-module-index-prot-09350966))
drive the caster's clip themselves, writing `actor[+0x1DA]` directly, and a
boss signature attack is several archive entries staged in sequence - a
wind-up, a carry and a strike - rather than one long clip. Nothing in the
archive marks the grouping: the chain lives in the module's code.

The staging sites are `sb ?,0x1DA(<caster>)` in the module image, at module VA
= the slot-B link base `0x801F69D8` + file offset. Sites that store
`<target>[+0x1F1]` (the knockdown index from the reaction map) are the
victim's clip, not the caster's, and the ~936-byte tail the modules share -
including a `sb zero,0x1DA(s0)` at `0x801F96F4` - is a common epilogue rather
than a per-move stage. For the three Delilas siblings:

| Module | Cast | Caster stages | Closing entry |
|---|---|---|---|
| PROT 958 | Gi, `0x79` | `10 -> 11 -> 12` | `13` |
| PROT 959 | Che, `0x7A` | `10 -> 11` | none |
| PROT 960 | Lu, `0x7B` | `14 -> 12 -> 13` | `15` |

Two staging idioms appear. A **literal** seeds or jumps the chain: `li v0,10`
at `0x801F6F88` (Gi), `li v1,10` at `0x801F6F40` (Che), and Lu's whole chain
as three of them - `li v0,14` at `0x801F7744`, `li v1,12` at `0x801F7A6C`,
`li v0,13` at `0x801F7AE0`. A **stepper** advances it in place,
`lbu +0x1DA; addiu +1; sb`: Gi's at `0x801F72C0` / `0x801F7628` /
`0x801F854C`, Che's single one at `0x801F768C`. The closing entries are
literals too - `li v0,13` at `0x801F89B0` and `li v0,15` at `0x801F8214`.

The archive agrees with the scan. Gi's `10`/`11`/`12` are his three
consecutive tag-`0x23` entries (11 / 30 / 23 frames, rates 1 / 2 / 2) with
`13` his tag-`0x22` close; Che's `10`/`11` are his only two tag-`0x23`
entries (50 / 50 frames) and his archive carries no tag-`0x22` entry at all,
which is why his module stages no closing index; Lu's `14` is her tag-`0x23`
entry and `12`/`13` her two tag-`0x0C` entries (16 / 19 / 39 frames, rates
1 / 2 / 2), with `15` her tag-`0x22` close.

Two consequences for anyone reading a chain out of the archive. **The stages
are addressed by entry index, not by tag** - Gi's and Che's are all tagged
`0x23`, so no tag search separates a stage from a generic castable. And
**stages do not share a rate byte**, so a chain's real duration is
`sum(frames_i * 8 / rate_i)` ticks, not a frame sum.

## Event-frame list (entry `+0x10..+0x13`)

Four bytes ahead of the effect script are a **zero-terminated list of up to
four clip frame indices** - the action's own significant beats. The list is
strictly ascending and never holed: across every action entry of every archive
in PROT 867 no populated run is out of order and no zero is followed by a
non-zero, and only four entries carry a value at or past their own
`frame_count`. Frames, not sixteenths - these are compared against the anim
tick's integer frame, `cursor >> 4`.

Both traced consumers are in the anim tick `FUN_80047430`, and both locate the
slot through the helper `FUN_80050E00(entry + 0x10)`
(`ghidra/scripts/funcs/80050e00.txt`), which walks `+0x11..+0x13` and returns
the 1-based index of the first zero, capped at `3`. The tick then reads
`entry[+0x10 + index]`:

- `0x80047918` - the event-flag path. With `actor[+0x1DC]` bit 1 set and
  `entry[+0x76] == 0`, the queued clip commits mid-clip once
  `event_frame + 2 < frame` (the bit-1 arm of [Playback](#playback) below).
- `0x80047E28` - runs every tick and latches
  `actor[+0x1F7] = (frame < event_frame)`. That byte gates `actor[+0x1F6]`
  in the commit `FUN_8004AD80` (the counter / guard window, which additionally
  requires the staged id to be one of the `+0x1EF`/`+0x1F0`/`+0x1F3` reaction
  entries) and paces the arts-input band.

Because the helper returns the index of the **terminator**, only a list with
all four slots populated hands its consumer a real frame - `entry[+0x13]`, the
last beat. Any shorter list resolves to the zero that ends it, i.e. no gate.
Those are the only two consumers reachable: a word-wise `jal` scan over
`SCUS_942.54` and every image in `extracted/overlays/` finds no third call
site for `FUN_80050E00`.

Reading the field as "the hit frames" fits the offensive entries and no more.
The archive census by tag is unambiguous about that - idle, walk, knockdown,
get-up and the approach/victory tags are all-zero almost everywhere, while
**every** light-flinch (tags `2`/`3`) and every block (`0x0B`) entry carries
exactly one early frame, and the castable/attack band (`0x0C..0x15`) carries
one to four ascending ones. So the field marks a clip's beats generally, of
which contact is the offensive case; a reaction's single value is the beat its
own clip turns on, not a hit it deals (Confidence: the layout, the ordering
invariant and the two consumers are **Confirmed**; "contact" for the offensive
band is **Inferred**).

The same field sits at the same offset on the player battle files' record[0]
entries, their equipment swing records and the art-bank records' embedded
entries ([`battle-data-pack.md`](battle-data-pack.md#battle-animations-record0)),
with the same shape - Vahn's flinch and block entries each carry a single
frame `3`, his idle / walk / knockdown / get-up entries none.

## Effect-script records (entry `+0x14..+0x53`)

Every per-action entry's head carries the action's **battle effect script**:
up to eight 8-byte records the battle anim-node tick walks once per frame to
place the action's visual effects. The walker is `FUN_801DEA50`, reached only
from the per-frame anim-node tick `FUN_80047430` (`jal` sites `0x800478B8` /
`0x80047C08`); its block argument is the committed anim record itself
(`node[+0x4C]`, shadowed from `actor[+0x234 + i*4]` by `FUN_80049348`), so
"the effect script" is simply this region of the entry - record `cursor` sits
at `entry + 0x14 + cursor*8` (`0x801deca8`), cursor bound `8`.

```
+0x00  u8   frame_gate   // skip while anim frame+1 < gate; 0 ends the walk
+0x01  u8   effect       // & 0x7F == 0x7F terminates (installs move power);
                         // & 0x80 selects the direct spawn (FUN_801DFDF0),
                         // else 0x801F6324[effect] via FUN_80050ED4
+0x02  i16  off_x        // actor-local offsets, scaled by the render node's
+0x04  i16  off_y        //   mesh scale (+0x72) and rotated by the actor's
+0x06  i16  off_z        //   facing (+0x46); Y is SUBTRACTED
```

The rotation reads the sin/cos pair `_DAT_8007B81C` / `_DAT_8007B7F8` -
both point into **one** static SCUS sine table at `0x80070A2C` (5120 `i16`
entries of `trunc(sin(i*2pi/4096)*4096)`, installed by `FUN_80026BE0`; the
`+0x800`-byte second pointer is the quarter-revolution cosine view, and the
table's last 1024 entries repeat its first 1024 so that read never wraps).

The same region exists on the player battle files' record[0] entries, the
equipment swing records, and the art-bank records' embedded entries
([`battle-data-pack.md`](battle-data-pack.md#battle-animations-record0)) -
retail walk clips carry per-footfall dust records, hit reactions carry impact
flashes, and monster casts carry per-frame emitter trains. Terminator
(`0x7F`) records appear only on entries that install a move-power record
([`move-power.md`](move-power.md)).

Parser: the region rides `legaia_asset::monster_archive::MonsterAnimation::effect_script`
(the entry head `+0x00..+0x54`); the walker port is
`legaia_engine_core::action_effect_script::step_effect_script`.
`see ghidra/scripts/funcs/overlay_battle_action_801dea50.txt`.

## Packed stream (entry `+0x8c`)

```
u8  part_count    // animated objects per frame == TMD object count
u8  frame_count
frames[frame_count]:
  parts[part_count]:
    u8 b[9]       // six packed 12-bit fields (see below)
```

Each part record is 9 bytes encoding six 12-bit fields. Low bytes sit at
`[0,1,3,4,6,7]`; the high nibbles are packed into `[2,5,8]`:

```
v0 = b0 | (b2 & 0x0f) << 8     tx  (translation X)
v1 = b1 | (b2 & 0xf0) << 4     ty
v2 = b3 | (b5 & 0x0f) << 8     tz
v3 = b4 | (b5 & 0xf0) << 4     rx  (rotation X)
v4 = b6 | (b8 & 0x0f) << 8     ry
v5 = b7 | (b8 & 0xf0) << 4     rz
```

- `tx, ty, tz` are **sign-extended** 12-bit (`-2048..2047`) translation in TMD
  model units.
- `rx, ry, rz` are **unsigned** 12-bit Euler angles (`0..4095`, where `4096` =
  a full turn); values near `4095` are small negative rotations.

One part maps to one [TMD](tmd.md) object (a rigid body part). Across the retail
roster the part count equals the TMD object count for >98% of actions (one model
carries an extra non-animated object).

The transform is **absolute model-space**, not a delta from a rest pose: each TMD
object is modelled at its own local origin (all parts overlap near `(0,0,0)`),
and the per-part `[tx, ty, tz]` places that object at its socket while
`[rx, ry, rz]` orients it about its local origin. The assembled vertex is
`world = Rz·Ry·Rx · v_local + t`. **Frame 0 is therefore the assembled rest
pose** - the translations of a humanoid's left/right limb objects are mirror-symmetric (e.g. Gobu Gobu's arm sockets at `tx ≈ +120` / `-115`), and assembling
frame 0 spreads the collapsed model into its full silhouette.

## Playback

The renderer (`FUN_80048a08`) keeps a 12.4 fixed-point phase in the per-actor
draw struct (`+0x68`): integer frame index = `phase >> 4`, sub-frame fraction =
`phase & 0xf`. The decoder (`FUN_8004998c`) interpolates between frame `i` and
`i+1`: **linear** for translation, **shortest-path angle-wrap** for rotation
(`& 0xfff`, treating a `> 0x800` gap as a wrap). The result is written to a pose
buffer (6 shorts per object) and applied per object via the GTE in the draw
loop, then `FUN_800495c8` / `FUN_8005b038` blend it onto the object vertices.

The per-frame cursor advance lives in the anim-node tick `FUN_80047430`:
`phase += (frame_dt * actor[+0x21D] * record[+0x78]) >> 1`, where `+0x21D`
is the actor's speed scale (normally `4`) and the entry's `+0x78` byte is
its **playback rate** (`1` or `2` across the retail corpus - `rate/8`
keyframes per 60 Hz tick in the normal case). When the cursor passes the
stream's frame count (or the `+0x1DC` event flags fire mid-clip) the tick
calls the commit `FUN_8004AD80`, which swaps the entry, zeroes the cursor,
and converges `+0x1D9 = +0x1DA`. On the last frame of a clip the decoder
cross-blends toward **frame 0 of the queued entry's stream** (looked up by
`+0x1DA`), so anim transitions tween rather than snap. Entry `+0x84` seeds a
loop-hold counter (`actor +0x176`) and `+0x85`/`+0x86` bound a loop window
(e.g. the player defeat entries hold a 2-frame loop); `+0x87` is a sound
cue fired at install.

Three consequences of that commit shape:

- **A "looping" clip (idle, the walk/Move cycle) loops by re-committing.**
  Nothing counts loops: at every natural end the commit re-installs the
  still-queued `+0x1DA` and zeroes the cursor, so the same entry replays
  seamlessly until something re-stages `+0x1DA`.
- **The event-flag byte `actor +0x1DC` steers the commit.** Bit 0 = commit
  now; bit 1 = commit when the cursor passes the entry's event frame
  (list at entry `+0x10`, gated on `entry[+0x76] == 0`); bit 2 = **stage
  idle at the natural end** (the tick clobbers `+0x1DA` to `0` at
  `0x80047B44` before committing - the exit-to-idle that returns a hit
  reaction to the idle loop, set with bit 0 by the damage primitive
  `FUN_800402F4`); bit 3 = knockdown latch (set by the commit's tag-4
  chain; blocks the root-motion drive below). The two commit sites clear
  the byte **asymmetrically**: the mid-clip event path clears bits 0-1
  only (`andi 0xFC`) while the natural-end path clears bits 0-2
  (`andi 0xF8`) - so a pending bit 2 survives an event-path commit onto
  the next clip, the race behind the summon-then-melee `0x19` park
  ([battle-action.md](../subsystems/battle-action.md#the-stale-field-0x1dc-bit-2-the-exit-to-idle-anim-event-flag)).
- **Approach root motion is the tick's, not the SM's**: while a clip plays
  (and `+0x1DC` bit 3 is clear), `0x80047D20..0x80047E18` advances the
  actor by `facing sin/cos × entry[+0xC] × frame_dt × actor[+0x21D] >>
  0xF` per tick while out of range, and entry `+0xE` contributes a
  phase-proportional displacement at clip end - which is how a monster
  slides toward its target during the `0x19` range poll.

### Vertex-blend variants (`FUN_800495C8` / `FUN_80049858`)

Once the decoder has a pose, two sibling routines write it onto the object
vertices in the per-actor draw buffer at `gp[0xA0C] + slot*4 + 0x1060`. Both
walk the actor's TMD object list (`obj[+0xC]` primitive groups reached through
the monster-object table `DAT_801C9370 + actor[+0x5A]*4 → +0x230`) and copy
each primitive's 8-byte packed vertices out of the pose scratch:

- `FUN_80049858` is the straight copy - two fixed passes (part indices `1`
  then `0`) that move the interpolated vertices into place with no further
  weighting.
- `FUN_800495C8` is the **morph** variant: it first derives a 12-bit blend
  factor from the actor's animation phase `actor[+0x68]` against a per-object
  frame-range table (bytes `[1..=4]` of the entry), then, after the same
  vertex copy, calls the blend kernel `FUN_8005B038(dst, group+3, count,
  factor)` to lerp each vertex toward its next-frame target - sub-keyframe
  vertex morphing on top of the rigid TRS decode.

Both thread GTE-buffer pointers and gp globals, so neither is a pure function;
the port keeps to the rigid-TRS decode already in `engine-vm/anim_vm.rs` and
defers vertex morphing to the renderer.

## Provenance

- `FUN_8004998c` - packed-stream decoder + frame interpolation (`ghidra/scripts/funcs/8004998c.txt`).
- `FUN_80048a08` - per-actor battle draw; reads the phase, drives the decoder, applies the pose per object (`ghidra/scripts/funcs/80048a08.txt`).
- `FUN_80048310` - a second decoder consumer: the weapon swept-trail builder (documented in [`battle.md`](../subsystems/battle.md#weapon-trail-builder-fun_8005112c--fun_80048310--fun_800485bc)) calls `FUN_8004998c` per sweep step, then submits the swept quads through the GTE emitter `FUN_800485bc`. Ported: `engine-vm::battle_trail` + `engine-ui::battle_trail`, sweeping the engine's pose-history ring instead of re-decoding (`ghidra/scripts/funcs/80048310.txt`, `800485bc.txt`).
- `FUN_800495c8` / `FUN_8005b038` - GTE vertex morph-blend of the decoded pose (`ghidra/scripts/funcs/800495c8.txt`, `8005b038.txt`).
- `FUN_80049858` - the non-morph sibling: straight two-pass vertex copy of the decoded pose (`ghidra/scripts/funcs/80049858.txt`).
- `FUN_80054cb0` - monster init; copies the action/effect pointer (record `+0x04`) into actor `+0x230` and builds the `+0x1EF..+0x1F3` tag map (`ghidra/scripts/funcs/80054cb0.txt`).
- `FUN_80047430` - per-frame anim-node tick: cursor advance, end-of-clip detect, commit dispatch (`ghidra/scripts/funcs/80047430.txt`). Its own caller is not in the dump corpus (open).
- `FUN_8004AD80` - anim commit/transition: id → entry install, `+0x1D9` convergence, reaction chaining, dynamic party art slots (`ghidra/scripts/funcs/8004ad80.txt`).
- `FUN_800402F4` - damage primitive; stages the target's hit reaction from the `+0x1EF` map (`ghidra/scripts/funcs/800402f4.txt`).
- `FUN_80050E2C` - **first-match** tag search over the entry-pointer array. Signature `(table, tag, count) -> idx_or_0xFF`; both `count` and the result are byte-truncated, so a table longer than 255 entries is unrepresentable and index `0xFF` is indistinguishable from the "not found" sentinel. Ported as `legaia_asset::monster_archive::find_action_by_tag` (sentinel surfaced as `None`).

Provenance for `FUN_80050E2C`: the first-match shape and the `0xFF` sentinel were read off `SCUS_942.54` directly at file offset `0x800 + (0x80050e2c - 0x80010000)`, because the dump then carried decompiled C over an empty disassembly section. That is **no longer the state of the dump** - `ghidra/scripts/funcs/80050e2c.txt` now disassembles the whole 72-byte body, including the returning `addiu v0,zero,0xff` at `0x80050e68`. The hand-read stands and the dump now corroborates it; this is no longer a routine whose behaviour cannot be checked from the corpus.

Both take their tags from `action_tags`, which walks **every** entry in the `+0x4C` array. That matters: `animations` skips entries whose keyframe stream is empty or malformed, and since the engine addresses animations by raw entry index (`+0x1DA`), pairing an index against the filtered list mis-maps it.

Two properties of the map are **not observable on the retail disc**, so no disc-gated test can pin them and the CI-side synthetic tests in `legaia_asset::monster_archive::animation` are what hold them: no shipped monster duplicates a reaction tag (so last-wins and first-wins agree everywhere on disc), and every monster carrying a light-flinch entry also carries a real tag-4 knockdown (so the fallback never fires). The disc-gated `monster_reaction_maps_match_an_independent_last_wins_transcription` checks the port against a separate transcription of the loop over all 120 archives and reports the duplicate-tag census rather than asserting a behaviour it cannot reach.

## Engine playback

The clean-room engine plays this stream for battle actors. At battle entry the
shell decodes each monster's idle clip (`idle_animation`, action 0) into a
`legaia_engine_core::battle_anim::MonsterAnimPlayer` - an 8.8 fixed-point loop
cursor whose `tick()` interpolates the keyframes (translation linear, rotation
shortest-path 12-bit step, matching `FUN_8004998C`) into a
`legaia_anm::PoseFrame` (one `(translation, rotation)` per object, the same
shape the field ANM player produces). `World::tick_battle_animations` advances
every battle actor's player each frame, and the renderer deforms the mesh with
the rigid `legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot` builder (`R·v + T`,
`Rz·Ry·Rx` about each object's local origin - the same composition as the
glTF export below and the site animator). The disc-gated `battle_anim_real`
test drives the whole decode → player → deform path on a real monster and
asserts the posed mesh moves frame-to-frame. The per-tick phase advance is
retail-pinned through the entry's rate byte
(`battle_anim::step_for_rate`, the `FUN_80047430` formula reduced to the
normal `frame_dt = 1`, `+0x21D = 4` case); the engine also plays the
hit-reaction family - `World::queue_battle_reaction` mirrors the
`FUN_800402F4` staging and `tick_battle_animations` the knockdown → get-up
chain. The decoder's cross-blend into the queued clip is a known engine
simplification (transitions restart at frame 0 without the tween).

## Export

`legaia_asset::monster_gltf::export_glb(entry, id)` packs a monster's mesh, its
baked texture, and **every** action animation into one binary glTF (`.glb`) - the
universal interchange format. The rigid-per-object model maps directly onto glTF
node animation: each TMD object becomes a node, the keyframe stream's
translation + Euler rotation drive that node's `translation` / `rotation`
channels (the `Rz·Ry·Rx` order recomposed as a quaternion), and a root node
rotates the rig 180° about X to convert the PSX `+Y`-down space to glTF's
`+Y`-up. The per-prim CLUTs (`cba & 0x3F`) that a single glTF material can't
index are baked into a vertical palette atlas, with each vertex's `V` remapped
into its palette band. Shading rides a `COLOR_0` attribute (the prim's packet
word as the `texel * colour / 128` blend factor) over a material marked
`KHR_materials_unlit` - retail applies no light source, so a lit material
would invent one; see [the renderer page](../subsystems/renderer.md#the-same-shading-in-an-exported-glb).
CLI: `asset monster-archive --id N --glb <out>`; the
enemy-table web page exposes the same export as a download button.

## See also

- [Legaia TMD](tmd.md) - the mesh whose vertices these keyframes morph.
- [ANM animation](anm.md) - the player/field-actor animation container.
- [Player battle files](battle-data-pack.md) - the sibling `battle_data` block (party-character containers, a distinct format from this archive).
- [`subsystems/battle.md`](../subsystems/battle.md) - the battle scene that drives the playback.
