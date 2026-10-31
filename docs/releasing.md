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
4. **Apple Developer ID signing and notarization** (optional; decision #115).
   Without it Mac builds are ad-hoc signed: macOS calls a downloaded copy
   "damaged" until you run `xattr -cr`, and may ask for Documents again after
   each update. With it, the app opens with no warning. On a Mac:
   1. Enroll in the [Apple Developer Program](https://developer.apple.com/programs/enroll/)
      ($99 a year; as an individual, or as an organization if one exists, which
      needs a D-U-N-S number). Wait for Apple's confirmation email.
   2. **Certificate.** In Keychain Access choose *Keychain Access > Certificate
      Assistant > Request a Certificate From a Certificate Authority…*, enter
      your email, choose *Saved to disk*. At
      [Certificates](https://developer.apple.com/account/resources/certificates/add)
      choose **Developer ID Application** (profile type *G2 Sub-CA*), upload the
      request, download the `.cer` and double-click it. In Keychain Access,
      under *My Certificates*, right-click *Developer ID Application: <name>
      (<team>)* (expand it: the private key must be under it), *Export…*, save
      as `.p12` with a strong password. Then in Terminal
      `base64 -i Certificates.p12 | pbcopy`.
   3. **API key for notarization.** At
      [App Store Connect > Users and Access > Integrations > Team Keys](https://appstoreconnect.apple.com/access/integrations/api)
      generate a key with access **Developer**. Note the *Issuer ID* above the
      table and the key's *Key ID*, and download the `AuthKey_<id>.p8` file
      (Apple lets you download it once).
   4. In this repo's **Settings > Secrets and variables > Actions**, add:

      | Secret | Value |
      | --- | --- |
      | `APPLE_CERTIFICATE` | the base64 text from step 2 |
      | `APPLE_CERTIFICATE_PASSWORD` | the `.p12` password |
      | `APPLE_API_ISSUER` | the Issuer ID |
      | `APPLE_API_KEY` | the Key ID |
      | `APPLE_API_PRIVATE_KEY` | the whole contents of the `.p8` file, including the `BEGIN`/`END` lines |

   Keep the `.p12`, its password and the `.p8` in a password manager and
   delete the loose copies. The next release signs both Mac builds with the
   certificate (hardened runtime), sends them to Apple to be notarized,
   staples the ticket, and fails if any of that didn't happen. If only some
   of the secrets are set, Mac builds fail rather than quietly going out
   unsigned. A Developer ID certificate lasts five years.

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

Windows builds aren't code-signed, so the *first* install shows a SmartScreen
warning; Mac builds do too until step 4 above is done. Updates themselves are
verified with the update signing key above. The `.dmg` itself isn't notarized
(the app inside it is, and stapled), which is enough for Gatekeeper.
