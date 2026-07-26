# Shipped-bundle freshness

`site/wasm/` is a **compiled artifact checked into the repository**, and no gate
runs the compiler that produces it. So the sources in the tree and the binary the
browser loads are two separate things that can disagree, and nothing about a
green test run says they agree.

This is not hypothetical. A footstep-SFX fix was committed, verified by unit
test, and reported as shipped while the play page kept playing the old sample -
the bundle predated the fix by four commits. The same class of miss then happened
a second time, on a branch that *had* rebuilt the bundle.

Gate: [`scripts/ci/check-wasm-freshness.py`](../../scripts/ci/check-wasm-freshness.py).
Stamp writer: [`scripts/ci/build-wasm.sh`](../../scripts/ci/build-wasm.sh).

## The committed bundle is a local-testing artifact

Worth knowing before deciding this gate is redundant with CI: the `deploy-pages`
job in `main-ci.yml` runs `wasm-pack build` itself and writes into `site/wasm/`
before publishing, so **the deployed site is always built from source** and never
loads the committed bundle.

What the committed bundle serves is local browsing of `site/` without a
`wasm-pack` toolchain - which is how the play page actually gets play-tested. So
staleness here is not a shipping defect, it is a *verification* defect, and a
sharper one than it first looks: it makes local testing disagree with the
deployed page, and it is the reason a fix can be real, deployed-correct, and
still absent from the build someone is looking at. Both misses that motivated
this gate took that form - a fix that was genuinely in the sources being reported
as done to someone whose bundle predated it.

## Why it needs a gate at all

The bundle's source closure is not the `web-viewer` crate. `legaia-web-viewer`
transitively compiles most of the workspace - the format crates, `engine-core`,
`engine-ui`, `engine-audio` - which is 27 workspace crates. Editing a PROT reader
or an audio kernel therefore changes what the play page does, with nothing in the
diff to suggest the web target is involved.

`engine-render` is deliberately **outside** the closure: it hard-links wgpu, which
is why `engine-ui` exists as the wgpu-free leaf both hosts share. An edit there
is correctly not a staleness signal.

## The two ways of guessing that don't work

Both were tried, and each returned "in sync" for a bundle that was not.

- **File mtimes.** Build output is newer than sources in the normal case, so
  "bundle newer than every source" looks like proof. It isn't: a checkout, a
  `cp`, or a cherry-pick rewrites source mtimes without touching the bundle.
- **`git log` last-touched.** Comparing the commit that last modified
  `site/wasm/` against the commits that last modified its sources looks
  airtight. It fails on rebase: a branch can build the bundle and then be
  rebased onto a different tree, so the bundle is a *descendant* of its build
  inputs in commit order while having compiled against sources that are no
  longer present.

Both failures share a shape - they infer content agreement from history or
timestamps. So the stamp is **content-addressed**: a hash of every source input,
recorded at build time and recomputed on demand.

## What is hashed

Tracked files only, so the answer is reproducible across clones and `target/`
never enters it. The crate list comes from `cargo metadata`'s own resolve graph
rather than a hand-kept list - a hand-kept list's failure mode is a gate that
reports fresh while the bundle is stale, which is worse than no gate.

Excluded on purpose: `*.md`, `tests/`, `benches/`. Retargeting a test must not
read as a stale bundle, or the gate cries wolf on test-only commits and gets
bypassed.

## Running it

```bash
python3 scripts/ci/check-wasm-freshness.py            # warn, exit 0
python3 scripts/ci/check-wasm-freshness.py --strict   # fail, exit 1
scripts/ci/check-wasm.sh --full                       # wasm-pack build + strict
```

Warn-only in pre-commit, by design: the closure is wide enough that plenty of
commits dirty the stamp without shipping anything web-visible, and a hard failure
there would train people to bypass the hook. The strict form is for releases -
and for any moment someone is about to state that a play-page fix is live.

When it reports stale it names the drifted files, because "the bundle is stale"
alone doesn't tell you whether the drift is web-visible.

`site/wasm/SOURCE_STAMP.json` is written by the build. Hand-editing it to quiet
the gate is precisely how a stale bundle ships.
