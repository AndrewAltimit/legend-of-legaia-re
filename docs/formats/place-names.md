# Place names

A place name - "Rim Elm", "Sol Tower", "Ancient Wind Cave" - is shown in three
different places in the game, and **each reads its own copy off the disc**.
There is no single name table. Editing one carrier changes exactly one display,
which is why a rename that only rewrites the well-known `SCUS_942.54` table
leaves the world map and the entry banner still saying the old name.

Parsers: `legaia_asset::worldmap_menu` (site 1),
`legaia_asset::place_names` (sites 2 and 3). Modding path:
`legaia_patcher::location_name` + `legaia-patcher --rename-location`, which
edits all three together. Disc pins in `crates/patcher/tests/place_names_real.rs`.

## Contents

- [The three carriers](#the-three-carriers)
- [Site 2 - the world-map location table](#site-2---the-world-map-location-table)
  - [Record layout](#record-layout) · [The label pass](#the-label-pass)
- [Site 3 - the scene display name](#site-3---the-scene-display-name)
  - [The latch is the save screen's, not the banner's](#the-latch-is-the-save-screens-not-the-banners) · [How it was pinned](#how-it-was-pinned) · [What else reads it](#what-else-reads-it)
- [Editing notes](#editing-notes)
- [Confidence](#confidence)

## The three carriers

| Site | What it draws | Carrier | Shape |
|---|---|---|---|
| 1 | the quick-travel / Door-of-Wind destination list | `SCUS_942.54` `0x80073B18` | 16 fixed `0x20`-byte NUL-padded cells ([`worldmap-menu`](#see-also)) |
| 2 | the labels drawn over the world map at each place's map position | the trailer of every **kingdom** MAN (`map01` / `map02` / `map03`) | `[u8 count][count x 0x20 record]` |
| 3 | the banner on entering a scene (a text balloon the MAN loader spawns), and the save-screen location row | each scene MAN's **section 2** | bare `strlen + 1` NUL-terminated string |

Sites 2 and 3 both live in a scene MAN and are reached through the section
chain [`man_section`](../../crates/asset/src/man_section.rs) already walks - no
byte hunt is needed for either.

The three name spaces are **not** the same set. Site 1 holds 16 names; site 2
holds 29, naming 14 places the quick-travel menu has no room for (Hunter's
Spring, Mt. Rikuroa, Snowdrift Cave, West/East Voz Forest, Zeto's Dungeon,
Shadow Gate, Mt. Letona, Dohati's Castle, Sol Tower, Nivora Ravine, Mt. Dhini,
Jette's Fortress, Snow Area). Site 3 holds one name per scene bundle, so a
multi-scene place repeats its name - Sol Tower across 13 bundles, Rim Elm
across 7, Bio Castle across 5.

Near-miss names are genuinely distinct strings and stay that way: site 1's
"Sol" is not site 2's "Sol Tower", and "Conkram" is not "Conkram (Past)".

## Site 2 - the world-map location table

MAN section 5 is the chain terminator (`length == 0`) in every scene bundle, so
its "body" is whatever trails the chain. In the three kingdom MANs that trailing
data is the location table, and the pointer `DAT_80073EE0` that `FUN_8003AEB0`
installs points at its count byte.

All three kingdom MANs carry a **byte-identical copy of the whole 29-record
table** - each is filtered by `region` at draw time - so a rename must edit
every kingdom MAN, not just the one whose continent the place sits on.

### Record layout

```text
DAT_80073EE0[0]                 u8    record count (retail 29)
DAT_80073EE0[1 + i*0x20]:
  +0x00  u8    region           0 = Drake, 1 = Sebucus, 2 = Karisto
  +0x01  u8    map x            world coordinate = x << 7
  +0x02  u8    map y            world coordinate = y << 7
  +0x03  u16   discovery flag   index queried through FUN_8003CE64
  +0x05  3     reserved         zero across the retail corpus
  +0x08  0x18  name             NUL-padded ASCII
```

The discovery flags run `0x0484..0x04A0`. They are not unique: the two element
caves (records 9 and 10, Ancient Wind Cave and Ancient Water Cave) share flag
`0x048E` and are told apart by `region`.

### The label pass

The consumer is the per-frame loop at `0x801CEBB6..0x801CEC30` in the world-map
band. Per record:

```c
if (record.region == current_kingdom &&
    (FUN_8003CE64(record.discovery_flag) || _DAT_8007B868 /* debug: show all */)) {
    pos = (record.map_x << 7, record.map_y << 7);
    screen = FUN_8003D368(pos);              // world -> screen projection
    FUN_80036888(&record.name, 0, 0, screen.x, screen.y);   // draw the label
    width = FUN_80035F04(&record.name);      // measure it
    FUN_8002C69C(screen.x, screen.y, width, 8);             // underline
}
```

So the name is drawn **at the place's own map position**, gated on having
discovered it. `see ghidra/scripts/funcs/overlay_world_map_top_801ce9c4.txt`.

## Site 3 - the scene display name

MAN section 2's body is the scene's display name. `FUN_8003AEB0` installs the
body pointer into `_DAT_801C6EA0` and, as the last act of the same load, puts
the name on screen itself (`0x8003BB40..0x8003BBDC`, `see
ghidra/scripts/funcs/8003aeb0.txt`):

```asm
8003BB44  lw    s1,0x6ea0(v0)          ; s1 = _DAT_801C6EA0 (the name)
8003BB4C  lbu   v0,0x0(s1)
8003BB54  beq   v0,zero,0x8003BBC4     ; empty name -> no banner
8003BB5C  lbu   v0,0x1618(s0)          ; 0x80085758 byte 0
8003BB64  andi  v0,v0,0x20             ; system flag 2
8003BB74  beq   v1,zero,0x8003BBC4     ; flag clear -> no banner
8003BB80  jal   0x80020de0             ; spawn from template 0x8007431C
8003BB98  sw    a0,0x90(s0)            ; +0x90 = the name
8003BB9C  sw    zero,0x94(s0)          ; +0x94 = 0 (no parent link)
          ...                          ; +0x9C = 0x78, x centred, y = 0xB4
8003BBD4  andi  v1,v1,0xdf             ; clear system flag 2 - on every path
```

So the entry banner is not a panel of its own. It is the ordinary `4C E1`
text balloon - template `0x8007431C`, whose handler word is `0x801DA7F0` -
with the spawner `FUN_8003C764` inlined, one difference apart: the parent-link
word `+0x94` is stored zero. With no link, the balloon's handshake reads a new
engagement as its cue to end early, so talking to someone during the banner
dismisses it. See [`cutscene.md`](../subsystems/cutscene.md) and
`engine-core::text_balloon` for the balloon itself.

The gate is **system flag `2`** (`0x80085758` bit `0x20`), a one-shot the
loader clears whether or not a banner spawned. Nothing sets it with a literal
index in the executable or the field overlay; its writers are the generic flag
helpers, driven by scripts. A disc-wide walk of every scene MAN's partitions
finds its clean `SET` sites in the three kingdom MANs (`map01` / `map02` /
`map03`) and in `koroutx2` - leaving the overworld for a place is what
announces the place.

**The body is not padded**: it is exactly `strlen + 1` bytes, so a longer name
needs the section resized. See [editing notes](#editing-notes).

The engine port is `engine-core::place_name_banner`, called from
`SceneHost::load_scene` right after the MAN loader's actor work; both hosts
draw the result through the shared balloon builders
(`engine-ui::text_balloon_box`). Disc pin:
`crates/engine-core/tests/place_name_banner_disc.rs`.

### The latch is the save screen's, not the banner's

The field overlay's fill-fade actor `FUN_801EE5D4` - and the two sibling
state machines whose latching stores sit at `0x801EAC7C` / `0x801EEAE4` - also
read `_DAT_801C6EA0`, and this page used to say they "open the banner panel
with it". They do not. The panel script they
run, `0x801F32B4`, is a single `op 5` record (close every panel) followed by
the terminator, read out of the PROT 0897 image at its `0x801CE818` base. What
those actors do with the name is copy it into `_DAT_8007B44C`, in the delay
slot of that script call:

```asm
801EE620  lui   $a0, 0x801f
801EE624  addiu $a0, $a0, 0x32b4      ; panel script: close all panels
801EE62C  lw    $v1, 0x6ea0($v0)      ; v1 = _DAT_801C6EA0
801EE634  jal   0x801e9b3c            ; panel command-script interpreter
801EE638  sw    $v1, -0x4bb4($v0)     ; _DAT_8007B44C = the name (delay slot)
```

and every reader of `_DAT_8007B44C` on the disc is the save screen (below).

### How it was pinned

The loader's banner tail is read off its disassembly. The live check predates
that read: breaking on the glyph renderer `FUN_80036888` across an
overworld-into-town transition captures the banner draw arriving with
`a0 == _DAT_801C6EA0`, spelling the scene's section-2 name - the balloon
handler draws its `+0x90` text through that leaf. Probe:
`scripts/pcsx-redux/autorun_location_banner_source.lua`.

### What else reads it

The menu overlay reads the latched `_DAT_8007B44C` twice, and both are the
save screen rather than the field:

- `0x801E1D9C` - draws it as the save-slot **location row**.
- `0x801E1A28` - `FUN_8001A8B0(0x80084340, name, 0x24)`, the copy into the save
  block's location field (SC block `+0x200`; see
  [`save-screen.md`](../subsystems/save-screen.md)).

That is why `0x80084340` holds the *last scene entered* rather than the current
one in a save state captured mid-scene: the buffer is filled at save time from
the latch, not per frame.

## Editing notes

- **Site 1** is a same-size overwrite of a 32-byte cell, zero-padded.
- **Site 2** is a same-size overwrite of a record's fixed 24-byte name field,
  then an LZS re-pack of the kingdom MAN.
- **Site 3** resizes the section when the name length changes. That needs no
  relocation work: the partition record-offset tables and the header's
  `u24_at_28` both address the region *before* section 0, and sections 3..5 are
  reached by walking the chain (`next = section + 3 + length`), so rewriting
  section 2's length prefix relocates every later section - including the site-2
  trailer - correctly by construction. What does have to move is the MAN's
  **decompressed size**, which is stored only in the scene bundle's descriptor
  word: rewrite it with `scene_asset_table::encode_size_word` alongside the
  re-packed stream.
- The **shared cap is 23 characters**, the tightest of the three (site 2's
  24-byte field minus its NUL). Retail's longest name, "Zora's Floating
  Castle", is 22.
- Every scene MAN is the last asset in its bundle, so the re-packed stream can
  grow into the entry's sector padding; the smallest headroom in the corpus is
  ~430 bytes. Clamp the budget to the entry's **true footprint** rather than to
  a `read_entry` length, which can over-read into the next entry (see
  [`prot.md`](prot.md)).
- Two count-5 scene bundles carry a MAN that the strict
  `scene_asset_table::detect` count allow-list (6/7) excludes - one of them
  `bubu1`, half of Buma. Reach them with `lenient_descriptor_walk`, which is
  what the runtime walker `FUN_80020224` does anyway, and confirm the find
  downstream (the stream must LZS-decode to its declared size, walk as a MAN,
  and yield a printable name).
- Ten scene MANs still carry an **untranslated Shift-JIS** name in section 2
  (the `ed*` ending scenes, plus `other7`), and seven carry an empty one. Both
  read as "no name" rather than being mangled into ASCII.

## Confidence

**Confirmed** - the record layout, the section indices, the count byte, and
both consumers are read off the disc and verified end-to-end: the three kingdom
tables are byte-identical row for row, a rename re-parses at every site, and
both the resized banner and the edited world-map table were read back out of
live RAM on a patched disc.

## See also

- [`worldmap-menu`](../subsystems/field-menu.md) / `legaia_asset::worldmap_menu` - site 1's table and its placement records.
- [`man_section`](../../crates/asset/src/man_section.rs) - the section chain both MAN-resident carriers hang off.
- [`world-map.md`](../subsystems/world-map.md) - the world-map band the label pass belongs to.
- [`randomizer.md`](../tooling/randomizer.md#location-names) - the `--rename-location` editor.
