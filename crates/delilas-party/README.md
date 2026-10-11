# legaia-delilas-party

The disc patcher's play-as-Delilas mod (`--delilas-party`): the playable party
replaced by the Delilas siblings Gi / Lu / Che, while the ravine duels field
Vahn / Noa / Gala.

The model transforms are the pure kernels of
[`legaia-party-swap`](../party-swap/README.md). This crate is the layer above
them: it writes the swapped models onto a disc and carries everything a
swapped hero needs beyond a mesh.

No Sony bytes: every byte written is derived from the user's own disc, and
nothing in this crate embeds game data.

## What lives here

| Module | Covers |
|---|---|
| `delilas_party` | The orchestrator and `PartyMapping` (any permutation of `gi` / `lu` / `che`): battle and field model swap in both directions, names, save portraits and the dialog pass. Its submodules: `apply`, `reskin` (arts names and combos), `moveset`, `signature`, `timing`. |
| `delilas_signature_attack` | A hero's Hyper art turned into the sibling's signature attack: stage-chain retarget, event-frame replace, renames in both id spaces. |
| `delilas_effects` | The signature effect prototypes transplanted into spare prototype-table ids. |
| `delilas_voice` | Battle grunt voices resampled from the sibling SPU banks in place. |
| `delilas_voice_fx` | The `--delilas-arts-voice` modes, including the pitch / formant re-voice. |
| `delilas_xa_voice` | The XA-sector voice write path, with the Form 2 EDC re-encoded. |
| `enemy_anim_mirror` | Hero idles and Hyper chains written into the swapped monster blocks, so the duel-side heroes fight under their own clips. |
| `nivora_field` | The duel field scene rebuilt with the mapped heroes' field rigs (the disc adapter over the `legaia-party-swap` kernel of the same name). |
| `delilas_cast` | Routing a swapped hero's signature art into the retail enemy cast module: the SCUS queue hook, the staged caves and the word edits in cast modules 958 / 959 / 960. |

The full mechanism reference is
[`docs/tooling/randomizer-delilas.md`](../../docs/tooling/randomizer-delilas.md#delilas-party-swap).

## What decides that a module sits here

It is part of the play-as-Delilas feature and it touches a disc. The model
transforms that need no disc are one layer down, in
[`legaia-party-swap`](../party-swap/README.md).

This crate names the crates that own what it uses: the swap kernels, the arts
`record[0]` helpers of [`legaia-arts-patch`](../arts-patch/README.md), the
encoders and arena map of [`legaia-code-hooks`](../code-hooks/README.md), the
portrait swap of [`legaia-texture-replace`](../texture-replace/README.md),
the dialog export / import of [`legaia-translate`](../translate/README.md),
the `DiscPatcher` of [`legaia-disc-patch`](../disc-patch/README.md), and the
audio codecs (`legaia-xa`, `legaia-vab`) the voice passes re-encode with.

The Delilas Challenge dome course and the custom items are a different
feature and live in
[`legaia-delilas-challenge`](../delilas-challenge/README.md); the two are
mutually exclusive on a disc and share no code.

What stays in [`legaia-patcher`](../patcher/README.md): the CLI subcommands
(`delilas-verify`, `delilas-audit`) and the disc-gated oracles
(`crates/patcher/tests/delilas_*_real.rs`, `enemy_anim_mirror_real.rs`,
`nivora_field_real.rs`).

## See also

- [`crates/patcher`](../patcher/README.md) - the patcher this was split from.
- [`crates/party-swap`](../party-swap/README.md) - the model swap kernels.
