# legaia-overlay-images

Code images: how a PROT entry is recognised as MIPS code, which entry each
loader picks and where it loads, and how a slot-B cast / summon module image
is laid out.

## The rule

This crate describes **the image**; [`legaia-game-tables`](../game-tables/README.md)
decodes **the data tables found in an image**, whether that image is
`SCUS_942.54` or an overlay. The dependency runs one way: `legaia-game-tables`
takes an overlay's PROT index, load base and window bounds from here, and
nothing here names a table. A band format that is part of the slot-B module
layout stays here even though it yields records, because the layout and the
band reader call each other.

## What lives here

| Module | Covers |
|---|---|
| `mips_overlay` | Per-PROT MIPS-code-likelihood detection. See [`mips-overlay.md`](../../docs/formats/mips-overlay.md). |
| `overlay_ptr_table` | The sister detector for pointer-table-led overlay entries. See [`overlay-ptr-table.md`](../../docs/formats/overlay-ptr-table.md). |
| `static_overlay` | The static-overlay map (which PROT entry loads at which base), the named slot-A images, base recovery, and extraction of each clean-copy overlay. See [`static-overlay-pipeline.md`](../../docs/tooling/static-overlay-pipeline.md). |
| `string_refs` | Where a code image forms a string's address (a `lui` / `addiu` pair or a pointer word), decoded on the image at hand so a reader follows a string a language pack moved. See [`ui-strings.md`](../../docs/tooling/translation/ui-strings.md#what-the-port-reads). |
| `boot_overlay` | The loaders' index arithmetic: overlay parameter to PROT entry, the slot-B default, the effect side-band choice. CLI `asset boot-overlay`. See [`boot.md`](../../docs/subsystems/boot.md). |
| `slot_b_module` | File layout of the slot-B cast / summon images: head table, code, spawn-record band, inherited tail. See [`slot-b-module-layout.md`](../../docs/formats/slot-b-module-layout.md). |
| `summon_overlay` | The spawn-record band of a slot-B module: the spawn-call scan and the part records it names. See [`cast-module.md`](../../docs/subsystems/cast-module.md). |
| `cast_effect_pool` | The slot-B band indexed by PROT entry (action id or capture sub-id to module), each image resolved to its spawn records. |
| `cast_presentation` | The presentation calls a slot-B module's tick arms make into the shared helpers (screen fades, washes, the streak ribbon, CLUT copies), decoded off the module image per phase arm. |

## Layering

Depends on `legaia-bytes` and `legaia-lzs` only (its disc-gated tests also
read through `legaia-prot` and `legaia-container-formats`). It must not depend on
`legaia-game-tables`, the container layer, or any scene, UI, minigame or
actor format crate. Above it, [`legaia-disc-audit`](../disc-audit/README.md)
holds `inherited_tail`, `switch_tables` and `byte_account`, which read
overlay images too but lean on the byte-accounting walk.

The static-overlay map this crate embeds stays at
`crates/asset/data/static-overlays.toml`, the path the analysis scripts and
the docs open by name.

## Module reference

Entry points, CLI subcommands (`asset ...` is the binary in
[`crates/asset`](../asset/README.md)) and the doc page each module is specified on.

### `static_overlay`

Static overlay-extraction pipeline. PSX overlays are clean copies of a
fixed-VA-linked blob, so each runtime overlay (field / battle / …) is extracted
straight from its `PROT.DAT` entry and disassembled at its load base, identity
attached from the source entry - the structural fix for the VA-aliasing identity
problem the `overlay_<label>_<addr>` dump naming works around. `recover_base`
recovers the load base statically from the overlay's own internal `jal` call
graph; `as_loaded` / `fingerprint` / `verify_fingerprint` back the committed map
(`crates/asset/data/static-overlays.toml`); `ghidra_import_jython` / `ghidra_import_driver`
emit the Ghidra import helpers. CLI: `asset overlay list/extract/verify/ghidra/scan/find-sig/generate`.
Complements the dynamic save-state captures (it does not address runtime values).
See [`docs/tooling/static-overlay-pipeline.md`](../../docs/tooling/static-overlay-pipeline.md).

### `summon_overlay`

`summon_overlay` - Seru-magic **summon scene-graph** part records:

- A per-summon stager overlay (player extraction PROT 0903..=0913 - Gimard
  (`0x81`, attack title *Burning Attack*; *Tail Fire* is the enemy move `0x27`,
  a different id space entry) arithmetics to 0903 under the corrected loader
  index math
  - the evolved-Seru block `EVOLVED_SUMMON_STAGER_PROT` (0914..=0923,
  `spell_id 0x8C..=0x95`, the same arithmetic run), high-summon 0927..=0934, and
  the six Cort enemy boss stagers `ENEMY_BOSS_STAGER_PROT`) stages each summon
  body part with a `FUN_80021B04` call passing a per-part record - directly or
  through the `FUN_80050ED4` pool wrapper (both scanned).
- `parse(bytes, link_base)` scans those call sites and recovers the records
  (`[i16 model_sel][u16 reserved][move-VM bytecode]`, `model_sel == -1` =
  transform/pivot node, `0x4000`/`0x4001` = render-mode nodes). Records live
  in-file under link base `0x801F69D8`. The input must be one entry's bytes
  and no more; `unique_content_len` recomputes that bound from a start/next
  LBA pair, and trims a stale `.BIN` written before the entry size was
  corrected (those run on into the following stagers).

CLI `asset summon-overlay <stager .BIN> [--trim 0xNNNN]`. See
[`open-rev-eng-threads.md`](../../docs/reference/open-rev-eng-threads.md) (Seru-magic
summon visual).

### `cast_effect_pool`

`cast_effect_pool` - the whole slot-B cast-module band (PROT 0903..0966) as one
index, keyed by the PROT entry either of PROT 0898's tick dispatchers names:
`seru_module_prot(action_id)` = `903 + (id - 0x81)` (`FUN_801F1ED4`, table
`0x801CF4EC`) and `capture_module_prot(sub_id)` = `935 + sub_id`
(`FUN_801F2160`, table `0x801CF56C`, keyed on the spell record's `+0x01` byte).
`CastEffectPool::insert` parses one entry's spawn records through
`summon_overlay::parse` - this is the band's **DATA** layer only, the records
those modules hand `FUN_80050ED4` / `FUN_80021B04`; the modules' code half
(lift, camera, phase machine, damage) is not expressible as a record. See
[`cast-module.md`](../../docs/subsystems/cast-module.md#what-the-port-runs).

### `slot_b_module`

`slot_b_module` - the same 64 images read as a **file layout** rather than as a
pool: head jump table, frame-matched code partition, and the spawn-record band
with a half-open extent per record. Every extent's ends are addresses the
module's own code computes and hands to `FUN_80021B04` / `FUN_80050ED4` in
`$a2`, cut where the image's code partition resumes (PROT 0943 and 0961
interleave records with code) and dropped where the naming call sits outside
every framed body (an inherited fragment of a sibling image's routine). Feeds
`byte_account`'s `slot_b_module` walker and the `spawn_record_band` shape in
`scripts/ci/disc-coverage.py`. See
[`slot-b-module-layout.md`](../../docs/formats/slot-b-module-layout.md).
