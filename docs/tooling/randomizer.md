# Randomizer / disc patcher

Track-1-adjacent tooling that edits gameplay data on a **user-supplied** retail
disc image: it shuffles monster item drops, random-encounter formations, and
treasure-chest contents, and writes the result back into the `.bin`. It does not
touch the clean-room engine.

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

## Editing model: same-size, in place

Every edit overwrites bytes **in place** and never changes a byte count, so no
LBA, PROT TOC, or ISO 9660 directory record ever moves. That keeps the patch a
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
legaia-rando randomize --input DISC.bin --seed myrun --drops shuffle
legaia-rando randomize --input DISC.bin --seed 0xC0FFEE --drops random \
    --encounters shuffle --patch run.ppf --output patched.bin --manifest run.toml
legaia-rando verify    --input DISC.bin --patch run.ppf       # apply + sanity-check
```

`randomize` plans the run, applies it to an in-memory copy of the disc, diffs
the result against the original, and writes the changes as a **PPF 3.0** patch
(default `<input>.ppf`). `--output` also writes a full patched `.bin` for local
play. The seed is resolved from a number or a hashed string and always printed,
so a run reproduces exactly; the same seed yields a byte-identical patched image
and PPF. `--drops` and `--encounters` each take `shuffle` / `random` / `none`.
`--dry-run` reports the plan without writing; `--manifest` writes a small TOML
record of the seed + options + change counts (no game bytes, safe to share). The
`verify` subcommand applies a PPF to a copy of the user's disc and confirms the
result still parses end to end — a recipient's check that a shared patch + seed
match their own disc.

The read-only `drops` and `chests` subcommands write nothing — they decode the
randomizable populations off the user's disc and print them (item ids + names
resolved from the disc's own SCUS table; chests grouped by scene via CDNAME and
followed by an item-multiset summary). `chests` lists the exact 275-site
treasure population the chest randomizer reassigns, which is the natural place to
audit for quest / key items a run might want to keep static.

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
| `crates/engine-core` `chest_randomizer_runtime_e2e` | disc-gated | runtime oracle: patch one chest, re-decode the MAN off the patched image, drive its inline interaction script through the real field VM, assert the runtime grants the patched id (not the original) |
| `crates/engine-core` `monster_drop_randomizer_runtime_e2e` | disc-gated | runtime oracle: patch one monster's drop item, re-decode the record off the patched archive, build the engine catalog, drive a one-monster formation through the victory-spoils path (`apply_battle_loot`), assert the runtime grants the patched drop (not the original) |

Disc-gated tests read `LEGAIA_DISC_BIN`; with it unset they skip and pass.

The two `engine-core` runtime oracles answer a question the `crates/rando`
patch tests don't: not just that the patched byte is *written* faithfully, but
that a runtime actually *reads it and grants the new item*. A savestate can't
prove this — the scene MAN / `battle_data` archive is resident in RAM the moment
you're in the room / battle, so a state captured on a patched disc still serves
the original from the cached RAM copy; the patched value is only seen after a
fresh scene/battle load re-streams it off disc. The clean-room engine sidesteps
that cache by decoding straight from disc bytes and running the actual grant
path, so it observes the patch a savestate would mask.

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
