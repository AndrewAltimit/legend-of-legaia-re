# Local wasm bundle freshness

`site/wasm/` is **build output and is not committed**. Two things produce it:

- [`scripts/ci/build-wasm.sh`](../../scripts/ci/build-wasm.sh), for browsing
  `site/` locally (~9 min cold).
- The `deploy-pages` job in `main-ci.yml`, which runs its own `wasm-pack build`
  into that path before publishing. **The deployed site is always built from
  source** and has never loaded a committed bundle.

So there is nothing to keep in sync in the repository. What remains is a question
about your working copy: *does the bundle I built still match the code I am
looking at?*

Checker: [`scripts/ci/check-wasm-freshness.py`](../../scripts/ci/check-wasm-freshness.py).
Stamp writer: `build-wasm.sh`. Neither is wired into a hook - there is no
committed artifact to gate, and the stamp is untracked local state.

## Why the question needs a tool

The bundle's source closure is not the `web-viewer` crate. `legaia-web-viewer`
transitively compiles most of the workspace - the format crates, `engine-core`,
`engine-ui`, `engine-audio` - which is 27 workspace crates. Editing a PROT reader
or an audio kernel therefore changes what the play page does, with nothing in the
diff to suggest the web target is involved.

`engine-render` is deliberately **outside** the closure: it hard-links wgpu, which
is why `engine-ui` exists as the wgpu-free leaf both hosts share. An edit there is
correctly not a staleness signal.

This has real consequences. A fix can be correct in the sources, correct on the
deployed page, and absent from the build someone is testing - which is how a fix
gets reported as live twice while the reporter keeps seeing the old behaviour. The
symptom is not a wrong fix; it is a right fix nobody is running.

## The two ways of guessing that don't work

Both were tried, and each returned "in sync" for a bundle that was not.

- **File mtimes.** Build output is newer than sources in the normal case, so
  "bundle newer than every source" looks like proof. It isn't: a checkout, a
  `cp`, or a cherry-pick rewrites source mtimes without touching the bundle.
- **`git log` last-touched.** Comparing the commit that last built the bundle
  against the commits that last modified its sources looks airtight. It fails on
  rebase: a branch can build the bundle and then be rebased onto a different
  tree, so the bundle is a *descendant* of its build inputs in commit order while
  having compiled against sources that are no longer present.

Both failures share a shape - they infer content agreement from history or
timestamps. So the stamp is **content-addressed**: a hash of every source input,
recorded at build time and recomputed on demand.

## What is hashed

Tracked files only, so the answer is reproducible across clones and `target/`
never enters it. The crate list comes from `cargo metadata`'s own resolve graph
rather than a hand-kept list - a hand-kept list's failure mode is a checker that
reports fresh while the bundle is stale, which is worse than no checker.

Excluded on purpose: `*.md`, `tests/`, `benches/`. Retargeting a test must not
read as a stale bundle.

## Running it

```bash
scripts/ci/build-wasm.sh                              # build + stamp
python3 scripts/ci/check-wasm-freshness.py            # warn, exit 0
python3 scripts/ci/check-wasm-freshness.py --strict   # fail, exit 1
scripts/ci/check-wasm.sh --full                       # wasm-pack build + strict
```

Run it before believing a locally served play page, and especially before telling
anyone a play-page fix is live. When stale it names the drifted files, because
"the bundle is stale" alone doesn't say whether the drift is web-visible.

## What a stale engine against fresh pages costs

The stamp answers "is the bundle on disk built from this tree?". A *served*
bundle has a second staleness axis the stamp cannot see: the browser's own HTTP
cache. `site/wasm/legaia_web_viewer.js` and `legaia_web_viewer_bg.wasm` are
fetched from URLs that never change, while every `js/*.js` beside them is
content-busted by `_gen.py` (see
[`site-shell.md`](site-shell.md#script-cache-busting-is-content-addressed)). So
a rebuilt engine can reach a returning browser late, or not at all, while the
page scripts around it update immediately.

How long "late" is comes from the response headers, and the two hosts differ:

| Host | Headers on `wasm/*` | Stale window after a rebuild |
|---|---|---|
| `python3 -m http.server` (local) | `Last-Modified` only, no `Cache-Control`, no `ETag` | Chrome's heuristic freshness - 10% of the file's age, so **hours** for a bundle last built days ago |
| GitHub Pages | `Cache-Control: max-age=600` + `ETag` | **10 minutes**, then revalidated |

What that skew costs in practice was measured rather than assumed, in both
directions, against an engine 76 commits apart: current pages on the old
engine, and old pages on the new engine. Both boot, both render textured, on
SwiftShader and on a real driver. The reason is that every page-side call into
an engine export added by that span is `typeof rt.x === 'function'`-guarded, so
a missing export degrades a feature instead of throwing. That is a property of
the current call sites, not a guarantee: an unguarded call to a newly added
export would turn the same window into a hard failure.

## Why the bundle is no longer committed

It was, for 87 commits. A 4.6 MB binary rebuilt that often came to roughly
**223 MB - about 86% of the repository's history** - for an artifact the deployed
site never reads. Its only benefit was letting a contributor browse `site/` with
no Rust toolchain, against a standing cost in clone size and a recurring one in
false "it's fixed" reports.

Note what the removal does and does not fix. It stops the history growing and
removes the *committed* staleness hazard. It does **not** remove the local one -
that is what the checker above is for. Deleting the artifact was the better fix;
building a gate for it first was treating the symptom.
