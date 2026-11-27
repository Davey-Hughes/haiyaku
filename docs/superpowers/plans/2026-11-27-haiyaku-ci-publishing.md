# Publishing from CI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a pushed `v<version>` tag on a commit of main publishes haiyaku to crates.io from GitHub Actions, with no stored token.

**Architecture:** one workflow file under `.github/workflows/`, which Forgejo ignores and the GitHub mirror runs; two shell checks guard `cargo publish`; a short release how-to in `docs/`.

**Tech Stack:** GitHub Actions, `rust-lang/crates-io-auth-action`, cargo.

**Spec:** [the CI publishing design](../specs/2026-11-27-haiyaku-ci-publishing-design.md)

## Global Constraints

Amended after the final review (the spec is current where they differ): the version check compares the full ref with `refs/tags/v<version>`, so a branch of that name is refused; a hand run (`workflow_dispatch`) on the tag is a second trigger; the `release` environment admits only `v*` tags.

- Workflow path: `.github/workflows/publish.yml`; crates.io's trusted publisher is configured with exactly that file name and the environment `release`.
- Trigger: `push` of tags matching `v*` (and `workflow_dispatch`, by the amendment above).
- Job permissions: `id-token: write`, `contents: read`, nothing else; environment `release`, no required reviewer.
- Every `uses:` is pinned by full commit SHA with the release name in a trailing comment.
- No test run in the workflow; `cargo publish -p haiyaku --locked` (it verifies the package by building it).
- Comments say why only (the repository's CLAUDE.md "Comments").
- Nothing on the desktop may write to `$HOME`: tool caches go through XDG locations (`uvx` already does).

## Review Focus

1. An annotated tag: `GITHUB_SHA` may name the tag object rather than the commit; the on-main check must test the checked-out commit (`HEAD`), and pass for an annotated tag on main.
2. A tag whose name differs from the version by form, not value (`v0.2`, `0.2.0`, `v0.2.0-rc1` against version 0.2.0): refused.
3. A tag on a commit that is not on main (a PR branch's commit): refused before any token is minted.
4. A shallow clone would make the ancestry check fail for the wrong reason: checkout fetches full history, and a failed `git merge-base` refuses (fails closed).
5. A tag for a version crates.io already has: the job fails at `cargo publish` with crates.io's message and publishes nothing; nothing to build, but releasing.md says so.

---

### Task 1: The publish workflow, its how-to and its status line

**Files:**
- Create: `.github/workflows/publish.yml`
- Create: `docs/releasing.md`
- Modify: `docs/status.md` (line 3's "Updated" date to 2026-11-27; one bullet under "## Decided", after the bullet that begins "haiyaku is published on crates.io")

**Interfaces:**
- Consumes: `[workspace.package] version` in `Cargo.toml` (0.2.0 now), read through `cargo metadata --no-deps --format-version 1` for the package named `haiyaku`.
- Produces: a job named `publish` in a workflow named `Publish`; the steps in this order: checkout, "Tag matches the version", "Commit is on main", the auth action (step id `auth`), "Publish".

- [ ] **Step 1: Write the two checks as a scratch script and watch them refuse**

In the session scratchpad (not the worktree), write `checks.sh` holding the two checks exactly as they will sit in the workflow, reading `GITHUB_REF_NAME` from the environment and testing `HEAD`:
- version check: `version=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "haiyaku") | .version')`; refuse with a message naming both values unless `"$GITHUB_REF_NAME" = "v$version"` and `version` is not empty (superseded by the amendment above: the shipped check compares `$GITHUB_REF` with `refs/tags/v$version`).
- main check: `git fetch --no-tags origin main` is the workflow's job, so the script only runs `git merge-base --is-ancestor HEAD origin/main` and refuses with a message on any non-zero exit.

Run it from a scratch checkout (`git worktree add --detach` in the scratchpad) with `set -eu`:
- `GITHUB_REF_NAME=v0.2.1` at origin/main: refused (version).
- `GITHUB_REF_NAME=v0.2`, then `0.2.0`, then `v0.2.0-rc1`: each refused.
- `GITHUB_REF_NAME=v0.2.0` at the design commit of branch `ci-publish` (off main): refused (main).
- `GITHUB_REF_NAME=v0.2.0` at origin/main (9dc5fce): passes both.
- An annotated tag made in the scratch checkout on origin/main, checked out by tag name: passes both.

Expected: five refusals with their messages, two passes. Remove the scratch worktree with `git worktree remove` afterwards.

- [ ] **Step 2: Write `.github/workflows/publish.yml`**

`runs-on: ubuntu-latest`, `timeout-minutes: 15`, the permissions and environment above. Checkout with `fetch-depth: 0` and `persist-credentials: false`; the main check step runs `git fetch --no-tags origin main` first (the remote is public, so no credential is needed). The auth step is `rust-lang/crates-io-auth-action` with `id: auth`; Publish runs `cargo publish -p haiyaku --locked` with `CARGO_REGISTRY_TOKEN: ${{ steps.auth.outputs.token }}`. Pin `actions/checkout` and `rust-lang/crates-io-auth-action` to the commit SHAs of their latest releases, read with `gh api repos/<owner>/<repo>/git/ref/tags/<tag>` (dereference an annotated tag's object to its commit). Comments: why Forgejo never runs this file, why no tests run, why actions are pinned by SHA, why `HEAD` rather than `GITHUB_SHA`.

- [ ] **Step 3: Lint it**

Run: `uvx actionlint-py .github/workflows/publish.yml` (actionlint runs shellcheck on the `run:` blocks) and `uvx zizmor .github/workflows/publish.yml`.
Expected: actionlint prints nothing and exits 0; zizmor reports no finding above "informational", or each one is answered in the step's comment or in the report.

- [ ] **Step 4: Write `docs/releasing.md`**

The release steps from the design's "A release" bullet, numbered: the version-bump PR (`Cargo.toml`'s `[workspace.package] version` and `Cargo.lock`); after it merges, `git fetch origin && git tag v<version> origin/main && git push origin v<version>`; watching the run at https://github.com/Davey-Hughes/haiyaku/actions; what to check when no run starts (the spec: every fix goes through Forgejo); what a refused or duplicate tag looks like (Review Focus 2, 3, 5). Then "Once" (the crates.io trusted publisher, values from the Global Constraints) and "Optional" (require trusted publishing). Link the design.

- [ ] **Step 5: Update `docs/status.md`**

Date on line 3 to 2026-11-27. The Decided bullet: a release publishes from GitHub Actions on a `v<version>` tag of a commit on main, through crates.io's trusted publishing, with the steps in [releasing.md](releasing.md). No O-number opens or closes.

- [ ] **Step 6: Check the tree and commit**

Run: `git status --short` (expected: the three files only), `scripts/check-all.sh` is not needed (no Rust changed).
Commit: `git add .github/workflows/publish.yml docs/releasing.md docs/status.md && git commit -m "Publish haiyaku from GitHub Actions on a version tag, with trusted publishing"`.
