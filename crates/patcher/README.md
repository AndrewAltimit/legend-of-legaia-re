# legaia-patcher

Disc-patching toolkit for a user-supplied Legend of Legaia disc image. Every
feature is built on the same foundation: same-size in-place sector edits with
the EDC/ECC re-encoded (`legaia_disc_patch::disc::DiscPatcher` over `legaia_iso::write`), emitted
as a portable PPF 3.0 patch and/or a patched `.bin` copy for local play.

That machinery lives in [`legaia-disc-patch`](../disc-patch/README.md).
This crate is the top of the family; the rule for what lives where:

| Crate | Holds |
|---|---|
| [`legaia-disc-patch`](../disc-patch/README.md) | The write foundation: `DiscPatcher`, PPF, the free-space ledger, re-pack budgets, the monster-slot re-pack, the seeded `rng`. |
| [`legaia-code-hooks`](../code-hooks/README.md) | The MIPS toolkit (`mips`, `mips_sim`), the shared injection-arena map (`arenas`) and the hook mods that are only a plan over those. |
| [`legaia-party-swap`](../party-swap/README.md) | Disc-free model-transform kernels. |
| [`legaia-arts-patch`](../arts-patch/README.md) | The Tactical Arts family: `arts`, `arts_power`, `arts_ap_grant`, `oscillating_ap`, `super_art_*`. |
| [`legaia-texture-replace`](../texture-replace/README.md) | The image-replacement family: `texture`, `texture_palettes`, `battle_texture`, `monster_texture`, `save_icon`. |
| [`legaia-translate`](../translate/README.md) | The language-pack pipeline. |
| [`legaia-delilas-party`](../delilas-party/README.md) | The play-as-Delilas family: `delilas_party` and its cast-route, voice, effect, signature-attack and field-scene modules. |
| [`legaia-delilas-challenge`](../delilas-challenge/README.md) | The Delilas Challenge family: the dome course, its enrollment script and the custom items. |
| `legaia-patcher` (this crate) | The `apply` layer that sequences every edit onto a disc, the CLI, the randomizer's own data shuffles, and the hook mods whose builders reach into those. |

Every consumer - the family itself, the browser patcher, the engine's
runtime oracles - names the owning crate. This page is the reference for
the whole family: a module named in the rest of it is a module of the crate
the table above assigns it to.

Four patching families share the machinery:

- **The randomizer** - seeded reassignment of gameplay data (drops,
  encounters, chests, steals, Tactical Arts combos, doors, shops, casino
  prizes, starting items and level, prices, equipment and battle-tuning
  tables), plus the hand-assembled MIPS hooks detoured into dead space and
  the seedless retail-bug fixes. The Fortress encounter fix
  (`fortress_encounter_fix`, a scene-MAN encounter-table edit) lives here;
  the two battle-code fixes are `legaia-code-hooks`'.
- **New-content mods** - seedless structural rebuilds shipped as flags and
  untouched by the seed: the Delilas party swap, the Delilas Challenge, the
  custom items, the Super Arts move list and ZetaPhoenix's Super Arts Pack.
- **Translation packs** - the `translate` CLI family: disc text out to an
  editable YAML pack, the filled pack back in as an in-place reimport.
- **Manual edits** - targeted single-record patching: monster blocks and
  models, texture replacement, per-art power and AP, place names, prices.

## What this crate holds

- `apply` - the orchestration layer: a module per feature family that
  sequences its edits onto a `DiscPatcher` and returns a report. The CLI and
  the browser patcher both go through it.
- The randomizer's own data shuffles and tuning modules (one module per
  feature under `src/`), and the hook mods whose builders reach into them.
- `bin/legaia-patcher` - the CLI: `randomize`, `translate`, the read-only
  listing subcommands, the manual-edit subcommands, `scus-pokes`.

## What it must not depend on

No engine crate. The patcher reads and writes a disc through the format
crates and `legaia-disc-patch`; the engine's runtime oracles depend on the
patcher (as a dev-dependency), never the reverse. It ships only code: no
game bytes are embedded or committed, and every test that needs real data is
disc-gated.

## Where it is documented

| Topic | Page |
|---|---|
| What each randomizer, slider and mod does; the full flag reference | [`randomizer.md`](../../docs/tooling/randomizer.md) |
| Every module: what it reads, writes and guarantees; the CLI; the tests | [`patcher-modules.md`](../../docs/tooling/patcher-modules.md) |
| The write path, the injection arenas, each MIPS hook's design | [`randomizer-internals.md`](../../docs/tooling/randomizer-internals.md) |
| The Delilas Challenge, custom items and party swap | [`randomizer-delilas.md`](../../docs/tooling/randomizer-delilas.md) |
| Language packs | [`translation/`](../../docs/tooling/translation/index.md) |

## See also

- [`crates/disc-patch`](../disc-patch/README.md) - the write foundation.
- [`crates/lzs`](../lzs/README.md) - the LZS encoder (`compress`) re-packing relies on.
- [`crates/iso`](../iso/README.md) - Mode 2/2352 sector write-back (`write` module).
- [`crates/actor-formats`](../actor-formats/README.md), [`crates/game-tables`](../game-tables/README.md) - the monster-archive and static-table parsers it edits.
