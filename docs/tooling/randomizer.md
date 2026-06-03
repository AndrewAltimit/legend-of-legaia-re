# Randomizer / disc patcher

Track-1-adjacent tooling that edits gameplay data on a **user-supplied** retail
disc image: it shuffles monster item drops, random-encounter formations,
treasure-chest contents, per-monster steal items, scene-transition doors/exits,
intra-town (house / interior) doors, and the new game's starting items, and
writes the result back into the `.bin`. It does not touch the clean-room engine.

Crate: [`crates/rando`](../../crates/rando/README.md) (`legaia-rando`). It ships
only code — no game bytes — and every test that needs real data is disc-gated,
so CI runs without a disc.

## Why this needs three new capabilities

Most editable values live *inside* a Legaia LZS stream that the asset
dispatcher decompresses at load. Changing one is therefore
decompress → mutate → recompress → write-back, which needed three pieces the
preservation track never had (it only ever *read* the disc):

1. **An LZS encoder** — `legaia_lzs::compress`. The retail game ships only a
   decoder (`FUN_8001A55C`); there was no way to produce a stream it accepts.
   See [LZS compression](../formats/lzs.md).
2. **Mode 2/2352 sector write-back** — `legaia_iso::write`. Overwriting the
   2048-byte user payload of a CD sector also requires recomputing its 4-byte
   EDC and 276-byte P/Q ECC, or the sector reads as corrupt. See
   [PSX disc geometry](../formats/disc.md).
3. **A disc bridge** — `legaia_rando::disc::DiscPatcher`, which ties the editing
   primitives to the sector write-back through the PROT.DAT TOC.

## Editing model: same-size in place, except doors

Drops / encounters / chests / steals / **house doors** (the `0x23 MOVE_TO` tile
shuffle) overwrite bytes **in place** and never change a byte count, so no LBA,
PROT TOC, or ISO 9660 directory record ever moves. **Scene-transition doors are
the one exception**: a scene-transition destination carries its
target scene's name inline, so re-pointing a door at a differently-named scene
changes the record's byte length. That is made safe by the
[MAN relocation engine](../formats/man-relocation.md), which rebuilds the
decompressed MAN, fixes every internal offset the resize disturbs, and keeps the
*recompressed* stream within the asset's on-disc footprint (or skips the scene).
The disc image's total size never changes either way. For the same-size edits:

Every same-size edit overwrites bytes **in place** and never changes a byte
count, so no LBA, PROT TOC, or ISO 9660 directory record ever moves. That keeps the patch a
pure byte-overwrite (plus EDC/ECC recompute) with no risk of cascading offset
shifts. It works because the edit targets fit a fixed slot with slack:

- The monster `battle_data` archive (PROT entry 867) gives each monster a fixed
  `0x14000`-byte slot laid out `[u32 decompressed_size][LZS stream]`. The
  decoded record length is unchanged by a drop edit, and the original stream was
  produced by Sony's packer; our greedy packer is weaker but still fits the slot
  comfortably (`repack_slot` rejects the rare case where it would not). The slot
  is re-emitted zero-padded back to `0x14000`.

## In the browser

The same randomizer is also exposed client-side: `legaia_web_viewer::rom_patcher`
(`patch_rom`) compiles the crate to WASM, and the static site's
`tooling/rom-patcher.html` page lets a user supply their own disc, toggle the
drop / encounter / chest settings, and download a patched image — the disc bytes
never leave the browser. The CLI below is the scriptable / shareable-PPF path.

## CLI: `legaia-rando`

The top-level binary turns a disc + seed into a portable patch:

```bash
legaia-rando drops     --input DISC.bin                       # read-only: monster drops
legaia-rando chests    --input DISC.bin                       # read-only: chest contents
legaia-rando steals    --input DISC.bin                       # read-only: steal items
legaia-rando doors     --input DISC.bin                       # read-only: scene transitions
legaia-rando house-doors --input DISC.bin                     # read-only: intra-town MOVE_TO targets
legaia-rando starting-items --input DISC.bin                  # read-only: new-game starting bag
legaia-rando randomize --input DISC.bin --seed myrun --drops shuffle
legaia-rando randomize --input DISC.bin --seed 0xC0FFEE --drops random \
    --encounters shuffle --steals shuffle --doors shuffle --door-coupling coupled \
    --starting-items 3 --patch run.ppf --output patched.bin --manifest run.toml
legaia-rando randomize --input DISC.bin --seed wild --encounters random \
    --unused-enemies --chests random --unused-items                  # bring back unused content
legaia-rando verify    --input DISC.bin --patch run.ppf       # apply + sanity-check
```

`randomize` plans the run, applies it to an in-memory copy of the disc, diffs
the result against the original, and writes the changes as a **PPF 3.0** patch
(default `<input>.ppf`). `--output` also writes a full patched `.bin` for local
play. The seed is resolved from a number or a hashed string and always printed,
so a run reproduces exactly; the same seed yields a byte-identical patched image
and PPF. `--drops`, `--encounters`, `--chests`, `--steals`, and `--doors` each
take `shuffle` / `random` / `none`; `--door-coupling` is `coupled` (default,
bidirectional) or `decoupled` (one-way); `--starting-items N` seeds the new
game with `N` random consumables (0 = vanilla; capped at 5). `--unused-enemies`
and `--unused-items` re-introduce content the game ships but never surfaces (see
[Unused content](#unused-content) below).
`--dry-run` reports the plan without writing; `--manifest` writes a small TOML
record of the seed + options + change counts (no game bytes, safe to share). The
`verify` subcommand applies a PPF to a copy of the user's disc and confirms the
result still parses end to end — a recipient's check that a shared patch + seed
match their own disc.

The read-only `drops`, `chests`, `steals`, `doors`, and `starting-items` subcommands write nothing
— they decode the randomizable populations off the user's disc and print them
(item ids + names resolved from the disc's own SCUS table; chests + doors grouped
by scene via CDNAME). `chests` lists the exact 275-site treasure population the
chest randomizer reassigns, which is the natural place to audit for quest / key
items a run might want to keep static. `doors` lists every scene-transition exit
(home scene → destination + entry tile) — the 160-site door population.

### Keep-static items

A few chest items are progression / quest / key items the player needs in a
predictable place. The chest randomizer keeps a **curated default set** static
(`legaia_rando::items::DEFAULT_STATIC_CHEST_ITEMS`): Mary's Diary, Dark Stone,
Fertilizer, Weed Hammer, Spring Salts, Silver Compass, and the Old Rod. A chest
whose original item is in this set keeps that item, the id is excluded from the
shuffle multiset (so it can never move to another chest), and it is dropped from
the `random` fill pool (so it can't be duplicated into an unrelated chest).
Override with `--keep-static-items 0x9a,0x71,…` (decimal or `0xHH`), or pass an
empty value (`--keep-static-items ""`) to randomize every chest. The resolved set
is recorded in the run manifest.

Because an edit changes bytes *inside* an LZS stream, the whole touched stream
is re-packed, so the changed-byte count (and the PPF) is dominated by
re-compression churn, not by the gameplay delta — this is inherent to editing
compressed data, and every edit stays same-size. `--drops random` reads the SCUS
item table off the disc for the valid item pool; the other modes need no
external table.

### Random encounters

Formations live in the per-scene MAN asset (type `0x03`, descriptor index 2 of a
scene bundle), inside an LZS stream; each formation record is
`[3 reserved][u8 count 0..4][u8 ids...]` (see
[encounter records](../formats/encounter.md)). `apply::randomize_encounters`
walks every PROT entry, and for each scene bundle it locates the MAN
(`SceneEncounters::locate`, straight from the entry bytes — no engine
dependency), decompresses it, rewrites the formation monster ids
(`Shuffle` redistributes the existing ids, `Random` draws from the pool),
recompresses, and writes the stream back over the original (the LZS decoder
stops at the descriptor's decompressed size, so a same-or-shorter re-pack is
safe). The id pool is **per scene** — only ids the scene already uses — so every
swapped-in monster is one the scene loads; no missing model, no crash.

### Treasure chests

A chest gives its item via the field-VM **`GIVE_ITEM` opcode `0x39`**, encoded
`[0x39, item_id]` — the item id is a **single inline operand byte** in the
per-scene field-VM script bytecode, not a per-scene table. (Pinned in the
dispatcher `FUN_801DE840` case `0x39` at `0x801E0448`: inventory-window setup
`FUN_8004313C` then add-by-id `FUN_800421D4(item_id, 1)`, PC += 2. The standalone
`FUN_801D71F0` add-item copy is dead/uncalled. See
[script-vm.md](../subsystems/script-vm.md).) The give sites live in the MAN
partition-1 per-actor interaction scripts (a chest is an interactable actor).

`chest::give_item_sites` finds them with a **dialogue-skipping opcode-aware
walk** — it walks each partition-1 record's interaction script from its true
entry PC with the field-VM disassembler ([`legaia_asset::field_disasm`], moved
into Track 1 for exactly this reuse). A chest's give op almost always sits
**after** the inline dialogue that announces it ("There is a {item} in the
treasure chest!" → give → "{name} now has the {item}!"). That dialogue is a
stream of `0x1F`-lead glyph segments, not bytecode, so a decode error **at a
`0x1F` byte** is treated as a segment to skip (advance past `0x1F`, consume
glyphs to the terminating `0x00`, with `0xC?` top-nibble bytes as 2-byte
escapes per the dialog box-pack format), and decoding resumes — the
inter-segment control bytes (`0x24`/`0x25`/`0x48` Nop, `0x26` `JMP_REL`, `0x36`
`SCENE_FADE`, …) are genuine ops that stay in sync, so the walk reaches the
post-dialogue `0x39`. Any **other** decode error stops the walk, and each
record's walk is bounded to the next record's start offset, so it can never run
off into unrelated data and mis-read a `0x39` data byte as an op — never a naive
`0x39` byte scan. (An earlier walk stopped at the *first* `0x1F` instead of
skipping it, which silently missed the post-announcement give in roughly 85% of
sites — including every chest in a scene whose first interactable record opens
with dialogue, such as `keikoku`.) Multi-`0x39` runs are genuine multi-item
gifts (a 10× consumable chest, the fishing starter kit of a rod + several lures,
the Genesis-Tree Ra-Seru equipment sets), each `0x39 <id>` its own op.

**Display vs grant — the announcement names the item from a different byte.** A
chest's flavor text ("There is a {item} in the treasure chest!" / "{name} now has
the {item}!") renders the item *name* from a dialogue **item-name token** `0xC2
<id>`, which is a **separate byte** from the `0x39` give operand that actually
adds the item to the bag. Patching only the give operand grants the new item but
leaves the message reading the old one — verified in-game: the inventory receives
the new item while the chest still *says* the original (an `0xC2 <old_id>` token
sits resident in the loaded MAN right beside the patched `0x39 <new_id>`). Pinned
across the corpus: of every `0xC?` 2-byte dialogue escape in chest records, only
`0xC2`'s argument matches the give operand (the other escapes are character-name /
glyph controls), and 241 of 275 sites carry one (announcement + "now has"). So
`give_sites_and_display_tokens` recovers, per give site, the `0xC2` token offsets
in the same record whose id equals that site's give operand (routed to the
*nearest* give so multi-item-gift records map each token correctly), and
`SceneChests::set_site` rewrites the operand **and** those tokens together — flavor
text stays in sync with the grant. Sites whose dialogue doesn't name the item
(~34) simply have no token to sync.

Chest item ids are global inventory ids, so `apply::randomize_chests` reassigns
them **globally** across every site (`Shuffle` redistributes the existing
multiset, `Random` draws from the valid item pool), then recompresses each
touched MAN like the encounter path. A scene whose recompressed MAN overflows is
excluded from the shuffle pool entirely (determined iteratively), so its items
neither leave nor enter circulation and `Shuffle` preserves the global multiset
exactly. On the retail disc this is 275 give sites across 50 scenes (one scene,
too tight to re-pack, is skipped).

### Steal items (Evil God Icon)

What the player steals from a monster (Evil God Icon equipped) is a per-monster
entry in a **static `SCUS_942.54` table** at `DAT_80077828` — `[steal_chance_pct,
steal_item_id]` per 1-based monster id, item at `+id*2+1` (see
[steal-table.md](../formats/steal-table.md)). It is **not** in the PROT 867
record. Because it's a plain executable table, an edit is the simplest of the
four: a single same-size byte overwrite of the item, applied straight to the
SCUS file via `DiscPatcher::patch_named_file` (the non-PROT sibling of
`patch_prot_entry`, built on `legaia_iso::write::patch_file_logical`). No LZS
re-pack, no overflow, so nothing is ever skipped. `apply::randomize_steals`
reassigns the item for every stealable monster (`Shuffle` redistributes the
existing steal-item multiset, `Random` draws from the valid item pool) and
**preserves each monster's steal chance** — the item changes, the rate doesn't.
On the retail disc 189 monsters are stealable. `legaia-rando steals` lists the
current table (the audit surface).

### Doors (scene transitions)

A field scene reaches another scene through the field-VM **`0x3F`
named-scene-change op**, which carries its destination inline: `[i16 index]
[u8 name_len][name][entry_x][entry_z][dir]`. These ops are **partition-2 MAN
records**, addressed at runtime through the partition-2 record-offset table (the
controller sets the VM bytecode base to `man_base + data_region +
partition2[slot]` and runs the record — pinned by a PCSX-Redux dispatch trace;
see [MAN relocation](../formats/man-relocation.md)). On the retail disc there are
160 doors across 48 scenes; the overworld scenes (`map01`/`map02`/`map03`) are
the hubs.

Because the destination name is variable length, `apply::randomize_doors` is the
only randomizer that **resizes** an asset: it rewrites the `0x3F` op through the
relocation engine, recompresses the MAN, and rewrites the descriptor's
decompressed-size word. The whole destination descriptor (scene + entry tile +
facing) moves as one unit, so a re-pointed door always lands you somewhere valid.

`--door-coupling` picks the connectivity:

- **`coupled` (bidirectional, default)** re-pairs doors into two-way connections
  via a random involution over the sites — for matched doors `A` and `B`, `A` is
  sent to where `B` is reached from and vice versa, so walking through a door and
  turning around returns you the way you came. To guarantee that this never
  half-applies, coupled mode restricts itself to **length-preserving** swaps: it
  re-pairs only *balanced* connections (equal door counts in each direction)
  whose destination names match in length, so the decompressed MAN size is
  unchanged and **no scene — including the un-growable overworld hubs — can
  overflow**. The result introduces zero new one-way edges (a whole-graph
  symmetry invariant, asserted by `door_patch_real`). Doors with no
  length-compatible reverse partner (dead-end / one-way story warps, or doors
  orphaned by an unequal-direction connection) are left at their original
  destination — never given a one-way reassignment — and reported as `unpaired`.
- **`decoupled` (one-way)** reassigns every door's destination independently
  (`shuffle` permutes the existing destinations, `random` draws from the global
  pool), so going back through the destination's own doors is not guaranteed to
  return you. This is the variable-length path: a destination of any name length
  can land in any door.

In **decoupled** mode a scene whose rebuilt MAN can't grow within its on-disc
footprint (the big overworld hubs, whose next asset sits flush after the MAN) is
**skipped** — it keeps its original doors — and reported, rather than relocating
the whole bundle. (Coupled mode is same-size, so this doesn't arise; should a
recompress ever overflow anyway, the revert is a transitive closure over both
the new and original pairings, so a whole connection cycle reverts together
rather than half-applying.)

### House doors (intra-town)

Entering a house/interior within a town is **not** a scene change — it's an
**intra-scene reposition**: the field VM runs a **`0x23 MOVE_TO`** op that
teleports the player to an interior sub-area tile in the *same* scene (pinned at
the instruction level by `probe.step.find_writer`; the writer is `FUN_801de840`
`case 0x23` — see [pcsx-redux-automation.md](pcsx-redux-automation.md)). So the
door "record" is the op's two operand bytes `[0x23][xb][zb]` (`tile = byte &
0x7F`). `legaia_rando::house_door::SceneHouseDoors` enumerates a scene's
non-sentinel MOVE_TO sites and `--house-doors shuffle` does a **per-scene,
multiset-preserving shuffle** of their target tiles — every target stays a tile
the scene already uses (no off-map placement), a same-size 2-byte operand edit
recompressed in place (no relocation). On retail: 220 shuffleable targets across
28 scenes.

**Experimental — the op is shared, and enumeration is partial.** `0x23 MOVE_TO`
is also how NPC / cutscene scripts move actors, and there's no clean structural
marker separating door warps from those, so the shuffle also scrambles some actor
positions within each town. It is opt-in, `shuffle`-only (a `random` draw would
place actors off-map), and excludes the `(0x7F, 0x7F)` "here" sentinel. The
read-only `house-doors` listing shows the touched population per scene.

Enumeration finds only the `MOVE_TO` ops a **clean** field-VM walk reaches (it
stops at the first byte that doesn't decode as a valid op, so it never mistakes
arbitrary data for a `0x23`). In some towns the interior-*entry* warps target
high interior tiles and sit past such a desync, so the sites found there are the
lower-coordinate doorstep / NPC repositions rather than the "enter the house"
warps — meaning a small town's house entries may not actually change. Closing
that gap needs a more aggressive walk-past-desync enumeration (with a way to
reject data false positives) or a per-town runtime trace of the entry op; both
remain open, which is why the feature is experimental.

### Starting items

A vanilla New Game begins with one inventory slot — Healing Leaf (item `0x77`)
×5 — and there is **no static starting-inventory table** to edit: the new-game
data-init `FUN_80034A6C` builds it in code, writing `inventory[0] = (0x77, 5)`
into the live consumable bag at `0x80085958` (`SC + 0x1818`) with an
`addiu`/`sb` pair (see [new-game-table.md](../formats/new-game-table.md)). So
this randomizer rewrites the **seed code** itself. The 40-byte region at
`0x80034b04` is reclaimable: it holds that seed plus a 6-instruction loop that
zeroes the 512 bytes *below* the inventory — redundant, because **both** callers
of `FUN_80034A6C` `memset` the whole `SC[0..0x1a18)` block (which contains the
inventory) right before the call.

`apply::randomize_starting_items` plans `n` distinct random consumables (each a
small random count) and writes one **packed halfword store** per item into that
region — an inventory slot is two contiguous bytes `[id][count]`, so
`addiu $v0, (count<<8)|id; sh $v0, (0x1818 + 2k)($s0)` seeds a slot in two
instructions. Ten instructions / two per item caps it at **five** starting
items. The patch is the same size as the original code (no executable growth or
relocation), applied like the steal table via `patch_named_file`. Because the
write lands directly in the consumable page (bypassing the engine's id-routing
add primitive), the pool is the contiguous consumable block `0x77..=0x8e`
(Healing Leaf … Wonder Elixir). `--starting-items N` (0 = leave vanilla); the
read-only `starting-items` listing shows the current bag.

### Unused content

The game ships fully-formed content it never surfaces in normal play; two opt-in
toggles bring it back. They are *additive* — a normal run never places them, so
the disc stays vanilla unless you ask. Both are pinned by the disc-gated
`unused_content_real` test.

**`--unused-enemies`** re-introduces the **Evil Bat**, an enemy whose record
lives in the `battle_data` archive (monster ids 176/177/178 are byte-identical
clones of each other and of the in-use Evil Bat at id 140) but which no scene's
encounter formation references. The battle loader streams a monster's
`0x14000` archive slot on demand keyed by its id — there is **no per-scene
monster preload list** — so injecting one of these ids into a formation byte is
sufficient to make it spawn and render; nothing else needs patching. The toggle
adds the curated ids ([`unused::UNUSED_ENEMY_IDS`]) to each scene's encounter
candidate pool. It only takes effect with `--encounters random`: a
multiset-preserving `shuffle` can't introduce a new monster, by construction.

**`--unused-items`** adds two items to the random-fill pool used by the `random`
drop / chest / steal modes:

- **"Something Good" (`0x6B`)** — a 50,000 G sell item the shipped game never
  hands out. It is *named* in the item table, so the valid pool already accepts
  it; the toggle includes it explicitly for clarity.
- **the unnamed accessory (`0xFD`)** — an accessory-class slot whose name string
  is *empty*, so the valid pool excludes it. The toggle is what makes it
  obtainable. Because a blank name would read as an empty line in chests / menus,
  the toggle also **names it "Seru Bell"**: it writes the string into a reserved,
  runtime-**constant** region of `SCUS_942.54` and repoints *only* `0xFD`'s name
  pointer at it (a same-size patch, like the starting-item seed; the other ids
  that share the empty-string slot — `0x12`/`0x1A`/`0x52`/`0xB9` — are left
  blank). Picking the target is the subtle part: the data segment's *trailing*
  zero-fill is **not** usable — it is zero in the file but is `.sbss`/`.bss`-class
  scratch the game overwrites with variables at runtime (a string there renders
  as a glyph that changes every frame). The string instead goes to
  `item_name::SERU_BELL_STRING_VA` (`0x80079900`), pinned for the US build inside
  a 3376-byte block at `0x80079840` that is zero in the file *and* across diverse
  runtime states (battle / field / menu / world-map / title) — reserved space the
  game never writes. The injection guards on the target bytes being zero, so a
  differently-laid-out image is skipped rather than corrupted.

  The accessory's documented effect is to make only Seru-class enemies appear in
  random encounters. Because it is unobtainable in retail that effect is never
  exercised by the shipped game, so treat it as experimental.

### Re-pack slack

A scene MAN is packed with **no compressed slack** (the next asset starts right
after it), so the re-packed stream must be no larger than the original. The
[LZS re-packer's lazy matching](../formats/lzs.md#encoding-re-packing) makes that
hold for every scene MAN but one (and for every monster-archive slot). The rare
stream that still overflows is **skipped** (its scene / slot left unchanged) and
recorded in the apply report rather than aborting the run; the CLI prints the
skipped entries.

## The patch chain

A PROT-entry-relative edit maps to a disc byte range like this:

```text
disc image (2352-byte sectors)
  -> ISO 9660: PROT.DAT lives at disc sector prot_lba
    -> PROT TOC: entry N starts at start_lba[N] * 2048 bytes into PROT.DAT
      -> asset: an edit at offset_in_entry bytes into the entry
```

so a PROT-entry-relative offset becomes the PROT.DAT-logical offset
`start_lba[N] * 2048 + offset_in_entry`, which
`legaia_iso::write::patch_file_logical` turns into physical-sector writes plus
EDC/ECC re-encode. `DiscPatcher::patch_prot_entry` is the generic entry point;
`patch_monster_slot` / `monster_slot` are the `battle_data` helpers.

## EDC/ECC: not game-specific

The error-correction math is the generic CD-ROM scheme from ECMA-130 / the
Yellow Book — the same EDC (CRC, reversed polynomial `0xD8018001`) and
Reed-Solomon P/Q ECC (over GF(2⁸), generator `0x11D`) every PSX disc and every
mastering tool uses. The header (`0x00C..0x010`) is treated as zero per the
Form 1 convention, so parity is independent of the sector's MSF address. It
embeds no game bytes. The decisive correctness check is the disc-gated test that
re-encodes real PROT.DAT sectors and reproduces their stored EDC/ECC
bit-for-bit.

## Tests

| Test | Gate | What it proves |
|---|---|---|
| `crates/lzs` unit tests | CI | `decompress(compress(x)) == x` across literals, RLE, repeats, pseudorandom, >4 KB-window input; structured data actually compresses |
| `crates/asset` `lzs_compress_roundtrip_real` | disc-gated | the encoder round-trips real monster records + LZS-container sections, and compresses them |
| `crates/iso` `write` unit tests | CI | encode is idempotent / self-consistent; corrupting user data invalidates until re-encoded; ECC is address-independent; a seam-straddling patch keeps both sectors valid |
| `crates/iso` `ecc_real` | disc-gated | the encoder reproduces real PROT.DAT sectors' EDC/ECC bit-for-bit; a one-byte patch + restore round-trips a real sector exactly |
| `crates/rando` unit tests | CI | seeded planner determinism; shuffle preserves the drop multiset; surgical `set_drop`; PPF diff/write/apply round-trip; a synthetic-disc patch round-trips through the disc → ISO → PROT chain |
| `crates/rando` `disc_patch_real` | disc-gated | patch a real monster's drop onto a scratch copy of the disc; it re-decodes off the patched image with neighbours untouched and sectors valid |
| `crates/rando` `rando_cli_real` | disc-gated | full-archive shuffle: plan from a seed → apply → each monster reads its planned drop (skipped slots unchanged) → diff into a PPF that reproduces the patched image; deterministic for a fixed seed |
| `crates/rando` `encounter_patch_real` | disc-gated | whole-disc encounter shuffle: re-decode every patched scene MAN off the disc and assert counts + id multiset preserved, ids in-pool, sectors EDC/ECC-valid, deterministic |
| `crates/rando` `chest_patch_real` | disc-gated | whole-disc chest shuffle: re-decode every patched scene MAN, assert give-item site offsets unchanged + chest-item multiset preserved + sectors valid + deterministic |
| `crates/rando` `steal_patch_real` | disc-gated | whole-disc steal shuffle: re-read the patched `SCUS_942.54` steal table, assert the steal-item multiset preserved + every steal chance byte untouched + the table sector EDC/ECC-valid + deterministic |
| `crates/asset` `man_edit` unit tests | CI | the MAN relocation engine: grow / shrink a destination name relocates the section + later-record offsets, a spanning relative jump's delta is fixed (a non-spanning one isn't), the rebuilt MAN re-parses |
| `crates/rando` `door_enumerate_real` | disc-gated | whole-disc door census: 160 doors across 48 scenes, every destination a clean CDNAME label, the pinned town01 → map01 exit present, the overworld hubs fan out |
| `crates/rando` `door_patch_real` | disc-gated | whole-disc door shuffle (one-way + coupled): re-decode every patched scene MAN, assert the destination multiset preserved (clean shuffle) / names valid (with skips), sectors EDC/ECC-valid, image size unchanged, deterministic |
| `crates/rando` `house_door_patch_real` | disc-gated | whole-disc intra-town (house) door shuffle: re-decode every patched scene MAN, assert the per-scene `0x23 MOVE_TO` target-tile multiset preserved, sectors EDC/ECC-valid, image size unchanged, deterministic |
| `crates/rando` `starting_items_patch_real` | disc-gated | starting-item randomize: re-decode the rewritten `FUN_80034A6C` seed off the patched `SCUS_942.54`, assert the seeded items match the plan + are in-pool consumables + the surrounding function bytes are untouched + image size unchanged + sector EDC/ECC-valid + deterministic |
| `crates/rando` `unused_content_real` | disc-gated | the unused-content facts: Evil Bat ids 176/177/178 are byte-identical clones of id 140; item `0x6B` is named vs `0xFD` unnamed (so the pool widens by exactly one); the `--unused-enemies` toggle injects an unused id only when enabled (deterministic); and the "Seru Bell" injection names only `0xFD` (others stay blank), same-size, sector EDC/ECC-valid, idempotent |
| `crates/engine-core` `chest_randomizer_runtime_e2e` | disc-gated | runtime oracle: patch one chest, re-decode the MAN off the patched image, drive its inline interaction script through the real field VM, assert the runtime grants the patched id (not the original) |
| `crates/engine-core` `monster_drop_randomizer_runtime_e2e` | disc-gated | runtime oracle: patch one monster's drop item, re-decode the record off the patched archive, build the engine catalog, drive a one-monster formation through the victory-spoils path (`apply_battle_loot`), assert the runtime grants the patched drop (not the original) |
| `crates/engine-core` `encounter_randomizer_runtime_e2e` | disc-gated | runtime oracle: patch one scene formation's slot-0 monster id, re-decode the MAN off the patched image, build the encounter table + per-row formation defs from those bytes, force that row into a battle through the live-loop encounter path, assert the spawned enemy actor carries the patched id (not the original) |
| `crates/engine-core` `steal_randomizer_runtime_e2e` | disc-gated | runtime oracle: patch one monster's steal item byte in `SCUS_942.54`, re-decode the steal table off the patched image, drive the engine steal-grant kernel (`World::apply_steal`), assert the runtime steals the patched id (not the original); chance preserved |
| `crates/engine-core` `door_randomizer_runtime_e2e` | disc-gated | runtime oracle: patch Rim Elm's exit (the `0x3F` op → map01) to a differently-named scene, re-decode the patched MAN off the patched image, drive the patched op through the real field VM (`World::load_field_script` + `tick`), assert the runtime warps to the patched destination (not the original) |
| `crates/engine-core` `starting_items_randomizer_runtime_e2e` | disc-gated | runtime oracle: confirm a New Game off the unpatched disc seeds Healing Leaf ×5 (baseline), randomize the seed on a scratch copy, re-decode it off the patched image, seed a fresh world via `World::seed_starting_inventory`, assert the bag holds exactly the patched items (not the vanilla Healing Leaf ×5) |
| `crates/engine-core` `unused_enemy_randomizer_runtime_e2e` | disc-gated | runtime oracle: run the `--unused-enemies` toggle path until it places an unused Evil Bat id at a formation slot, re-decode off the patched image, force that row into a battle, assert the spawned enemy actor carries an unused-enemy id (baseline spawns the vanilla monster) |
| `crates/engine-core` `unused_item_randomizer_runtime_e2e` | disc-gated | runtime oracle: apply the "Seru Bell" name injection and assert the item table resolves `0xFD` to it (others stay blank), then patch a monster's drop to `0xFD` and drive `apply_battle_loot`, asserting the bag receives the unused accessory (baseline grants the original) |

Disc-gated tests read `LEGAIA_DISC_BIN`; with it unset they skip and pass.

The eight `engine-core` runtime oracles answer a question the `crates/rando`
patch tests don't: not just that the patched byte is *written* faithfully, but
that a runtime actually *reads it and acts on it* — grants the new item, spawns
the new monster, or warps to the new scene. A savestate can't prove this — the
scene MAN / `battle_data` archive / steal table is resident in RAM the moment
you're in the room / battle (or as soon as the executable loads), so a state
captured on a patched disc still serves the original from the cached RAM copy;
the patched value is only seen after a fresh scene / battle / executable load
re-streams it off disc. The clean-room engine sidesteps that cache by decoding
straight from disc bytes and running the actual grant / spawn / warp path, so it
observes the patch a savestate would mask.

## No-Sony-bytes hygiene

The crate never embeds, commits, or redistributes game bytes. A patched `.bin`
contains Sony data and is never committed; the intended distribution form is a
patcher tool + seed, and/or the **PPF patch** the CLI emits. A PPF carries only
the deltas between the user's original disc and the patched one — it is
meaningless without the original image the user already owns, so it is safe to
share where a patched `.bin` is not.

## See also

- [`crates/rando`](../../crates/rando/README.md) — the crate.
- [LZS compression](../formats/lzs.md) — the encoder this builds on.
- [PSX disc geometry](../formats/disc.md) — the Mode 2/2352 sector layout.
- [PROT.DAT TOC](../formats/prot.md) — entry → LBA addressing.
- [Monster animation](../formats/monster-animation.md) and
  [encounter records](../formats/encounter.md) — the data the randomizer edits.
