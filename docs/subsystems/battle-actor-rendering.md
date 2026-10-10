# Battle actor rendering

### Battle party meshes (assembled)

The party renders the real **battle-form meshes**, assembled per character the
way the retail loader builds the blobs it installs into `DAT_8007C018[0..=2]`:
each member's mesh is spliced from their player battle file's equipment-id
sections (`legaia_asset::battle_char_assembly`, extraction PROT 863..865,
equipped ids from the roster record's `+0x196..+0x19A` bytes) and relocated
into the slot's runtime VRAM band by
`battle_char_assembly::relocate_tsb_cba` (the registration-time TSB/CBA pass,
`FUN_80053a28` - texpages `x ∈ [512, 896), y = 256`, CLUT row `481 + slot`;
see [`character-mesh.md` § Battle render](../formats/character-mesh.md#battle-render-load-time-tsbcba-relocation)).
PROT 1204 (the Baka Fighter / default-equipment sibling pack) is the
per-member fallback when assembly fails, and supplies the atlas pixel pages -
uploaded at their authoring rects and, when an assembled mesh is bound, also
written into the runtime band the relocated meshes sample.

The battle char TMD is a set of object-local pieces (head/torso/limbs),
**not** a single pre-assembled mesh, so the engine sockets them with the
**character's own idle keyframe stream from `record[0]` of the same player
file** (`battle_char_assembly::idle_battle_animation` - the monster-format
`[parts][frames][9-byte TRS]` stream at action entry `+0xAC`, `parts` =
skeleton bones; see
[`battle-data-pack.md` § Battle animations](../formats/battle-data-pack.md#battle-animations-record0)).
Frame 0 is the combat-stance rest pose, applied `R*v + T` per object
(`tmd_to_vram_mesh_posed_rot`); the clip then loops through the same
`MonsterAnimPlayer` the enemies use. Channel `i` drives object `i` directly
(post-sort object index == bone tag); the `expand_animation_for_objects`
pass duplicates each `200+` equipment extra's **attach-bone** channel onto
it (the assembler's `anm_bones` map), which is what makes the duplicate
weapon/Ra-Seru pieces coincide with their attach piece instead of floating
apart. The **PROT 1203 ANM (`other5`) is NOT this pose source** - its banks
(Vahn @ 0 / Noa @ 9 / Gala @ 18) are authored against PROT 1204's own
object order, which differs from the assembled tag order per character, so
it stays the rest-pose source for the **1204 fallback mesh only** (identity
object→bone). Pinned live + cross-pipeline in
`crates/engine-shell/tests/battle_party_pose_live.rs`. Palette: every
upload block is `[CLUT struct][pixels]` and `FUN_80053B9C` writes both
halves, so the band uploads of record[0] and the five **equipped** sections
already put the character's palette on row `481 + slot` - the equipped
pieces in their own colours (a Ra-Seru armour set is not the unequipped
default). The separator-default collectors (Vahn `parse_record`, Noa/Gala
`collect_palette`) only paint a fallback picture - a PROT 1204 mesh, or an
assembled mesh whose pool decode failed; running them over a real band
repaints every equipped piece in default colours. Pinned against two
late-game captures in
`crates/engine-core/tests/battle_party_palette_retail_capture.rs`.
A 4th party slot is not rendered: the runtime texture band + CLUT rows cover
party slots 0..=2 only, so Terra (player file 866, idle stream 17 parts)
has no relocation target.

The party's forms are an engine duty, not a host one: retail's loader
assembles the party with the fight whether or not anything draws it, so
`SceneHost::tick` runs `SceneHost::ensure_battle_party_forms` the tick a
battle is up, once per fight (`BattleState::entry_serial`, bumped by
`World::enter_battle`). Every session gets the idle, action clips, art bank
and art records - the play hosts and headless drivers (the ladders, the soak
harness) alike; without them a fight swings zero-length clips and matches no
art. The kernel is `engine-core::battle_party_form`
(`install_party_battle_forms`: `PartyFormSources::load`, then
`build_party_battle_form` per member, then `World::install_party_battle_form`).
Its VRAM writes go to a `VramWriteLog` rather than a live VRAM; a renderer
reads `SceneHost::battle_party_forms`, replays the log into the battle VRAM
it composes, and adds only the GPU upload, its own posing and the facial
animator's registration. A session that drives a bare `World` with no
`SceneHost` has no disc index, and is the one explicit injection point: it
calls `install_party_battle_forms` itself after `World::enter_battle`. The
kernel falls back to PROT 1204 when the
player file carries no idle stream - an assembled mesh with no pose source
draws every piece at its object origin - and overlays the separator-default
battle palette on a fallback mesh's rows only. Monsters install their texture slot and idle
clip through `World::install_monster_battle_form`, and the slot a mid-battle
summon takes is one past the highest monster slot bound
(`battle_party_form::monster_tex_slots_used`), since repeated species share
a slot.

#### The battle display list is the registration set, not `active`

Retail's loader gives the fight its own actor set: `FUN_800513F0` registers
the backdrop, the party blobs and the monster meshes into `DAT_8007C018[]`
and links **those** actors into the render OT. The field scene's actor list
does not survive the transition.

The port keeps one actor array across the transition (the world clones it
into `field_return` and restores it at battle end), so every field slot
arrives in the battle still holding its scene-mesh binding and draws at
whatever battle-world coordinates its `move_state` carries - which for a
scene actor that never moved is the **origin**, dead centre of the arena
between the two rows.

"Draw only `active` actors" is **not** a sufficient gate, and the earlier
reading that the leftover slots are all inactive is false: rikuroa hands the
battle two live field actors, which drew a scene prop over the party member
and made the fight look like it had no party in it at all. The registration
set is the gate - `unregister_non_battle_meshes` (native host
`window/battle.rs`) drops the `tmd_binding` of every slot the battle loader
did not just register, so the display list is exactly what the loader built.
Nothing is stashed for the restore: the bindings return with the field actor
table. Regression: `battle_display_list_tests` in the same module.

`LEGAIA_DIAG_BATDRAW=1` prints that display list at battle entry - one row
per bound slot with its role (party ordinal / monster id / `STRAY`), seat,
mesh vertex count and projected seat. It is the "which meshes is this battle
actually drawing" instrument; `LEGAIA_DIAG_BATCAM` answers the per-frame
framing question and `LEGAIA_DIAG_POSE` the per-frame mesh one.

### One staged-anim channel: `actor[+0x1DA]`

Which clip an actor plays is a **single byte**, `actor[+0x1DA]`, with two
committed mirrors: `+0x1D9` (the *playing* id - what the end-of-clip
chains and a cast module's confirm gates read back) and `+0x1DB` (what the
camera-variant dispatch in `FUN_801D5854` keys on). Every producer writes
that same staged byte, and the last writer wins:

| Producer | Site | What it writes |
|---|---|---|
| Action SM, party approach | attack band state `0x14` | literal `1` (the walk entry) |
| Action SM, strike loop | attack chain | the strike-script byte (`0x0C..0x0F` swings, art ids) |
| Damage arm, flinch | `FUN_800402F4` `0x80042124` | `actor[+0x1EF]` (tag-2 entry) |
| Damage arm, knockdown | `FUN_800402F4` `0x80042118` | `actor[+0x1F1]` (tag-4 entry) |
| Knockdown → get-up chain | `FUN_8004AD80` `0x8004BEC4..0x8004BECC` (living actor); `0x8004B690` (dead monster, Seru staged) | `actor[+0x1F2]` (tag-5 entry) |
| Capture-class cast module | slot-B module code ([cast-module.md](cast-module.md)) | caster stage literals / steppers; victim `actor[+0x1F1]` |

The commit `FUN_8004AD80` snaps `+0x1D9 = +0x1DA` and copies `+0x1DB`
unconditionally (`0x8004AEB0..0x8004AEB8` for the `+0x1DB` copy; see
`ghidra/scripts/funcs/8004ad80.txt`); there is no reaction guard anywhere
on that path.
So a hit reaction is not a mode an actor is *in* - it is just the current
value of the staged byte, and the next thing the SM stages replaces it.

Which arm the damage takes is decided at `0x800420F4..0x80042124`: flinch
when `actor[+0x1F2] == 0` (no get-up entry) **and** the damage is survivable,
knockdown otherwise. The `+0x1EF..+0x1F3` map is filled by `FUN_80054CB0`
(`0x80055360..0x800553F0`), one slot per action tag `2/3/4/5/0xB`, with the
tag-4 → tag-2 fallback at `0x80055428`. Every player battle file carries a
tag-5 entry, so a party member takes the **knockdown** arm on any hit.

The port models the reaction with its own `Actor::battle_reaction` latch
(`engine-core::world::actors`) because its `Pose` hook - the per-frame
`pose(Idle)` the attack band issues - is an engine-local channel with no
retail counterpart and would otherwise cancel a reaction on the frame after
it starts. That latch must **not** outrank the staged channel:
`commit_staged_battle_anim` clears it whenever it installs a staged clip.
Giving the latch priority instead is what made a hit party member spend its
whole attack turn face-down - it walked to the target and back playing the
knockdown / get-up pair, and the approach clip plus every weapon swing were
dropped on the floor. Regression:
`crates/engine-core/tests/battle_reaction_stage_precedence.rs`, plus the
GPU-free pose oracle in
`crates/asset/tests/battle_pose_orientation_real.rs` which pins that the
upright family really is upright and the reaction family really is prone.

### The commit's clip-tag ladder

Every path through the commit `FUN_8004AD80` converges at `0x8004BDD8`: the
new entry is installed at node `+0x4C`, `+0x1D9 = +0x1DA`, the entry's
`+0x84` / `+0x87` bytes are folded, and then the routine tests the **new**
entry's tag byte `+0x00` once per commit (`0x8004BE30..0x8004BF4C`; see
`ghidra/scripts/funcs/8004ad80.txt`):

| Tag | Condition | Writes |
|---|---|---|
| `2` | first monster `0xB3` or `0xB5`, committing actor at HP `0` | the entry's tag byte becomes `4` in place (the tag-`4` row then runs); for `0xB3` also entry `+0x56 += 1` |
| `4` | HP not `0` | `+0x1DA = +0x1F2` (the get-up, staged behind the knockdown), `+0x1DC = 0` |
| `4` | HP `0`, party seat | `+0x1DA = 7`, `+0x1DC = 0` |
| `5` | - | `+0x1DC` bit `0x4` set (idle at the natural end) |
| `7` | - | `+0x1DA = 8` |
| `8` | - | `+0x1DC` bit `0x8` set (the latch that stops root motion, `0x80047D20`) |

`+0x1DA` is what the natural-end path commits next: `FUN_80047430`
(`0x80047B30..0x80047B58`) replaces it with `0` when `+0x1DC` bit `0x4` is
set, masks `+0x1DC &= 0xF8` - bit `0x8` survives - and calls the commit. So a
downed party member's chain is knockdown, then entry `7`, then entry `8`,
and entry `8` re-commits itself at every natural end with the latch raised;
it is the entry-`8` commit, not the knockdown, that sets the latch, and the
knockdown's own commit clears `+0x1DC`. Three catalogued battle states each
read a dead party member at `+0x1D9 = +0x1DA = 8`, `+0x1DC = 8`. The party
files carry entries `7` and `8` with `tag == slot` (Terra's hold empty
streams), so this chain is where those entries play; the action SM never
stages either id (its literal `+0x1DA` stores are `0`, `9` and `0x15`, the
rest come from the queue bytes and the tag searches).

The monster-death arm earlier in the routine (`0x8004B094..0x8004B6A0`, the
committing monster's **previous** entry tagged `4` at HP `0`) re-installs
that entry held on its last frame, runs the death spoils (ported:
`battle_steal`), sets `+0x21C = 2` and the same bit-`3` latch, and - with a
Seru staged in `ctx[+0x269]` - overwrites `+0x1DC = 4` and restages the
get-up `+0x1F2` (`0x8004B688..0x8004B6A0`), so the fallen monster rises for
the absorb. Captures hold both: dead monsters on their knockdown entry with
`+0x1DC = 8` and `ctx[+0x269] = 0`, and one on its get-up entry with
`+0x1DC = 4` while `ctx[+0x269] = 1`.

**Engine.** `engine-core::world::battle::clip_ladder` ports the ladder as a
pure kernel (`commit_tag_ladder`) and runs it on the reaction channel: a hit
commits the `+0x1EF` / `+0x1F1` entry (the `FUN_80054CB0` map - last match
wins, a missing knockdown takes the flinch entry), the ladder's staged entry
is committed at the clip's natural end, the downed party chain and the
monster-death arm (latch, spoils, Seru get-up) follow the table, and the
root-motion drive reads the latch in `battle.flag_bits` rather than "a
knockdown is playing". One timing seam remains: the port stages the reaction
inside the hit, ahead of the combo total's HP write, so the tag-`4` row is
evaluated at the knockdown's natural end on the landed HP instead of at its
commit. The tag-`2` rewrite changes the clip's tag, not the disc entry, and
the `+0x56` bump has no engine field.

### Monster mesh (record `+0x04`)

Each decoded monster block carries the monster's **battle model**: a
[Legaia TMD](../formats/tmd.md) embedded at the block-relative offset held in
the stat record's `+0x04` field (immediately after the name string). This is
the same pointer the loader installs at battle-actor `+0x230` and that
`FUN_80049858` / `FUN_800495C8` walk as `0x1C`-stride records - a TMD
object-table entry is exactly `0x1C` bytes, so that walk is iterating the
mesh's per-object table. Verified across the archive: **186 of the 194 slots
carry a Legaia TMD at `+0x04` that the parser walks cleanly** (the other 8 are
empty / filler ids); e.g. Gimard (id 10) = 200 vertices / 269 textured prims
at block `+0x7c`.

Decoded-block layout (after the stat-record head at `+0x00`):

```
+0x00  stat record head (name_offset, +0x04 mesh offset, +0x08 pool offset, stats, rewards, spells)
name   NUL-terminated name string (at name_offset, typically just before the mesh)
+0x04→ Legaia TMD              ; the monster's battle model (magic 0x80000002)
spells spell-entry blobs       ; each carries its own attack-effect geometry
+0x08→ texture / CLUT pool     ; per-monster palettes + 4bpp texture pages
```

The name string carries a two-byte **element-icon escape**: a `^` + letter
prefix (`^A Gimard`, `^F Aluru`) the battle UI renders as the element badge,
in the fixed order `^A`=Fire, `^B`=Thunder, `^C`=Wind, `^D`=Water, `^E`=Earth,
`^F`=Light, `^G`=Dark, `^H`=Evil (the icon-glyph row `0x1D..0x24` in the same
order - **not** the element-id order of the [`+0x1D` element byte](battle.md#monster-record-source-layout)).
Across the roster every carrying monster's caret letter agrees with its
element byte, with **no** exceptions - `^H` maps to element byte `7` (the
no-affinity id whose matrix row and column are all-100) exactly as the other
seven letters map to theirs; only 64 of the 186 populated records carry an
escape at all. The
[per-letter census](battle-hud.md#the-element-badges-and-their-per-badge-palette) has the
counts. Boss-tier `$2`/`$3` name suffixes are literal ASCII, not
markup.

The mesh's primitives are textured: they reference a CLUT + a 4bpp texture page
via per-prim CBA/TSB. The matching palette + pixel bytes live in the **texture
pool at record `+0x08`**, whose layout is pinned from the battle loader
`FUN_80055468` (the streaming archive loader `FUN_800542C8` calls it with the
pool pointer, the embedded TMD, and the battle-slot index):

```
+0x000  15 x [16 BGR555 colours]   ; CLUT region (0x1E0 bytes; zero-padded for
                                   ;   monsters that use fewer than 15)
+0x1E0  4bpp indices               ; texture page, width x 256 texels, row-major
```

The loader uploads the CLUT region to VRAM `(0, 484 + slot)` (256 colours wide,
STP bit set on non-zero entries) and the page to `(slot*64 + 320, 256)`. The
page is **always 256 rows tall**; its width is **128 texels** (32 fb-units) for
most monsters or **256 texels** (64 fb-units) when the per-monster wide flag is
set - so `width_texels = (pool_len - 0x1E0) / 256 * 2`. A primitive selects its
palette by `cba & 0x3F` and samples the page at its per-vertex `(u, v)`; PSX
index 0 (colour `0x0000`) is transparent. The byte arithmetic is exact: Gimard
`0x1E0 + 128*256/2 = 0x41E0`, Tetsu `0x1E0 + 256*256/2 = 0x81E0`, both equal to
their pool sizes. (The on-disc CBA/TSB are nominal defaults the loader relocates
per slot, so the raw pool bytes do not appear verbatim in a battle VRAM dump -
the `FUN_80055468` layout is the ground truth; see
`ghidra/scripts/funcs/80055468.txt`.)

Parser: `legaia_asset::monster_archive::mesh(entry, id) -> Option<MonsterMesh>`
(returns the decoded block + the TMD/pool offsets); `MonsterMesh::texture()`
decodes the pool into `MonsterTexture { palettes, indices, width, height }`. CLI
`asset monster-archive --id N --obj <out>` exports the mesh as Wavefront OBJ and
`--texture-png <out>` bakes the texture page. WASM: the
`LegaiaViewer::monster_mesh_{positions,normals,indices,bounds,uvs,palette_index}`
and `monster_texture_{indices,palette_rgba,dims}` accessors feed the in-browser
WebGL viewer on the enemy-table site page, which textures the model with the
index→palette lookup the PSX GPU does in VRAM.

### Native renderer bridge (from-scratch engine)

The from-scratch engine renders the decoded monster directly through its standard
PSX-VRAM texture path rather than the site's index→palette shortcut.
`MonsterMesh::battle_render_mesh(slot, &mut vram)` reproduces the loader's
per-slot relocation: it writes the CLUT region to VRAM row `484 + slot` - with
the loader's STP bit on every non-zero entry (`battle_clut_region`), without
which the near-camera ghost and the defeat fade, both semi-transparent draws,
blend nothing and draw the body solid - and the
4bpp page to `((5 + slot) * 64, 256)`, then rewrites every prim's CBA/TSB to
point at those regions (`relocate_cba` / `relocate_tsb`), keeping the
page-local UVs untouched. Because the on-disc CBA/TSB are nominal defaults the
loader relocates, this is what makes the textures resolve against the injected
VRAM. The CLUT region (`x < 240`) and the texture pages (`x >= 320`) never
overlap, so up to five monster slots coexist in one VRAM.

`World::battle_monster_slots()` reports the active enemies as
`(actor_index, monster_id, battle_slot)`; the engine itself never loads the
archive, so the host resolves each id to a `MonsterMesh`, injects it, and binds
the relocated mesh to the actor. `play-window` does this on each
`Field → Battle` transition (against a throwaway clone of the
field VRAM, restored on the way back) so the enemy is drawn, not a stand-in.

### Browser play-page battle render

The browser play page runs the same `Field → Battle` edge through
`legaia_web_viewer::play_battle_render` (`LegaiaRuntime::enter_battle_render`
/ `exit_battle_render`), reusing the shared kernels above rather than a
second implementation: the scene's battle-kind resource build
(`SceneLoadKind::Battle`) makes the stage dome + its textures resident, the
dome takes `drawn_objects_tmd` + the `MirrorXTable` second copy
(pre-appended via `VramMesh::append_scaled` so the page uploads one mesh),
the ground grid comes from `build_ground_grid` with the `DAT_80078C1C` far
colour, the flame atlas + per-slot monster injection + assembled party bands
land in a throwaway battle VRAM the page swaps in for the fight, and each
actor's idle / action / swing / art-bank clips are installed on the world so
the shared battle SM poses them (`pose_frame`, read back per frame through
`play_battle_actor_pose`). Actor draws compose the same live-facing yaw and
retail 4× world scale as the native window.

Host differences that remain, disclosed rather than approximated silently:
the floating damage numerals and the `N HIT` / `TOTAL` counter draw from the
font atlas on the page where the native window samples the retail 24x24 art
cells out of VRAM (the layout is the shared builder's on both; only the
glyph source differs), and the field move-VM stager parts are not resolved
while a battle is up. Everything else in this branch runs on both hosts
from one kernel: the phase-scripted camera (`battle_cam_script`), the
ground grid's per-draw depth cue, the battle-intro screen-prim emitter, the
mid-battle summon-creature spawn, and the per-tick VRAM re-stamps (facial
animation, the Stone CLUT recolour, the effect CLUT stage) - the page runs
the same three drains against its battle VRAM copy and re-uploads on
`play_battle_vram_take_dirty`.

### Weapon-trail afterimage streak

The trail a swinging weapon leaves is one semi-transparent `POLY_FT4` per emitter call (`FUN_801E1AB0`), and its two projection inputs are context words the action effect script's terminator writes: the billboard centre from `ctx[+0x1144]` and the half-width as `ctx[+0x6C6] - 0x200`. The terminator stages both in one block - `sw` of the move-power record pointer to `+0x1014`, `sh` of that record's `+0x04` to `+0x6C6`, then a four-slot loop writing phase `1` to `+0x24E + i` and the launch position to `+0x1144 + i*8` (`FUN_801DEA50`, `0x801DF284..0x801DF2E0`).

The launch position is **not** the bare actor position: retail re-seeds its stack pair from `actor[+0x34..+0x3B]` at the top of every record iteration and runs the scale + facing rotation on it before the terminator test, so the quad the seed loop copies out carries the terminator record's own placement.

Port: `engine-core::action_effect_script::MoveFxStreak` is the block (record id rather than pointer, one shared launch point rather than four identical copies), installed by the live per-frame walk in `World::step_actor_effect_script` and read back through `World::casting.move_fx_streak`. `engine-ui::streak_pass` projects it once per frame and hands the corners to the ported packet builder `afterimage::build_afterimage_quad`, whose jitter law, brightness band, UVs, CLUT (`0x7700 + trail id`) and texpage (`0x0027`) are unchanged. The native window appends the quads to its screen-space textured batch.

Two disclosed departures. The **projection** is the engine camera's, not the GTE's: `project_streak_corners_mvp` takes the screen-space gradient of the battle MVP and fans the corners out along the screen axes, which is the same operation `FUN_800195A8` performs in view space - but the engine's battle camera carries no GTE rotation/translation pair to feed the exact port (`billboard::project_billboard`). And retail links each packet at the projected billboard's own OT bucket, inside the scene; the engine's screen-space batch draws them over the actors instead of interleaved with them.

The chained-ribbon sibling `FUN_801E1D98` is wired through the same pass, and the dispatcher choice (`0x801E0CA0` vs `0x801E0CD0`) is decoded: the phase driver `FUN_801E09F8` walks the counter `ctx[+0x6C6]` down `DAT_1F800393 << 2` per frame and selects by value - party afterimage at `>= 0x281`, ribbon below `0x201` (nothing in the dead band), monster ribbon at every value (`0x801E0C64..0x801E0CE8`). Port: `streak_pass::streak_quads_scheduled` + `engine-core::MoveFxStreak::tick_counter`. See [`battle-action.md` § Arts presentation](#arts-presentation-slow-motion-and-after-image-ghosts).

**Reachability today.** The pass is wired into the native window's screen-FX
builder, but a live `--battle` fight emits **zero** quads. It is gated on
`World::active_move_fx_trail_texpage()`, which is set only when
`World::spawn_move_fx` stages a move-power record's Spawn prototypes - i.e.
when a *move* runs. A party basic Attack stages no move: the attack
resolution leaves the actor's `+0x1DF` action stream all-zero, so the attack
chain reads its terminator on the first byte and exits straight to recovery
without staging a swing (`0x0C..0x0F`) at all. Damage still lands - the live
loop applies it through its own strike path, not through the SM's strike
band - but until the action stream has a producer for the party attack, both
the swing clips and the streak that trails them stay unreached.

## Weapon trail builder (`FUN_8005112C` + `FUN_80048310` + `FUN_800485BC`)

The swept `POLY_G4` streak an ordinary arts swing leaves behind a party
character's blade (distinct from the mesh after-image ghosts, which only the
Super / Miracle starter dash gets).

**Trigger** (`FUN_8005112C`, called per party seat from the per-actor battle
draw tick `FUN_800480D8`): fires only while the committed action record's
`+0x77` clip-identity byte matches a per-character constant - Vahn `0x29`
(base object `0x0C`, tint `0x802040`), Noa `0x1E` (base `0x04`, `0x80FFC0`)
and `0x2A` (base `0x0A`, `0x208040`), Gala `0x64` (base `0x06`, `0x204080`) -
always with **3 control points** (the weapon bone chain `base..base+3`).

**Sweep** (`FUN_80048310`): saves the anim cursor `actor[+0x68]`, and up to 16
times re-decodes the pose at the current cursor (`FUN_8004998C`), copies the
control points' decoded object positions out of the pose pool
(`gp[0xa0c] + 0x6f4`, stride `0xC`) into a 16-step scratch, and rewinds the
cursor by `2 * record[+0x78]` - two display frames per step - stopping at the
clip start. With at least two captured steps it emits gouraud bands: segment 0
white -> `0x808080`, segment 1 `0x7F7F7F` -> black, then every segment `k` of
`n` with the trigger tint faded linearly (`rgb * (n-k)/n -> rgb * (n-k-1)/n`,
truncating division) - all semi-transparent, stacking additively.

**Band emitter** (`FUN_800485BC`, 275 instructions): per band, yaw-rotates the
two steps' local control points by `actor[+0x26]` against the sin/cos LUTs
(`_DAT_8007B81C` / `_DAT_8007B7F8`, a 12-bit angle **mask** into 4096-entry
`s16` 1.12 tables), adds the battle slot's world base
(`*(int*)(0x801C9370 + actor[+0x5A]*4) + 0x34/+0x38`), projects each vertex
through `FUN_800195A8`, and drops `0x3B808080` packets into the OT - a
**`POLY_G4`**: four-point gouraud, semi-transparent, *untextured*
(`0x808080` is a placeholder the per-vertex fill overwrites; vertices
`v0/v2` = the leading step's pair carrying the band's lead colour, `v1/v3`
trailing). Vertex products carry a `+0xFFF` bias when negative before the
`>> 12` (round-toward-zero), and the OT slot is the average of the four corner
depths with the same fixup.

**Port**: trigger table + sweep/band schedule `engine-vm::battle_trail`; the
projected band packets `engine-ui::battle_trail` (a gouraud `FlatQuad` through
the shared screen-prim pass, ABR 1); `World::battle_weapon_trail_draws`
samples the sweep off the pose-history ring (step `k` = the pose `2k` frames
ago, the retail rewind under a constant rate) bounded by the ring's per-frame
clip key. Both hosts project with their own battle camera and composite the
bands over the scene - the OT interleave with scene depth is the same
disclosed simplification as the move-FX streak.

### Move-FX streak ribbon (`FUN_801E1D98`)

The move-FX draw dispatcher has two 2D streak shapes and picks between them by call site: `0x801E0CA0` calls `FUN_801E1AB0`, the single-billboard afterimage; `0x801E0CD0` calls `FUN_801E1D98`, the chained ribbon. Both take the trail-texture id from the move-power record's `+0x0b` byte and both build the same kind of packet - a semi-transparent textured `POLY_FT4` (`0x2e808080`), texpage `0x27`, CLUT `0x7700 + trail_id`.

The ribbon starts from one `FUN_800195A8` billboard projection of the actor point - half-width `0x100`, half-height `0x200`, no in-plane spin, and no `+0x120` Y push (that push is the afterimage's, not shared). From the projected quad it derives two governing numbers:

- **Suppression.** If the projected top edge spans `0x41` px or more (`x1 - x0`, signed), the routine returns without linking anything. The packet it had already carved out of the frame arena is simply abandoned; there is no single-quad fallback.
- **Segment height.** The projected height `y2 - y0` is kept when it is at least `0x40`, otherwise `0x40` is substituted. That is a **floor**, not a cap - a tall billboard produces tall segments and therefore a shorter chain.

Every further segment reuses the previous segment's top edge as its own bottom edge, so the quads form one continuous strip, and the un-jittered baseline steps up by exactly one segment height per iteration. The walk stops when the baseline (sign-extended to 16 bits) is no longer greater than `-height`, i.e. once the strip has left the top of the screen.

The jitter law differs between the first segment and the rest, and the magnitudes are all shifts of the segment height `h`:

| Segment | `rand` draws | What each moves |
|---|---|---|
| Bottom (from the projection) | 7 | one shared `[-h/4, +h/4]` X wobble on the whole top edge, one shared `[-h/8, +h/8]` X wobble on the whole bottom edge, then four independent `[-h/8, +h/8]` Y wobbles in corner order, then the brightness band |
| Each further segment | 4 | one shared `[-h, +h]` X wobble carried across both new top corners, two independent `[-h/4, +h/4]` Y offsets off the stepped baseline, then the brightness band |

Because the X wobble is shared inside an edge, the strip keeps its width and snakes sideways rather than shearing. The brightness band is `(rand & 3) << 5`, selecting one of four `0x20`-wide texture sub-columns; the quad then samples `band ..= band|0x1f` horizontally and `0 ..= 0x3f` vertically, assigned `TL, TR, BL, BR`. That corner assignment is **mirrored relative to `FUN_801E1AB0`**, which puts the `|0x1f` edge on corners 0 and 1 - folding the two UV builders together would flip the texture on one of them.

Retail links every segment at the **same** OT bucket, the depth `FUN_800195A8` returned for the bottom billboard, so the strip is depth-flat.

Ported as `legaia_engine_ui::afterimage::build_streak_ribbon` (injected rng, unit-tested); projection is `project_ribbon_corners`, and arena allocation plus OT linking stay on the retail-renderer side that the port replaces.


### How the tint words reach the pixel

The tint pass `FUN_8004A908` packs the actor's `+0x04` lanes (`>> 2`) into
the render node's `+0x74` colour word and copies `+0x0C` into the node's
`+0x78` whenever it is non-zero (`0x8004AA24..0x8004AA70`; with `+0x0C == 0`
the pass instead derives `+0x78` from the transformed depth and dims the
lanes by `radius / depth` - the distance-dimming branch). The draw pass
`FUN_80048A08` then stages the two words as the GTE far colour and `IR0`
(`gp[0x9D8]` / `gp[0x9DC]`, `0x80048BEC..0x80048C00`) for the actor's
prims. So the prim's **modulation** colour becomes
`baked + (tint - baked) * blend / 0x1000`, and the GPU still multiplies the
texel through it (`texel * colour / 128`): a hit is the actor's own texture
pushed toward the element colour, never a flat silhouette. `IR0` is loaded
bare, so the item / spirit cue-group flash's `0x2000` extrapolates past the
far colour until the DPCS output clamp bounds it.

Retail capture: `battle_gimard_tail_fire_a` / `_b` (Tail Fire striking
Vahn) hold `+0x21F = 1`, `+0x0C = 0x1000` and a red `+0x04` word eight
lane-units apart between the two frames - the arm-0 ease at `1 * 8` per
frame - and the struck Vahn reads red `160..248` over green / blue `8..80`
across his texture. The impact table's five words are red, two blues, a
violet and white (`0x801F53D4`, parsed by
`legaia_asset::move_power::parse_impact_effect_table`). A colour word of
`0` is the summon-hide's "not drawn" value (`FUN_800480D8`'s word-zero
arm), not a black tint.

Both hosts render that law through the per-draw depth-cue seam, one rule for
every writer, and the rule is the whole tint pass rather than only its blend
arm: `World::battle_actor_draw_plan` runs `engine-vm::battle_actor_tint` (the
port of `FUN_8004A908`) and the draw tick `engine-vm::battle_actor_tick`
(`FUN_800480D8`) per body per frame, and both hosts take the far colour and
`IR0` from it and skip the bodies it does not draw. The capture / defeat fade
(`+0x21C == 2`, arm 2) additionally ORs `0x81000000` into the node's mode
word so the fading actor draws additive; neither host has a per-draw blend
override yet, so that state is left un-cued rather than drawn as an opaque
black silhouette, and the body drops out once its lanes reach zero.

#### The distance fade

With no blend running the pass still writes both words. The view depth `a3 =
node[+0x34] / 16` (the `MVMVA` of the node position, `FUN_8003D344`) is set
against `a2 = radius / 2`, the radius being `*(actor[+0x22C]) + 0x58`: `640`
on the party seats and the record size class `<< 5` on a monster. A near body
(`a3 < a2`) takes its lanes at weight `a3 * 4`; a far one takes each lane
scaled by `a2 / a3` (floored at `4`) at weight `3 * (2*a3 - a2)`, saturated at
`0x1000`, so it is pushed toward a darker copy of itself. On the thirteen
outdoor stages (`DAT_8007BDA8`, the `DAT_80078C1C` table) a grey result is
complemented and its weight divided by eight - the far body brightens a
little instead. Then the `+0x16E` status colours (`0x1` -> `0xFF2020`, `0x2`
-> `0xFF0420`, `0x380` -> `0xF020F0`, weight `0x800`), bit 26 of the word, and
the `+0x226` additive fade. The view depth itself is one row of the battle
camera: `R * (4p - 4*focus) + tr` with `R = Rx(pitch) * Ry(yaw)` over the
camera trio `0x8007B790` and translation `0x800840B8`
(`battle_cam_script::battle_view_depth`), which reproduces the stored `+0x34`
of 296 of 379 captured bodies to within two units (341 within 32; the rest
read as states where the camera or the body moved after the draw).

Measured over the 97 catalogued battle states: recomputing the pass from each
seated actor's fields reproduces the stored `+0x74` / `+0x78` exactly for 258
of 266 drawn bodies, 54 of them on the distance-fade arm (e.g. the Tetsu
command menu's monster at depth `9098`: colour `0x48` a channel at weight
`0x990`). The eight that differ are frames where a later writer touched the
node after the draw.

The cursor-dim state (`+0x21C == 0xC8`, the target cursor's non-pointed
monsters) is its own arm: colour `0x010101` at weight `0x1000` unless the
seat's formation cell (`0x8007BD09 + seat`) holds monster `0xA8`, which reads
as a black silhouette. No catalogued state holds that flag, so both hosts keep
their own cursor cue for the two cursor flags until one does.

#### The near-camera ghost pass (`FUN_8004DC68`)

The tint pass's colour word takes its top byte from the pool actor's `+0x8`
word (`0x8004AA44..0x8004AA50`), and that byte is the draw's mode: bit 31
raises semi-transparency, bits 24/25 pick the blend rule. One routine owns
the bits that matter, `FUN_8004DC68`, called once per battle frame by the
frame driver `FUN_80046A20` (`jal` at `0x80047124`, between the camera update
and the tint SM `FUN_80050120`). It only ever sets or clears `0x83000000` -
mode `3`, `B + F/4`, a faint ghost of the body (see
`ghidra/scripts/funcs/8004dc68.txt`):

- **Near the camera.** It forms a point on the view axis - the focus trio
  `0x80089118` / `0x80089120` pulled back by `dist * 25 / 128` along the yaw
  `0x8007B792`, `dist` being the eye depth `0x800840C0` - and for each of
  pool slots `0..=6` measures the planar distance to it (the sum
  `|dx| |sin b| + |dz| |cos b|` over the `FUN_80019B28` bearing `b`). Within
  `dist / 4` a body ghosts - unless it is the acting actor, the command-flow
  byte `ctx[+6]` is below `0x1F` or one of `0x32` / `0x6E` / `0xFE`, the
  action state is below `0x0B`, or it is the actor's target while `ctx[+6]` is
  `0x64` / `0x65` (or `0xFF` with the actor's category in `1..=3`).
- **Whole-side scopes** clear a side: target byte `8` keeps the party's
  bits, `9` keeps the monsters', anything above clears both.
- **Nothing ghosts** during a run (category `5`), on a pre-emptive round
  (`ctx[+0x290] == 1`), in action state `0x0B`, or after the battle ends
  with `ctx[+0x26B]` raised.
- **Magic casts** (action states `0x28..=0x2E`, `MagicCastBegin` through
  `MagicExit`) ghost the caster's whole side, then clear the caster and its
  target: the allies fade while the spell plays and the one it lands on
  stays solid.

The action SM's `0x5A` end-of-action sweep clears the bits on every slot
(`0x801E6478`), and `FUN_801D5854`'s out-of-range guard does the same through
`FUN_801DB9C4`. Recomputing the pass from RAM over the catalogued battle
states reproduces the stored bits on 277 of 279 seated slots. The two misses
are bits set where the pass would clear them (action states `0x1E` and
`0x35`, flow `0xFF`), and they are not the driver's gate: the driver skips
the call while `gp[+0x330]` is non-negative (`lb` at `0x800470EC`), and that
byte - `0x8007B648`, the battle-load stage `FUN_80046A20` hands to the loader
`FUN_80052770` while it is below `0x80` (`0x80046EEC..0x80046F08`) - reads
`0xFF` in 59 of the 60 battle-mode (`0x15`) mednafen library states and `0x84` in the other,
negative in every one, so the pass ran on each of those frames.

`ctx[+0x26B]` is the battle's side-band stream request: `FUN_80055B4C`
stores `a0 + 1` there (`0x80055B58`) - the victory hook's win-pose archive
and the summon stagers' streams - and the stream tick `FUN_801F17F8` clears
it once the stream lands (`0x801F19D8`). The pass reads it only with the
battle-end signal `0xFE` up, and there the request is the win-pose archive,
which the results sequencer also waits on (`0x8004E5C0`). It reads `0` on
the one results-frame state (`noa_levelup_banner`). The command-flow byte
`ctx[+6]` reads `0x1E`, `0x28` and `0x14` in the library's command-band and
round-start states, and `0xFF` in every state of a running action.

An earlier reading here and in `port-catalog-ignore.toml` called the routine
a "target-highlight pass" measuring distance from the **acting actor**. The
reference point is the camera, not an actor, and the effect is translucency,
not dimming.

**Engine.** `engine-vm::battle_action::camera_ghost_pass` is the kernel;
`World::tick_battle_camera_ghost` runs it every frame after the battle camera
tick (slots converted from the engine's compacted seating to retail's fixed
pool slots) and keeps the word in `BattleActor::flag_word`, which
`battle_actor_draw_plan` hands the tint pass as its top byte
(`BattleActorDrawPlan::semi_mode`). `ctx[+6]` is the engine's flow mirror
`BattleFlowState` (the selection band byte for byte) while the command band
runs, `0xFF` while the action SM owns the round and `0x14` before the first
round executes; retail's one-frame `0xFE` hand-off has no engine frame.
`ctx[+0x26B]` follows the measured span - the engine streams nothing: on
`rim_elm_gimard_victory` the request rises with the battle-end signal (v322)
and clears 28 vsyncs later (v350), after which the phase walk's own two CD
waits run the rest of the 80-vsync load hold (`autorun_victory_timeline.lua`,
columns `req26b` / `prog26c`). So bodies near the camera ghost again for the
last 52 vsyncs of the hold, and the engine raises the byte for exactly its
first 28 (`VictorySequence::side_band_request_up`). The `gp[+0x330]` gate has no
engine twin because the engine has no load stage. Both hosts draw the ghost:
`engine-core::battle_body_blend` ORs the word's ABE / ABR into the body's TSB
words, as `FUN_80043390` does into its packets.

**The pose actor is the acting slot.** The battle-over close-up sits right
behind the posing character, well inside `dist / 4`, and retail keeps that body
solid only because the pass's acting slot `ctx[+0x13]` is the same field the
results sequencer frames (`noa_levelup_banner`: `ctx[+0x13] == 0`, seat 0's
`+0x8` clear and filling the foreground opaque, the dead monster in slot 3 the
only body near `P`). The engine's acting mirror keeps the fight's last actor,
so while a non-escape `VictorySequence` is armed `tick_battle_camera_ghost`
feeds the pass the pose actor instead; otherwise a win landed by another seat
fades the framed leader to a screen-filling `B + F/4` ghost. Other party
members near the close-up still ghost, as retail's pass would. The field's
camera-occlusion fade is a separate mechanism and never arms in battle
(`field_occlusion::fade_armed` requires `SceneMode::Field`).

## Arts presentation: slow-motion and after-image ghosts

The two channels that make a retail art read as an *event* - the battle clock
slowing while the art plays, and the mesh trail behind a Super / Miracle
dash - are one per-actor byte and one per-actor draw walk, both SCUS-resident.

### The animation-rate byte `actor[+0x21D]`

`+0x21D` is the per-actor **animation-rate scalar**, normal `8`. The anim
tick `FUN_80047430` advances each render node's 12.4 anim cursor by
`(DAT_1F800393 * actor[+0x21D] * clip[+0x78]) >> 1` per game frame - `>> 2`
only for a **Slowed** actor on idle (status `+0x16E & 0x1000` at
`0x800476E0`, then `+0x1D9 == 0` at `0x800476EC`; `0x800476D8..0x80047764`),
so an ordinary idle loop advances as fast as any clip - and `4` is half
speed, `2` quarter speed and `0` a freeze, and every animation-driven edge
(swing pacing, root motion, the strike loop's per-clip gate) stretches with
it. Battle seating (`FUN_800513F0`, `0x80051608`/`0x80051888`) seeds it from
the scratchpad speed scalar `0x1F80037D`. The strike loop multiplies the same
byte into its per-frame impact drift, which is where the earlier "impact-step
magnitude" name came from - that is one consumer, not the field.

The same tick decides when a staged clip replaces the playing one, and an
idle or walk loop is no exception. Mid-clip, `+0x1DC` bit 0 commits at once
and bit 1 once the cursor frame is past the entry's gate frame by more than
two (`0x800478EC..0x80047948`), both refused by the entry's `+0x76` lock;
otherwise the staged byte waits for the natural end (`0x80047B54`), which
calls `FUN_8004AD80` whatever is staged - a clip left staged behind itself
re-commits from its first frame, which is how a loop loops. So the strike
loop's first swing, staged under bit 1 over the idle `0x19`'s arrival
committed under bit 0 (`0x801E35C0`), waits for idle frame 3
(`player_steal_skeleton_pre`: idle cursor `0x20`, `0x0F` staged), and every
loop cycle of the acting actor re-zeroes the camera's ramp / accumulator /
latch like any other commit (`0x8004BF50..0x8004BF78`). Port:
`World::commit_staged_battle_anim` (the looping-clip gate) and the
natural-end re-commit in `World::tick_battle_animations`.

The writers are the anim-commit `FUN_8004AD80`'s arms, all on the **party**
ladder (the monster path branches clear at `0x8004B6F4`):

| Trigger (raw staged id `+0x1DA`) | Effect | Site |
|---|---|---|
| any commit, rate `!= 8` | rate = `4` | `0x8004B080..0x8004B090` |
| `0x1A` SpecialStarter | all slots `0`, acting actor `2` | `0x8004B728..0x8004B750` |
| `>= 0x1B` art constant | all slots `2` if `ctx[+0x243]` set, else `4` | `0x8004BB78..0x8004BBA8` |

So a Super / Miracle starter is a freeze-frame with only the dashing actor
moving at quarter speed; each art strike then plays the whole battle at half
speed; direction swings (`0x0C..=0x0F`) never slow anything. The same `0x1A`
arm raises the `ARTS!!` banner byte `ctx[+0x28B]` (from the queue-builder's
side-array mark at `0x801F6990` / the Miracle marker; default `2`), zeroes
the banner clock `+0x28C`, and queues the per-character arts shout
(`FUN_8004FCC8` ids `0x101/0x111/0x121` + per-follow-up variants) - which
pins the `+0x28B` writer [`flash_ramp`](battle-hud.md#arts-announcement-banner-fun_801e2524--fun_801e2650)
was still missing. The restore is `FUN_801E93C8`, `jal`ed from the shared
tail at `0x801E5F64` that the Done arm falls into and that `0x1E`, `0x1F` and
`0x20` jump to on nearly every pass (`0x801E39AC`, `0x801E3A68`, `0x801E3A80`,
`0x801E3AF8`, `0x801E3B18`, `0x801E56C8`, `0x801E55A0..0x801E5658`): once the
acting actor's materialised art clip has ended (party: `+0x1D9 < 0x10`;
monster: committed record flag `+0x87 == 0`) every slot returns to `8` and
`ctx[+0x243]` clears. So the battle is back at full speed the pass the last
art clip ends, and a monster that art killed plays the rest of its
knockdown at normal speed rather than waiting for the Done band.

**Port.** Kernel `legaia_engine_vm::battle_anim_rate`
(`BattleActor::anim_rate`, default 8); commit arms in `engine-core`'s
`commit_staged_battle_anim`; the rate-scaled advance in
`battle_anim::MonsterAnimPlayer::tick_rated`; the restore was already ported
as `battle_gauge_rearm::restore_anim_rates` (whose old "arts-gauge arm width"
reading of `+0x21D` is superseded).

### The after-image ghost walk (`FUN_80049348`)

The anim tick keeps four 32-deep per-actor **history rings**, shifted one
slot per frame with slot 0 taking the live values
(`0x80047E58..0x80048060`): position (`+0x4C`, 8-byte stride), anim cursor
(`+0x17A`), committed clip record (`+0x234`) and committed anim id
(`+0x1FB`; party = `+0x1D9`, monster = `clip_tag + 0x10`, or `0x11` when the
record flag `+0x87` is set). The per-actor draw tick `FUN_800480D8` then runs
`FUN_80049348`, which draws **two ghosts** of the actor's own mesh from the
ring:

- **Spacing** `step = 8 / actor[+0x21D]` (monster seats double it), depths
  `step` and `2*step` - the trail stretches exactly when slow-motion drops
  the rate (quarter speed → depths 4 and 8).
- **Gate**: ring id `> 0x10` (`sltiu 0x11` at `0x80049460`). The ring id
  is stamped by the anim tick from the **committed record's own bytes**,
  never from the actor's staging state: a party seat copies the committed
  dynamic slot `+0x1D9` (`0x80047FCC`), a monster seat stamps
  `record[+0x77] + 0x10`, or `0x11` when the record's `+0x87` solo byte is
  `1` (`0x80048044..0x80048060`). A party art materialises at dynamic slot
  `0x10` except the `0x1A` / re-staged-`0x10` commits, which land at `0x11` -
  so party mesh ghosts belong to the **SpecialStarter dash** (ordinary art
  swings leave the 2D weapon-trail streak instead; a **chained** art is a
  re-staged `0x10` and lands at `0x11` too - the `battle_melee_hit_spark`
  capture holds Vahn at `+0x1D9 = +0x1FB = 0x11` mid-Somersault with his
  ghosts drawn, and Gimard at ring id `0x10`, none). A monster ghosts only on
  a clip whose record carries a non-zero `+0x77` or `+0x87 == 1` - on the
  disc that is the solo / special entries (Tetsu's tag-`0x0F` special
  carries both; Gobu Gobu has none), and **no** idle entry qualifies, so an
  idle or walking monster never ghosts. The earlier "any non-idle clip tag"
  reading is falsified ([re-do-not-re-walk.md](../reference/re-do-not-re-walk.md#battle--arts--level-up));
  the disc census is `crates/engine-core/tests/battle_afterimage_gate_real.rs`.
- **Colour**: flat additive. The draw wrapper `FUN_80043390` decodes the
  colour word's mode byte (`0x85`): bit `0x80` → the GP0 ABE bit, low bits →
  ABR mode 1 (B + F), bit `0x04` → the flat-colour prim bank with the GTE far
  colour as the RGB. Base per character - SCUS word table `0x80076908`
  (Vahn red `0x60/0x30/0x30`, Noa green, Gala blue; monsters `0x80076914`
  olive `0x50/0x50/0x30`) - stepped down `0x101010` per drawn ghost. The
  ghost's OT depth is pushed `0x50` buckets deeper than the live body
  (`FUN_80048A08`, the `+0x10` bit-`0x800000` arms).

**Port.** Kernel `engine-core::battle_afterimage` (schedule / gate / colour
law), history ring on `Actor::battle_pose_history`, plan API
`World::battle_ghost_draws`. The native window draws each ghost as a
flat-coloured additive posed mesh on the colour pipeline; the browser play
page uploads per-actor ghost mesh copies on its flat + additive path and
poses them from `play_battle_actor_ghost_pose`. Both hosts must carry
retail's deeper-bucket ordering explicitly, because their additive passes
deliberately pass on **equal** depth (coplanar decals) - so a ghost pose
coincident with the live body would otherwise blend over every body
fragment and wash the whole mesh additive (the "monster glows yellow"
defect). The native window scales each ghost uniformly about the eye until it sits
past the body (`battle_afterimage::ghost_eye_push_scale` in the redraw pass); the play page draws ghost
placements with a strictly-nearer depth test (`strictDepth` → `gl.LESS`).
Either way the live body hides the overlap and only the separated trail
shows, which is what retail's `+0x50`-bucket push produces on the console.

### The streak emitter schedule (decoded dispatcher)

The choice between the two 2D trail emitters -
`FUN_801E1AB0` single-quad afterimage vs `FUN_801E1D98` chained ribbon - is
not per-move data: the phase driver `FUN_801E09F8` walks the streak counter
`ctx[+0x6C6]` down by `DAT_1F800393 << 2` per frame (`0x801E0C1C..0x801E0C40`,
floored at 0) and selects by its value (`0x801E0C64..0x801E0CE8`): a party
acting actor draws the afterimage while the counter is `>= 0x281` (its
half-width is `counter - 0x200`, so the quad shrinks), nothing through
`0x280..0x201`, and the ribbon below `0x201`; a monster acting actor draws
the ribbon at every value. Port:
`engine-ui::streak_pass::streak_quads_scheduled` +
`MoveFxStreak::tick_counter`.

The ribbon has a second caller outside that dispatcher: the per-clip pass of
`FUN_8004CE2C` (SCUS), whose Gala tag-`0x67` arm (`0x8004D1E8..0x8004D248`)
calls `FUN_801E1D98(&target[+0x3C], 0xC)` on every frame the committed clip's
cursor sits in `0xB0..=0xF0` - `addiu a0,s1,0x3c` in the branch delay slot at
`0x8004D220`, `li a1,0xc` at `0x8004D224`. The anchor is the target's seat
vector (`+0x3C..+0x43`, copied verbatim from the spawn node by the battle
setup at `0x8005158C..0x80051598`) rather than the launch point `ctx[+0x1144]`,
and the trail id is the literal `0xC` rather than the move record's `+0x0B`.
Port: the impact pass stores the frame's source in `BattleState::clip_ribbon`
and both hosts draw it through `streak_pass::clip_ribbon_quads`.
