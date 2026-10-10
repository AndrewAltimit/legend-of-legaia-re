# legaia-delilas-party

The disc patcher's play-as-Delilas mod, split out of
[`legaia-patcher`](../patcher/README.md): the playable party replaced by the
Delilas siblings Gi / Lu / Che, while the ravine duels field Vahn / Noa /
Gala. `legaia-patcher` depends on this crate, never the other way round, and
re-exports every module at its old path (`legaia_patcher::delilas_party`,
`legaia_patcher::nivora_field`, ...), so the CLI, the browser patcher and the
disc-gated tests name the same paths they always did.

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
| `nivora_field` | The duel field scene rebuilt with the mapped heroes' field rigs. |

The full mechanism reference is
[`docs/tooling/randomizer.md`](../../docs/tooling/randomizer.md#delilas-party-swap).

## The split line

A module sits here when it closes over only the swap kernels, the arts
`record[0]` helpers of [`legaia-arts-patch`](../arts-patch/README.md), the
cast route and encoders of [`legaia-code-hooks`](../code-hooks/README.md),
the portrait swap of [`legaia-texture-replace`](../texture-replace/README.md),
the dialog export / import of [`legaia-translate`](../translate/README.md)
and the `DiscPatcher` of [`legaia-disc-patch`](../disc-patch/README.md). The
crate root aliases those modules at the `crate::<module>` paths the files
were written against.

What stays in `legaia-patcher`: the CLI subcommands (`delilas-verify`,
`delilas-audit`), the two Delilas mods that reach into the `apply` layer and
the custom-item table - the Delilas Challenge (`delilas_challenge`) and the
Muscle Dome fight (`delilas_dome`) - and the disc-gated oracles
(`crates/patcher/tests/delilas_*_real.rs`, `enemy_anim_mirror_real.rs`,
`nivora_field_real.rs`).

## See also

- [`crates/patcher`](../patcher/README.md) - the patcher this was split from.
- [`crates/party-swap`](../party-swap/README.md) - the model swap kernels.
