# Publishing haiyaku from CI

Approved by the owner on 2026-11-27 ("yes, no click"), after their choice of
GitHub Actions on 2026-11-24.

## Why

haiyaku 0.1.0 and 0.2.0 were published by running `cargo publish -p haiyaku`
on the owner's desktop with an API token kept there. The owner wants a
release to publish from CI instead, so that what reaches crates.io is built
from a commit on main and no long-lived token is needed.

## Goals, non-goals

- A release is one act by the owner: pushing a `v<version>` tag on a commit
  of main. CI publishes that commit to crates.io.
- No crates.io token stored anywhere: crates.io's trusted publishing trades
  the CI job's identity for a token that lasts minutes.
- A tag that does not match the crate's version, or that names a commit off
  main, publishes nothing.
- Not a goal: bumping the version, writing a changelog or making a release
  page. The version bump stays an ordinary PR.
- Not a goal: running the tests again. Main only takes a commit through a PR
  whose Forgejo CI passed, and `cargo publish` builds the package before it
  uploads.

## Constraints

- The forge is Forgejo, and Forgejo pull requests and CI are where haiyaku
  is built. crates.io's trusted publishing accepts only GitHub Actions and
  GitLab CI, so the publish job cannot run on Forgejo without a stored token.
- Forgejo push-mirrors the repository to github.com/Davey-Hughes/haiyaku:
  on 2026-11-27 GitHub's main was Forgejo's main (9dc5fce). Forgejo's push
  mirror pushes with `--mirror` and a `+refs/tags/*` refspec, so it carries
  tags and removes from GitHub the refs Forgejo no longer has (read in
  Forgejo's source). Neither side had a tag yet, so no tag has been seen to
  arrive.
- Forgejo reads workflows from `.forgejo/workflows/` and, when that folder
  exists, ignores `.github/workflows/`. haiyaku has `.forgejo/workflows/`,
  so a file in `.github/workflows/` runs on GitHub only.

## Approaches

1. **GitHub Actions with trusted publishing (picked).** A workflow on the
   GitHub mirror publishes on a tag. No token is stored; it relies on the
   mirror carrying the tag.
2. **Forgejo Actions with a token secret.** Everything stays on Forgejo, but
   a crates.io token lives in Forgejo's secrets, which is what the owner
   wants to be rid of.
3. **Keep publishing by hand.** Nothing to build; the token stays on the
   desktop and a release can be built from a tree that is not main's.

The owner picked 1 on 2026-11-24.

## Design

- **File:** `.github/workflows/publish.yml`.
- **Trigger:** a pushed tag matching `v*`, or a run started by hand
  (`workflow_dispatch`) on such a tag, for a tag push that started no run. A
  hand run on anything but a `v*` tag stops at the release environment's tag
  rule before any step; one on a `v*` tag goes through the same checks as a
  pushed one, and one on a tag already published fails at "Publish". The
  hand run is the owner's choice of 2026-11-27, after reviews found every
  way of re-sending a tag through the mirror had a trap.
- **Job:** on a GitHub-hosted Ubuntu runner, in the GitHub environment
  `release`, with `id-token: write` and `contents: read` and nothing else.
  The environment has no required reviewer: the owner's tag push is the
  decision (the owner's choice, "no click"). Its deployment rule admits only
  tags matching `v*`, so a run started by hand on a branch gets no token
  even if that branch's copy of the workflow has lost its checks. A `v*` tag
  still runs its own commit's copy, so the checks guard against mistakes;
  against an edited workflow the gate is who can push tags to Forgejo or
  GitHub, which is to stay the owner alone. That includes the credential
  Forgejo's push mirror uses, which only the owner is to hold (on 2026-11-27
  GitHub's only collaborator was the owner). The toolchain comes from
  `rust-toolchain.toml`, as everywhere else.
- **Steps:**
  1. Check out the tagged commit with full history.
  2. Refuse unless the run's ref is the tag `v` followed by the `haiyaku`
     package's version as cargo reads it (the full ref, so a branch of that
     name is refused).
  3. Refuse unless the tagged commit is on `origin/main`.
  4. Trade the job's identity for a crates.io token with
     `rust-lang/crates-io-auth-action`.
  5. `cargo publish -p haiyaku --locked` with that token.
- **Actions are pinned by commit SHA**, with the release they are named in a
  trailing comment, because a moved tag would otherwise change what runs
  with publishing rights.
- **A release** (written up in `docs/releasing.md`): a PR bumps
  `[workspace.package] version` and `Cargo.lock`; after it merges, the
  owner tags main's merge commit `v<version>` and pushes the tag to
  Forgejo; the mirror carries it to GitHub and the job publishes. When no
  run starts, the owner checks the tag's name and the mirror's sync on
  Forgejo (the mirror makes GitHub's refs match Forgejo's, so nothing is
  pushed to GitHub directly), and starts a run by hand on the tag if GitHub
  has it.

## The owner's part

- Once, in this order: on GitHub, add the environment `release` (or open it
  if an earlier run created it with no rule) with a deployment rule for tags
  (Ref type Tag) matching `v*` and no required reviewer; then on crates.io,
  haiyaku, Settings, Trusted Publishing, add GitHub with owner
  `Davey-Hughes`, repository `haiyaku`, workflow `publish.yml`, environment
  `release`. With the rule in place before the publisher, no run gets a
  token without it.
- Optional, after the first publish from CI: crates.io's "require trusted
  publishing" for haiyaku, which turns off publishing with the desktop's
  token.

## Left to the builder

- Which release of each action to pin, and the shell of the two checks.
- What `docs/status.md` says about it (a line under "Decided").
- Checking the workflow with `actionlint` (and `zizmor` if it is there)
  before the PR. The workflow's first real run is the next release, since
  0.2.0 is already on crates.io.
