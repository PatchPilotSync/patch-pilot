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
3. **Update signing key.** On a founder's own computer (never in a cloud
   session or chat), in `app/` after `npm ci`, run
   `npx tauri signer generate -w ~/.tauri/patchpilot.key` and choose a password.
   Save the contents of `~/.tauri/patchpilot.key` and the password as the Actions
   secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
   Put the contents of `~/.tauri/patchpilot.key.pub` in `plugins.updater.pubkey`
   in `app/src-tauri/tauri.conf.json`. Keep the key and password in a password
   manager: if they're lost, installed apps can't update and have to be
   reinstalled.

## Each release

1. Bump the version in `app/src-tauri/tauri.conf.json`, `app/src-tauri/Cargo.toml`
   (then `cargo check` to update `Cargo.lock`) and `app/package.json` (and
   `package-lock.json`, with `npm version <x.y.z> --no-git-tag-version`).
2. Add a `## <x.y.z> - <date>` section to `app/CHANGELOG.md`. This is what users
   see in the update prompt, so write it for them: what's new, what's fixed.
   Headings and `-` bullets are shown as such.
3. Merge that PR. Then on GitHub open **Actions**, choose **Release**, click
   **Run workflow** (branch `main`) and confirm. It tags the commit `v<x.y.z>`
   once the release is out. Pushing a `v<x.y.z>` tag yourself works too.
4. The **Release** workflow checks the versions match the tag, builds macOS
   (Apple silicon and Intel) and Windows, and publishes the release with
   `latest.json`. Apps pick it up at their next launch.

If a build job fails, re-run it, then re-run **Publish release**. The release
stays a draft (invisible to the updater) until every platform is in.

## Not done yet

macOS builds aren't signed with a Developer ID or notarized, and Windows builds
aren't code-signed, so the *first* install shows a Gatekeeper or SmartScreen
warning. Updates themselves are verified with the update signing key above.
