# legaia-arts-patch

The disc patcher's Tactical Arts layer, split out of
[`legaia-patcher`](../patcher/README.md): every edit whose subject is an art -
the combo that triggers it, the damage it deals, the AP it grants and costs,
and the Super Arts' list, menu and power. `legaia-patcher` depends on this
crate, never the other way round, and re-exports every module at its old path
(`legaia_patcher::arts`, `legaia_patcher::super_art_list`, ...), so the CLI,
the browser patcher and the disc-gated tests name the same paths they always
did.

No Sony bytes: the data edits mutate a player battle file's `record[0]` handed
in as bytes, the code mods are assembled from the
[`legaia-code-hooks`](../code-hooks/README.md) encoders, and nothing in this
crate embeds game data.

## What lives here

| Module | Covers |
|---|---|
| `arts` | The button-combo randomizer: the SCUS arts-name table's glyph pointers and the `record[0]` trigger bytes, edited together so the menu and the matcher agree. Also the `record[0]` decode / re-pack helpers the modules below share. |
| `arts_power` | `--arts-power`: a chain art's per-hit power tiers, keyed by input combo. |
| `super_art_power` | `--super-art-power`: the same knob for a Super Art, which has no combo and no arts-table row and so is keyed by name. |
| `arts_ap_grant` | `--arts-ap-grant` / `--arts-ap-cost`: the per-art AP override hook in the battle overlay. |
| `oscillating_ap` | `--oscillating-ap`: every art dealt per battle onto a cost side or a grant side, with the damage scale that pays for it. |
| `super_art_list` | `--show-super-arts`: a character's Super Arts on the in-battle move list, with the arrows that trigger them. |
| `super_art_menu` | The same list on the pause-menu Status page, hosted in the custom overlay. |

The mechanics each one edits are in
[`docs/subsystems/arts-command-gauge.md`](../../docs/subsystems/arts-command-gauge.md)
and [`docs/formats/art-data.md`](../../docs/formats/art-data.md); the flags
and their exclusions are in
[`docs/tooling/randomizer.md`](../../docs/tooling/randomizer.md).

## The split line

A module sits here when it closes over only the arts tables of
[`legaia-art`](../art/README.md), the encoders and arena bounds of
`legaia-code-hooks`, and the seeded generator of
[`legaia-disc-patch`](../disc-patch/README.md). The crate root aliases `mips`,
`mips_sim`, `shiny_seru`, `seru_overlay` and `rng` at the `crate::<module>`
paths the files were written against.

What stays in `legaia-patcher`: the `apply` layer that sequences these edits
onto a disc and enforces which flags exclude each other, and the two arts
mods whose builders reach into it - the Super Arts Pack (`super_arts_pack`)
and the arts-name fix (`arts_name_fix`). The disc-gated oracles
(`crates/patcher/tests/arts_*_real.rs`, `super_art_*_real.rs`,
`oscillating_ap_real.rs`) stay with the patcher too, since they drive the
edits through `apply`.

## See also

- [`crates/patcher`](../patcher/README.md) - the patcher this was split from.
- [`crates/art`](../art/README.md) - the arts tables, trigger matchers and
  tokenizer these edits are written against.
