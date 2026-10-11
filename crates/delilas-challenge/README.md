# legaia-delilas-challenge

The disc patcher's Delilas Challenge: a fourth option on the Muscle Dome
enrollment menu that runs a new two-round arena course against the Delilas
siblings (`--delilas-challenge`), and the three custom items that can be its
full-clear reward and that also stand alone (`--custom-items`).

It is a separate feature from the play-as-Delilas party swap in
[`legaia-delilas-party`](../delilas-party/README.md). The two are mutually
exclusive on one disc (both rewrite the Delilas arm of the battle AI picker)
and share no code, so each has its own crate.

No Sony bytes: the script bytes and the routines are the patcher's own, and
nothing in this crate embeds game data.

## What lives here

| Module | Covers |
|---|---|
| `enrollment` | The `koin1` script half: the fourth menu option, its gate on the Koru event, the warp into the arena and the reward announcement lines. |
| `dome` | The code half: the course seed hook, the slim sibling clones for the two-enemy round, the battle-rule edits the course needs and the coin payout. |
| `custom_items` | Nature's Elixir, the Ra-Seru Tear and the Fury Bloom: item records, effect machinery, battle hooks and the reward grant. |

The design reference is
[`docs/tooling/randomizer-delilas.md`](../../docs/tooling/randomizer-delilas.md).

## Layering

Every module here is a *plan*: it reads image bytes handed in as slices and
returns the edits to write. It names the MIPS encoders and the arena map of
[`legaia-code-hooks`](../code-hooks/README.md), the MAN re-pack budget and the
field-script flag encoders of [`legaia-disc-patch`](../disc-patch/README.md),
and the parser crates.

It must not depend on `legaia-patcher`, on `legaia-delilas-party` or on any
other feature crate. The `apply` layer in
[`legaia-patcher`](../patcher/README.md) sequences these plans onto a disc
(`apply::apply_delilas_challenge`, `apply::inject_custom_item_set`), and the
disc-gated oracles (`crates/patcher/tests/delilas_challenge_real.rs`,
`delilas_dome_real.rs`, `custom_items_real.rs`) stay with the patcher because
they drive the feature through `apply`.
