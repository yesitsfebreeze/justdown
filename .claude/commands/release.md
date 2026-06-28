---
description: Cut a release — write the CHANGELOG entry from the commits since the last tag, then gate, bump every manifest, commit, tag, and push so CI builds the binaries.
argument-hint: <major.minor.patch>
allowed-tools: Bash(git:*), Bash(jd:*), Bash(just:*), Read, Edit, Write
---

Release version **$ARGUMENTS** (must be `MAJOR.MINOR.PATCH`; if missing or
malformed, stop and ask for it).

This is high-danger and outward-facing: it pushes a tag that triggers the release
CI to publish prebuilt binaries. Confirm the version with the user before the
final push if there is any doubt.

Do these steps in order:

1. **Collect what happened.** Find the last tag and the commits since it:
   `git describe --tags --abbrev=0`, then
   `git --no-pager log <lasttag>..HEAD --oneline`.

2. **Write the CHANGELOG entry.** Edit `CHANGELOG.md`: a
   `## [$ARGUMENTS] - <today, YYYY-MM-DD>` section directly under
   `## [Unreleased]`, summarizing the commits grouped into **Added / Changed /
   Fixed / Removed** (omit empty groups). Write for a human reader — group related
   commits, drop noise (`chore: bump version`, merge commits). Fold any existing
   `[Unreleased]` notes in. **If a `[$ARGUMENTS]` section already exists, update it
   in place — never add a second one.** Update the compare-link footer: add
   `[$ARGUMENTS]: …/compare/<lasttag>...v$ARGUMENTS` and re-point `[Unreleased]`
   to `…/compare/v$ARGUMENTS...HEAD`.

3. **Release.** Run the release recipe — it gates (cargo build + test), syncs every
   manifest to the version via `@tools/version`, commits (CHANGELOG + manifests),
   tags `v$ARGUMENTS`, and pushes with `--follow-tags`:

   ```sh
   jd get tools_release --justfile | just --justfile - release $ARGUMENTS
   ```

4. **Report** the pushed tag and that the release CI
   (`.github/workflows/release.yml`) now builds the prebuilt binaries for it.
   Return the tag and the CHANGELOG path — nothing else.
