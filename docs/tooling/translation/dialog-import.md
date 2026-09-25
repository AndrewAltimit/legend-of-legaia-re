# Dialog import

Names are overwritten in place or moved between table slots; dialog is harder.
A scene's dialog lives inside its scene MAN, which is LZS-compressed into a
fixed footprint, and the script around each line addresses it by byte offset.
This page describes how import rewrites a scene, what it does when the result
no longer fits, and how the dialog phase is ordered around the randomizer.
The room rules themselves are summarised on
[`space-and-budgets.md`](space-and-budgets.md#dialog).

## Same-size writes

The fast default is same-size in place. A shorter translation is space-padded
to the English line's exact length, so the `0x1F ... 0x00` framing - and every
script offset around it - never moves. Import edits the segment inside the
LZS-decompressed MAN, recompresses the MAN, and writes it back at the same
LBA.

Before writing, each keyed line is checked on the disc: the `0x1F` framing
must sit at the keyed offset, and in a working pack the bytes there must equal
the pack's `source`.

## The generalized rewriter

When a line overflows its own span, or when the space-padded scene no longer
recompresses (padding every shorter line back to the English length spends
bytes a zero-slack footprint does not have), the importer tries the
**generalized rewriter**. It grows every filled segment in that scene MAN to
full length and relocates all crossing references - partition tables,
`u24_at_28`, straddling relative jumps (`man_edit::apply_text_edits`). The
result is verified as the same program by re-walking both buffers
(`text_edits_preserve_scripts`). The byte-level rules are on
[`man-relocation.md`](../../formats/man-relocation.md).

Each line is first placed on its record's clean script walk
(`man_edit::text_site`):

- a keyed run that turns out to be a coincidence inside an instruction's
  operands is skipped on every path, with a diagnostic;
- a run the walk never reaches is written same-size only.

## Recompression

A whole scene's edits must recompress into the MAN's original LZS footprint
at the same LBA. Text compresses well, and the repack falls back to an
optimal-parse LZS encoder (`legaia_lzs::compress_optimal` - exact
shortest-encoding DP, including back-references into the decoder's initial
zero window) when the fast greedy parse just misses the budget.

The retail scene entries are sector-aligned with **zero compressed slack**,
though, so an in-place grow only fits when the rewritten MAN recompresses no
larger than the original.

## Rollback

A scene that overflows at full length is re-fitted with lines rolled back to
the source text, the ones that grow the MAN most first, a batch sized by the
measured overflow at a time (`fit_man_in_footprint`). So a scene a few bytes
over loses a line or two, not the whole scene.

Only a scene the relocator refuses outright falls back to the padded write and
its longest-first rollback.

Each rolled-back line carries a per-key diagnostic. Shorten the reported lines
and re-run, or use the whole-sector relayout below, which the
[PAL fit report](../pal-localizations.md#fit-rate-against-the-usa-target)
motivates.

## Moved scenes are skipped whole

A translation-only pack has no source text to prove a line is the one it was
written for. So once any keyed line of a scene no longer sits where the pack
expects it - an earlier import relocated the scene, or another patch moved
it - the whole scene is skipped rather than risk a line landing on a neighbour
of the same length.

## Disc relayout (`--allow-relayout`)

With `import --allow-relayout`, a scene MAN whose full-length dialog
overflows its footprint is grown by whole sectors through a full-ISO relayout
instead of being rolled back, so the dialog imports byte-faithfully.

- The image grows, so this writes `--output`, not a same-size `--patch`; the
  CLI refuses the combination.
- It was built for the official PAL lifts
  ([`pal-localizations.md`](../pal-localizations.md#full-iso-relayout-closes-the-residual)):
  the PAL discs gave most scene entries extra sectors at mastering in the same
  way.
- Sectors after a grown entry shift, so an emulator save state made on another
  layout no longer matches the disc. Test a relaid-out image from a cold boot
  or a memory-card save.

On the site, the ROM patcher's *Give translated dialog more room* checkbox is
the same option ([on the site](index.md#on-the-site)).

## Streaming dungeon scenes

The ten streaming dungeon scenes carry `raw:` keys: an uncompressed MAN leads
a typed-chunk stream. They take the same growth path as a scene MAN
(`translation::stream_man`):

- the MAN chunk is relocated like a scene MAN;
- the chunks after it shift;
- the entry grows into its own sector slack, or, with relayout, by whole
  sectors.

## The dialog-carrier gate (`raw:` writes)

The `0x1F <text> 0x00` dialog framing is short enough to occur **by
coincidence** all over the disc's binary asset banks: sequenced music
(`music_01`), VAB sample banks (`vab_01`), the battle-character mesh/animation
packs (`battle_data`, PROT 1204 `other5`), monster archives, and every scene's
first ANM slot all contain runs that read as a two- or three-letter
"segment".

The per-segment quality gate (`segments::qualifies`) accepts those runs - they
are printable and letter-shaped - so a scanner that trusted them would hand a
translator a write **into binary data**. Overwriting such a coincidental hit
corrupts the asset with a same-size write that passes every framing/budget
check yet freezes the game: a garbled SEQ hangs the sound driver as New-Game
BGM starts, and a garbled PROT-1204 pack freezes the in-battle menu that
renders a character's battle form (e.g. Meta's Seru-magic list).

Both **export** and **import** therefore gate every `raw:` write on a
per-entry **dialog-carrier** check (`segments::is_dialog_carrier`): a PROT
entry is a real raw text carrier only if it carries at least
`MIN_CARRIER_PROSE` prose segments (a segment with an interior space and
enough letters to be a multi-word line). Across the retail disc the two
populations separate with a wide margin - binary banks top out at two
coincidental prose hits per entry, while the smallest genuine event-script /
dungeon-MAN carrier has eight - so the gate keeps every real carrier and
refuses every binary bank.

Import re-runs the check on the disc it is patching and skips a non-carrier
entry with a per-key diagnostic; export never emits a `raw:` key for one, so
freshly generated packs are clean. SCUS name-table (`scus:`) and scene-MAN
(`man:`) writes are unaffected - those targets are structurally addressed,
not scanned.

## Ordering: dialog before the randomizer, names after

Combined with the randomizer, a pack is applied in two phases
(`translation::import_pack_phase`, `ImportPhase::DialogOnly` /
`ImportPhase::NamesOnly`; a phase pair reports identically to one
`import_pack` run):

- **Dialog sections (`man:` / `raw:` keys) go first.** A dialog edit is
  keyed by a byte offset into the decompressed MAN, whereas the door and
  starting-bag passes **relocate** records (variable-length insertion) -
  moving every byte after the splice. Applied first, the translated text
  simply rides along with any later relocation. The reverse order is not
  corrupting (the framing/source check skips a moved key) but it silently
  loses the relocated scenes' lines.
- **SCUS name sections (`scus:` keys) go last.** The equipment-bonus-drop
  pass classifies gear by matching the disc's item names against curated
  English names (`legaia_patcher::equipment::equipment_pool`); with the item
  table already translated its pool comes back empty and the pass aborts.
  Nothing in the randomizer relocates a SCUS string, so translating the name
  tables after every pass is always safe - and every other name-keyed pass
  tests only whether a name is non-empty, which a translation preserves. The
  overlay `ui_menu` strings ride with this phase for the same reason: no
  randomizer pass relocates or classifies an overlay string.

The randomizer otherwise reads structure - records, tables, item **ids** -
never text, so translated strings never perturb it. A standalone
`translate import` (no randomizer) applies everything in one pass.
