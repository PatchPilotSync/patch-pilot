# Security

## Reporting a problem

Please report security problems privately, not in a public issue. Use
**Report a vulnerability** on this repo's
[Security tab](https://github.com/PatchPilotSync/patch-pilot/security). We'll
reply as soon as we can and credit you in the release notes if you'd like.

Only the latest release is supported; fixes ship as a new version that
installed copies offer to install.

## What Patch Pilot handles

- **Scan results** (lists of plug-ins, versions and folders) are shared between
  linked machines through the relay in [`relay/`](relay/README.md), encrypted
  on the machine with the rig key before upload. The relay stores bytes it
  can't read and keeps only a hash of each rig's access token.
- **Your own files** (templates, scripts, presets, settings) travel through a
  folder or service you choose: a cloud folder, your home network (encrypted
  with the rig key) or a private GitHub repo of your own.
- **Never handled:** plug-in binaries, installers or sample libraries leaving
  the machine, and iLok or vendor account credentials.
