# L10 handoff - `scripts/ci/port-catalog-ignore.toml`

Lane L10 (docs for the `801e****`/`801f****` overlay band + the whole SCUS
band) did not edit the ignore list. Two findings for whoever owns that file.

## 1. No additions needed

Every SCUS address in L10's worklist slice is **already** on the ignore list -
all 47 of them, across the `[bios]`, `[libgte]`, `[libgpu]`, `[libsnd]` and
no-op-stub sections. Nothing to add.

L10 also re-read the ignore list looking for the "real portable game function
closed by mistake" class. It found none in its slice. The two closest calls
were `80017d98` / `8001ad38`, which are game-authored rather than library code -
but the existing reasons already say so correctly (the dev/CONFIG monospaced
text path behind `FUN_8001AA68`), and the clean-room engine renders text
through the `legaia-font` atlas instead, so the ignore is right.

## 2. Three reason strings are wrong, and two of them swap a pair

These are label errors, not scope errors - the addresses stay ignored either
way. They are worth fixing because the same two labels had propagated into
`docs/reference/functions.md` and `docs/subsystems/battle-action.md`, where
L10 has now corrected them; leaving the toml as-is re-seeds the error.

Evidence is the veneer body itself - each is `li t2,0xA0; jr t2; li t1,N`, so
`N` is read directly off the third instruction.

| Address | Current reason | Should be | Why |
|---|---|---|---|
| `80056798` | `srand (BIOS A0 0x2F)` | `rand (BIOS A0 0x2F)` | The vector number is right, the name is not. A0 `0x2F` is `rand`; `srand` is A0 `0x30`. Callers use the return value - e.g. `FUN_801F45A4` does `jal 0x80056798` then `andi v0,v0,0x7` - and `srand` returns nothing. `battle-action.md` treats this address as the combat RNG throughout, which is the correct reading. |
| `80057014` | `rand (BIOS A0 0x2E)` | `memchr (BIOS A0 0x2E)` | The mirror error. A0 `0x2E` is `memchr`; naming it `rand` gives the corpus two different addresses both called `rand`. |
| `80056758` | `strncpy (BIOS A0 0x19)` | `strcpy (BIOS A0 0x19)` | A0 `0x19` is `strcpy`; `strncpy` is A0 `0x1A`. Consistent with its use as the party-name / scene-name copy thunk. |

Cross-check that the A-table reading above is the same one the rest of the file
already assumes: `8005acd8` = `GPU_cw` (A0 `0x49`), `8005bbe8` = `FlushCache`
(A0 `0x44`), `80056778` = `bzero` (A0 `0x28`), `80056768` = `strlen`
(A0 `0x1B`) are all present and all agree.

## 3. One reason string is imprecise

`8003cc88` is listed as `no-op stub (jr ra; nop)`. The body is `jr ra` with
`clear v0` in the delay slot - it returns **zero**, so a caller that branches on
the result always takes the false arm. Same ignore verdict, but the distinction
matters if anyone ever ports a caller.
