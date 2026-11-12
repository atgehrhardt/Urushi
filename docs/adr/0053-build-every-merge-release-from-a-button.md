# 0053. Build the apps on every merge to main; release only from a button that tags Cargo.toml's version

- **Status:** Accepted
- **Date:** 2026-11-12
- **Supersedes:** the tag-push trigger of [0032](0032-tagged-macos-releases.md) and the manual dry runs of [0040](0040-release-android-apks-signed-with-a-repository-secret-key.md)

## Context
Building and releasing were one workflow. `release.yml` ran when a `vX.Y.Z` tag was pushed,
or as a dry run when started by hand. Merges to `main` built the iOS and Android apps
inside CI, but never the macOS app. So the only way to get a macOS build of `main` was to
release it, or to build it locally. Releasing took three hand steps: a version-bump pull
request, merging it, and pushing a matching tag. Only the docs asked that a tag go only on
a commit where CI had passed; nothing checked it.

UR-70 asks to separate builds from releases. Every merged pull request should build the
apps automatically, and releases should stay manual.

## Decision
- **One reusable workflow builds the apps** (`.github/workflows/build-apps.yml`,
  `workflow_call`). It holds the macOS, Android and iOS jobs and their checks, which used
  to be in `release.yml`. Builds and releases therefore make the same files in the same way.
- **Every push to `main` builds them** (`build.yml`, "Build"). It uploads them as workflow
  artifacts named `Urushi-X.Y.Z-<commit>-…`, kept 30 days, along with the iOS Simulator app.
  These builds are never published. Their APK is signed with the debug key, never the
  release key. Pull requests that change the build scripts or workflows run the same build.
- **A release is started only by Actions → Release → Run workflow on `main`.** The
  workflow reads `[workspace.package].version` from `Cargo.toml`. It refuses if:
  - the run isn't on `main`;
  - `vX.Y.Z` is already tagged;
  - CI for the commit hasn't finished successfully.

  Otherwise it builds through the same reusable workflow with the release key and
  publishes. `gh release create --target <commit>` creates the tag. Pushing a tag no
  longer triggers anything.
- Pull requests keep their CI app builds (`ios`, `android`). On `main` those two jobs no
  longer run in CI, because "Build" covers them.

## Alternatives considered
- **A rolling pre-release of `main`:** easier to find than artifacts, but it fills the
  Releases page with untested builds. It also invites a mix-up with the real release that
  the macOS updater reads (ADR 0049).
- **Keep the tag push as the trigger:** it leaves the CI check to habit, and a pushed tag
  that fails a check must be deleted by hand. The button can refuse before anything exists.
- **A button that takes a version and commits the bump itself:** the workflow would need
  to push to `main`, and a typed version could disagree with the code. That disagreement
  is the objection ADR 0032 raised against a button. Reading the version from `Cargo.toml`
  avoids both problems.
- **Publish the artifacts that `main`'s build already made:** faster, but the release
  would then depend on a finished build of exactly that commit, and on artifacts that
  expire. The merge build would also need the release key. A rebuild through the same
  reusable workflow gives identical files.

## Consequences
- A release takes two steps: merge a version bump, then press Run workflow once CI is
  green. The `version` job no longer compares a tag with `Cargo.toml`, because the tag is
  made from it.
- There is no hand-started dry run any more. A build of `main` (or of a pull request that
  touches packaging) builds the same files, except that its APK is debug-signed. The
  release key is first used by a real release.
- Each merge spends runner minutes on two macOS jobs and one Linux build job, in addition
  to CI.
- A tag made with the workflow token doesn't trigger other workflows. Nothing listens
  for tags, so that's harmless, but a future tag-triggered workflow wouldn't run on
  releases.
