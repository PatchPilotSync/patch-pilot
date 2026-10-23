# Releasing the app

Installed copies of Patch Pilot check for a new version each time they start.
When one exists they show what changed and install it only if the user clicks
**Install and restart** (decision #32).

## One-time setup

1. **Releases repo.** Create the public repo `PatchPilotSync/patch-pilot-releases`
   with a README (the release tags need a commit to point at). It only ever holds
   installers and release notes, never source.
2. **Token.** Create a fine-grained personal access token with access to
   `patch-pilot-releases` only, permission *Contents: Read and write*. Save it in
   this repo as the Actions secret `RELEASES_TOKEN`.
3. **Update signing key.** Save the private key and its password as the Actions
   secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
   The matching public key is `plugins.updater.pubkey` in
   `app/src-tauri/tauri.conf.json`. Keep a copy of both in a password manager:
   if the key is lost, installed apps can't update and have to be reinstalled.
   To replace it, run `npx tauri signer generate` in `app/` and update the
   secrets and the public key together.

## Each release

1. Bump the version in `app/src-tauri/tauri.conf.json`, `app/src-tauri/Cargo.toml`
   (then `cargo check` to update `Cargo.lock`) and `app/package.json` (and
   `package-lock.json`, with `npm version <x.y.z> --no-git-tag-version`).
2. Add a `## <x.y.z> - <date>` section to `app/CHANGELOG.md`. This is what users
   see in the update prompt, so write it for them: what's new, what's fixed.
   Headings and `-` bullets are shown as such.
3. Merge that PR, then tag the merge commit and push the tag:
   `git tag v<x.y.z> && git push origin v<x.y.z>`.
4. The **Release** workflow checks the versions match the tag, builds macOS
   (Apple silicon and Intel) and Windows, and publishes the release with
   `latest.json`. Apps pick it up at their next launch.

If a build job fails, re-run it, then re-run **Publish release**. The release
stays a draft (invisible to the updater) until every platform is in.

## Not done yet

macOS builds aren't signed with a Developer ID or notarized, and Windows builds
aren't code-signed, so the *first* install shows a Gatekeeper or SmartScreen
warning. Updates themselves are verified with the update signing key above.
