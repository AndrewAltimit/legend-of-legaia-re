# Lane I - the two mis-aimed censuses

Both rows in the lane-C "still red" table were **instruments pointed at the
wrong thing**, not regressions. Both are now closed with the diagnosis
preserved in the test bodies and in `docs/`.

## 1. The `0x4C 0xD8` census - re-pointed at the MAN

`0x4C` (`MENU_CTRL`) is a **field-VM** opcode, so its bytecode lives in the
scene **MAN**. The census walked `Scene::find_event_scripts`, which carries
**move-VM prescripts** and no field-VM bytecode at all - so its post-fix `0`
was the correct answer for what it scanned, and its pre-fix non-zero count was
the over-read: a one-sector prescript entry read under the declared-span size
ran into the block's bundle, and the bundle MAN's opcodes were filed under the
prescript's name.

Re-derived over MAN carriers (bundle + streaming variants), decoding at real
instruction boundaries with `field_disasm` rather than scanning for the byte
pair:

> **17 opcode sites in 5 scenes** - `balden` 4, `balden2` 4, `garmel` 2,
> `jagaroom` 6, `juui2` 1.

**The unit was worth checking.** The old figure conflated two: the assertion
counted `4C D8` *byte-pair occurrences in event-script records*, while the
comment beside it said "currently 14 **scenes**". The new census reports both
separately; `17` is a count of decoded instructions and `5` of scenes.

What the re-aim also buys, all newly assertable:

- every site sits in **partition 1 record 0** - the scene-entry system script.
  No per-actor interaction script and no cutscene-timeline record uses the
  synchronous spawn;
- every site decodes exactly 9 bytes wide and is `clean` (a full
  `CLEAN_RESYNC_INSNS` run-up), so none is a resync artifact;
- per scene the sites chain contiguously at a 9-byte stride. That stride
  alignment is the same structural proof of the encoding the old test wanted,
  now taken on the right carrier.

### Corroboration, so the number isn't the code blessing itself

New test `decoded_4c_d8_census_matches_the_walker_independent_byte_scan`: for
**every** MAN carrier on the disc, the raw `4C D8` byte-pair count equals the
decoded instruction count. The two instruments fail in opposite directions - a
byte scan over-counts (operand / Shift-JIS aliases), an opcode walk
under-counts (a desync silently drops real ops) - so exact agreement carrier by
carrier rules out both. Disc-wide: 17 = 17, and per carrier 4=4, 4=4, 2=2,
6=6, 1=1. Same discipline as `flag_test_bytescan` on the flag census.

Also checked: `balden` (bundle MAN, PROT 183) and `balden2` (streaming variant,
PROT 320) carry byte-identical clusters at identical record offsets, but are
**two carriers, not one seen twice** - see §2's uniqueness property.

`drives_real_balden2_4c_d8_into_synchronous_spawn` now sources its bytes from
balden2's variant-MAN P1[0]. They are the same 9 bytes the old test drove
(`4C D8 01 63 00 66 00 66 00`), so the drive's content claim was right all
along - only its carrier attribution was wrong. `balden2_natural_drive_...`
already drove that P1[0] script and was already green; it is now the un-sliced
companion to a census that reads the same carrier.

## 2. The MAN-variant / gate-variant census - 3 is honest, corroborated

The gate-variant count going `>= 4` → `3`, and the `gameover_data` "dev copy"
of the Rim Elm `0x225` latch vanishing, are **one fact**, and it is structural
rather than statistical:

- `gameover_data`'s CDNAME block is extraction entries `1..3` - a strict
  **subset** of `town01`'s `1..10`. The two head defines (`init_data 0`,
  `gameover_data 1`) sit inside the TOC header rows and keep unshifted legacy
  windows, so they land on entries the `-2` shift gives to the following scene
  (`docs/formats/cdname.md`);
- that window holds no `SceneAssetTable` entry at all - only `town01`'s
  `FieldMap` (entry 1) and its one-sector `SceneV12Table` (entry 2). Under the
  declared-span size that one-sector entry read through the event scripts and
  into entry 4, `town01`'s bundle and its MAN;
- corrected, entry 2 is its 2048 bytes, `find_bundle` returns `None`, and the
  block carries no MAN. There was never a dev copy: it was `town01`'s MAN, once.

Independent corroboration of the surviving three, all asserted:

| scene | PROT entry | start LBA | MAN bytes |
|---|---|---|---|
| `town01` | 4 | 243 | 45338 |
| `town0b` | 13 | 731 | 50141 |
| `town0c` | 22 | 1222 | 38014 |

Three distinct entries, three distinct start LBAs, three pairwise-distinct
payloads, each with the identical `P2[3]` C1=`[0x225]` / `P2[4]` C2∋`0x225`
gate pair. Plus the general form, as a new disc-wide test:

> **`no_two_man_carriers_share_bytes_disc_wide`: 101 carriers, 101 distinct
> payloads.**

That is the property every MAN-keyed census silently depends on - it makes a
per-scene walk a partition of the corpus, so "N scenes carry X" and "N carriers
carry X" are the same statement. It is also the general form of the failure
lane C found: when it does not hold, one MAN reached through two windows
inflates every count over it by an amount nothing inside the census can see. If
it ever goes red, re-read every number in that file before trusting it.

Follow-on edits from the same fact:

- `flag_549_writer_...`: the `0x225` SET site set is now the single `town01`
  P2[3] self-latch.
- `flags_0x5a1_and_0x6c3_...`: `gameover_data` dropped from the "C3-CC
  data-table desync, not a real op" list, since it has no MAN of its own and
  was re-checking town01's bytes under another name. The remaining two
  (`town01` raw 2, `town0b` raw 3, both genuine 0) are now guarded
  non-vacuous - `town0c` was *not* substituted in, because it carries zero
  `56 C3` pairs and would have made the assertion vacuous.

## Neither instrument was worth deleting

Both measure something real once aimed correctly. The `0x4C 0xD8` census is now
the only disc-wide statement of where retail uses the synchronous spawn, and
the byte-scan cross-check makes it self-corroborating. The MAN-variant census
gained the carrier-uniqueness property, which is worth more than the row it
replaced.

## Docs

- `docs/subsystems/script-vm-menuctrl.md` - new "Where `0x4C 0xD8` occurs on
  the disc": the census, both traps (wrong carrier; balden/balden2 are two
  carriers), and the cross-check.
- `docs/subsystems/script-vm.md` - the "NOT `scene_event_scripts`" heading now
  says *why* a census aimed there can come back non-zero anyway; the
  variant-carrier section gains the byte-uniqueness property + the
  `gameover_data` correction.
- `docs/formats/man-relocation.md` - safety note: an edit is keyed to the
  carrier entry, and a name that appears to resolve another scene's MAN is a
  framing bug, not a shared asset.
- `docs/reference/open-rev-eng-threads.md` + `docs/reference/re-settled-threads.md`
  - two one-line corrections of the falsified `gameover_data` claims. **Both
  files are outside this lane's stated scope**; they each asserted a fact this
  lane disproved, so leaving them would have left a live false claim in
  committed docs. Flagging rather than assuming it was wanted.

## Verification

- `cargo test -p legaia-engine-core --release --test field_actor_spawn_disc_e2e
  --test man_variant_carrier_census_disc`: **6/6 + 28/28**, from 6 red tests
  across the two targets.
- Disc gating by **contrast**: `0` `[skip]` lines with `LEGAIA_DISC_BIN` set vs
  **34** with it unset (6 + 28), passing either way.
- `cargo fmt` clean; `cargo clippy --all-targets -p legaia-engine-core --
  -D warnings` clean.
- `check-doc-density.py` + `check-md-links.py` OK.

## Not done - out of scope, reported instead

- `crates/engine-core/examples/scan_4c_d8.rs` was rewritten alongside the test
  (the test's own header says it lifts that example's scan logic; leaving it
  aimed at event scripts would have left a second wrong instrument).
- No `engine-core` / `asset` source change was needed: the re-aimed census is
  built entirely from existing public API (`scene_man_carriers`,
  `partition_record_span`, `CLEAN_RESYNC_INSNS`, `field_disasm::LinearWalker`).
- Lane H's rows (`town01` TMD pool, `catalogued_field_states`,
  `pochi_leftovers`) and `crates/prot/**` untouched.
