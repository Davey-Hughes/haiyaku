# Releasing haiyaku

A release publishes from GitHub Actions when a `v<version>` tag reaches the
GitHub mirror: `.github/workflows/publish.yml` checks the tag and publishes
with crates.io's trusted publishing, so no token is stored anywhere. Why it
works this way is in
[the CI publishing design](superpowers/specs/2026-11-27-haiyaku-ci-publishing-design.md).

## Once, before the first release from CI

1. Before adding the trusted publisher in step 2: on GitHub, in the
   repository's Settings, open Environments and add one named `release`, or
   open it if an earlier run created it with no rule. Under Deployment
   branches and tags, choose Selected branches and tags and add a rule with
   Ref type Tag (not the default, Branch) and pattern `v*`. A run on
   anything else, such as a branch picked by mistake when starting a run by
   hand, then gets no crates.io token, whatever that branch's copy of the
   workflow says. Leave required reviewers off: pushing the tag is the
   decision.
2. On crates.io, open haiyaku's Settings, then Trusted Publishing, and add a
   GitHub publisher:

   - repository owner: `Davey-Hughes`
   - repository name: `haiyaku`
   - workflow file: `publish.yml`
   - environment: `release`

   Without it the run fails at "Get a crates.io token".

## Steps

1. Open a PR that raises `version` under `[workspace.package]` in
   `Cargo.toml`, and run `cargo check` so that `Cargo.lock` takes the new
   version too. The workflow publishes with `--locked`, so a lockfile left
   behind fails the release.
2. After the PR merges, tag main and push the tag to Forgejo:

   ```sh
   git fetch origin
   git tag v<version> origin/main
   git push origin v<version>
   ```

3. Forgejo's push mirror carries the tag to GitHub, and the Publish workflow
   runs at https://github.com/Davey-Hughes/haiyaku/actions.

If no run starts:

- Check the tag's name. Only a tag that starts with `v` starts a run, so
  `0.3.0` starts nothing, wherever it is pushed.
- Check the push mirror: the repository's Settings page on Forgejo, under
  Mirror settings, shows its last sync and any error, and has Synchronize
  now. Fix the mirror rather than pushing to GitHub directly: it makes
  GitHub's refs match Forgejo's. GitHub's main has to have reached the
  commit too, or the run refuses at "Commit is on main".
- If GitHub has the tag (https://github.com/Davey-Hughes/haiyaku/tags) and
  main's commit but no run started (GitHub starts none for tags when one
  push carries more than three, which a mirror sync after several tags can
  do), start one by hand: on the Actions tab, open Publish, choose Run
  workflow, and pick the tag `v<version>` under Tags. Look for a run already
  queued first: two runs of one tag leave the second failed at "Publish". A
  branch picked by mistake stops at the environment's rule before any step
  runs ("not allowed to deploy to release"); a `v*` tag goes through the
  same checks as a pushed one, and a tag already published fails at
  "Publish".

A run that failed for a reason fixed since (the trusted publisher missing,
GitHub's main behind) can be run again from its page on the Actions tab.

## When it refuses

Each of these publishes nothing:

- A tag that starts with `v` but is not exactly `v` and the version, such
  as `v0.3` or `v0.3.0-rc1` for version 0.3.0, fails at "Tag matches the
  version".
- A tag on a commit that is not on main fails at "Commit is on main", before
  any crates.io token is asked for.
- A run started by hand on a branch stops before its first step: the
  `release` environment admits only `v*` tags.
- A tag for a version crates.io already has fails at "Publish": cargo, or
  crates.io after the build, refuses a version that is already there.

The checks run from the tagged commit's own copy of the workflow, so they
catch mistakes. Against an edited workflow the gate is who can push tags to
Forgejo or GitHub: keep that, and the credential Forgejo's push mirror uses,
to the owner.

Delete a wrong tag on Forgejo (`git push origin :refs/tags/<tag>`); the
mirror's next sync removes it from GitHub. Then tag the right commit.

## Optional

Once a release from CI has worked, crates.io can be set to accept only
trusted publishing for haiyaku. That stops every API token, the desktop's
included, from publishing it.
