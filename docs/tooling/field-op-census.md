# The field-VM opcode census

`asset field-op-census` walks every field-VM bytecode carrier on the disc,
decodes it, and counts opcode occurrences - with the sub-dispatched opcodes
(`0x4C`, `0x43`, `0x45`, `0x49`, `0x34`) broken out by sub-arm. It answers one
question, for an **arbitrary** opcode:

> does any shipped scene carry op X at a decoded instruction boundary?

Three censuses existed before it - `legaia-engine man-scripts`'s
`--system-flag-census`, `--motion-flag-census` and `--op49-window-census` - and
each reports one *family* of ops. An arm outside those families had no
instrument at all, so the rows that turn on "is there a carrier" rested on
nothing. [`reach-triage.md`](reach-triage.md) names the two that were filed
that way.

```bash
asset field-op-census extracted/PROT --cdname extracted/CDNAME.TXT \
    --csv target/field-op-census.csv
asset field-op-census extracted/PROT --cdname extracted/CDNAME.TXT \
    --only '4C CF' --context
```

`--only <op>` lists the carriers of one op (`4C CF`, `4CCF` and `39` all
parse); `--context` prints the decoded neighbourhood of each of its clean hits.
`--csv` writes the whole per-carrier x per-op table. Output goes under
`target/`, regenerable from the disc; nothing about it is committed.

## Why a byte scan is not the instrument

A field-VM record embeds Shift-JIS message text, and every opcode byte value
occurs in prose. Scanning `PROT.DAT` for the bytes `4C EA` finds the pair
inside dialogue at whatever rate two bytes occur, and the scripted-encounter
hunt already records what that costs. The census decodes instead: it walks each
record from its **first-opcode offset** with the field-VM disassembler and keys
the tally on an instruction boundary.

## The three carrier kinds

All three are walked, and each carries scripts the others do not:

| carrier | what it is |
|---|---|
| `bundle` | the scene bundle's MAN - the `type 0x03` descriptor, LZS-packed |
| `stream` | a `type 0x03` chunk inside a DATA_FIELD stream: the per-scene **variant** MAN, which carries cutscene records the bundle MAN does not |
| `event` | a raw event-script carrier (`scene_event_scripts`), which is where a `.PCH` scene's prescript records live - the prescript is the PROT entry *after* the `.PCH` table, not a field inside it |

Inside a MAN, every record of all three partitions is walked, each from the
first-opcode offset its **own** partition's header shape gives. The three
differ, and applying one formula to all three starts a whole record class three
bytes late and desyncs it - which is how an entire class of door records goes
missing from a census. See `partition_record_span` in
[`field_disasm/census.rs`](../../crates/asset/src/field_disasm/census.rs).

## `clean` versus `total`, and what a zero means

The walk is an over-approximating linear disassembly. Once it hits a decode
error inside a record - a truncated operand, an unsized sub-op, a byte that is
not an opcode - it resumes one byte on, so every later boundary in that record
is a guess. Each tally is therefore kept twice:

- **`total`** counts every decoded occurrence;
- **`clean`** counts only the ones decoded *before* that record's first decode
  error.

`clean` is the defensible number and `total` is a lead. The `0x45` rows make
the gap concrete: `45 6C` has 257 `total` and **0** clean, because `45 6C` is
`"El"` in ASCII.

**A `clean` count of zero means no shipped scene reaches that arm through the
field VM's own bytecode.** For a runtime-reach row that is the whole question:
there is no carrier to drive, so the row is not waiting on a replay fixture and
belongs in the not-playthrough bucket rather than the no-ladder one. It is
*not* a claim that the handler is dead - an arm can still be reached by a
cross-context dispatch whose carrier the walk mis-sizes.

**A clean hit is a lead until its record is read.** A linear walk can stay
error-free through message text and re-sync on a byte that is no opcode, so a
rare op's handful of hits get `--context` run over them and the decoded
neighbourhood read. Every finding below was checked that way.

## Cross-validation

The camera lane's per-arm instrument
(`crates/engine-core/tests/field_camera_zone_arms_disc.rs`) counts four `0x4C`
arms over the same corpus with no coherence gate, and its four figures are the
census's `total` exactly: `[4C 38]` 250, `[4C 39]` 323, `[4C C4]` 44, `[4C 3E]`
5. For the two rare arms the scene counts agree as well - `[4C C4]` in two
scenes, `[4C 3E]` in two - which is the useful check, because those are the
counts a coherence gate could have moved and did not.

## What the census settled

### `[4C EA]` - the scripted game-over - has exactly one carrier

One clean occurrence disc-wide, in the world-map bundle `map03` (PROT 0392),
partition 2 record 9. Its neighbourhood is well-formed on both sides: a named
scene change, a self-looping `JmpRel`, the `4C EA`, then a 12-frame wait and
another self-loop - the shape of a scripted hand-off that never returns. A
second occurrence in `map01` (PROT 0086) sits past that record's first decode
error and is residue.

So the runtime-reach row for the handler is not "no carrier exists": the
carrier is one record of one world-map scene, and driving it is a story-state
fixture rather than a missing instrument.

### `[4C 52]` - `TAKE_ITEM` - has three

Three clean occurrences in three scenes: `geremi` (PROT 0166) partition 2
record 16, and `ropeway` (PROT 0208) / `ropeway2` (PROT 0339) partition 1,
which carry the same script in the two variants of one scene and both set a
system flag immediately before confiscating. The op's *fallback* leg - unequip
when the bag misses - still needs the item worn rather than carried, so the row
stays gated; what it no longer lacks is a carrier to point a fixture at.

### `[4C CF]` - the script camera-focus override - has 46, all in one scene

Every clean occurrence is in `uru` (PROT 0435), and the neighbourhoods place
them beside `0x45 C0` camera applies and `CamCfg` writes. See
[`script-vm-menuctrl.md`](../subsystems/script-vm-menuctrl.md) for what the arm
does; the census is what says one scene uses it and ninety-odd do not.

### A third reach row the census speaks to, and does not close

`field_actor_timers.rs` asks for a scene script issuing `0x43 0C` (the
cinematic wipe) or `0x43 09` (the three-axis tween). The census finds **zero**
coherent occurrences of either, and 31 incoherent ones across six carriers -
every one inside a record that had already desynced. Both sub-ops are sized by
the decoder (`AllocScripted`, `Sub9Tween`), so the census is not structurally
blind to them.

What a zero cannot do is rule out an occurrence sitting behind an *earlier*
desync in its own record, and that is the honest limit of the instrument: it
under-counts, never over-counts. So the row's premise has no evidence behind it
and its fixture cannot be specified, but the bucket does not flip until those
31 are read.

## Reading the summary table

The default run prints one row per key, `clean`-descending, with the number of
distinct scenes each op's clean hits span. Two things are worth knowing before
quoting a figure:

- the **scene** column counts CDNAME blocks, resolved through
  `block_for_extraction_index`, so it is in extraction-index space;
- the header line carries the walk's own health - carriers, MAN payloads,
  records, desynced records, decode errors, script bytes - because a per-op
  count is only as good as the walk that produced it, and roughly half of all
  records desync somewhere (most of them are text-heavy).
