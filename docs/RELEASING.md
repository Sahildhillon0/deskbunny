# Releasing DeskBunny

Releases are published by `.github/workflows/release.yml` straight to the
repo's **Releases** page, where anyone can download the installers.

It runs every time the `CI` workflow finishes green on `main`, and **every run
ships a new version**: the patch number goes up by one (`1.0.0` → `1.0.1`).
Nothing to bump by hand.

## Cut a release

Push to `main`. When CI passes, the `Release` workflow:

1. reads the latest published release (say `v1.0.4`) and picks the next
   version: `1.0.5`;
2. runs `scripts/bump-version.sh 1.0.5` and pushes the result to `main` as
   `release: v1.0.5 [skip ci]` (so **pull before your next commit**);
3. creates a draft release `DeskBunny v1.0.5`, builds the firmware once and
   installers on Linux, Windows and macOS, and attaches them plus the
   `.hex`/`.bin` images, `manifest.json` and the updater's `latest.json`;
4. publishes the release as **Latest**, which creates the `v1.0.5` tag. Apps
   already installed see it under **Check for updates**.

Bigger steps go in the **first line** of the commit message (only the head
commit of a push counts):

| First line contains | 1.0.4 becomes |
|---|---|
| nothing | `1.0.5` |
| `[minor]` | `1.1.0` |
| `[major]` | `2.0.0` |
| `[skip release]` | no release for this push |

You can also run it from the Actions tab (`Release` > `Run workflow`) and pick
`patch`, `minor` or `major`. A version set higher by hand with
`scripts/bump-version.sh X.Y.Z` is released as is.

If any build fails, the draft stays hidden and the bump commit stays on
`main`. Fix the problem and push again: the next run releases that same
version and reuses the draft.

If two pushes land while CI runs, only the newest commit is released.

To rebuild an already published version (for example after adding a signing
secret), delete that release and its tag on GitHub, then run the workflow by
hand.

## In-app updates

The app checks `releases/latest/download/latest.json` on launch and from the
download button in the library header. `latest.json` and the `.sig` files are
made only when `TAURI_SIGNING_PRIVATE_KEY` is set; the matching public key is
`plugins.updater.pubkey` in `app/src-tauri/tauri.conf.json`. Lose the private
key and installed apps can no longer update themselves: keep the copy in
`~/.tauri/deskbunny.key` (and its `.password`) backed up somewhere safe.

The release workflow turns updater bundles on with
`--config src-tauri/tauri.updater.conf.json`, so CI and local builds without
the key still work (the `TAURI_CONFIG` environment variable does not do it).
To build signed bundles locally:

```sh
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/deskbunny.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat ~/.tauri/deskbunny.key.password)"
cd app && npm run tauri build -- --config src-tauri/tauri.updater.conf.json
```

Self-update works for the Windows installers, the macOS app and the Linux
AppImage. A `.deb`/`.rpm` install is updated by installing the new package;
when the in-app install fails the dialog links to the Releases page.

## Artifacts

| OS | Runner | Files on the release |
|---|---|---|
| Windows | `windows-latest` | `.msi` (WiX) and `-setup.exe` (NSIS) |
| macOS | `macos-latest`, universal (Apple Silicon + Intel) | `.dmg` (plus `.app.tar.gz` when updater artifacts are enabled) |
| Linux | `ubuntu-22.04` (older glibc, so it runs on more distros) | `.AppImage`, `.deb`, `.rpm` |
| Firmware | `ubuntu-latest` | `*.hex`, `*.bin`, `manifest.json` |

Updater signatures (`.sig`) are produced only when
`TAURI_SIGNING_PRIVATE_KEY` is set.

Every push to `main` and every pull request also builds the same installers
(unsigned) in `ci.yml`. They are attached to the workflow run as artifacts
`deskbunny-linux`, `deskbunny-windows`, `deskbunny-macos-universal` and
`firmware-dist`. Installers are kept for 7 days and `firmware-dist` for 3 days
(artifact storage quota); download what you need, or use a release.

## Code signing (optional)

Builds succeed unsigned when these secrets are absent. Add them under
Settings > Secrets and variables > Actions. Secrets are not available to pull
requests from forks, so CI builds are always unsigned.

| Secret | Used for |
|---|---|
| `APPLE_CERTIFICATE` | base64 of the Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | password of that `.p12` |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Name (TEAMID)` |
| `APPLE_ID` | Apple ID email, for notarization |
| `APPLE_PASSWORD` | app-specific password for that Apple ID |
| `APPLE_TEAM_ID` | 10-character team ID |
| `WINDOWS_CERTIFICATE` | base64 of an Authenticode `.pfx` |
| `WINDOWS_CERTIFICATE_PASSWORD` | password of that `.pfx` |
| `TAURI_SIGNING_PRIVATE_KEY` | **required for in-app updates**: contents of `~/.tauri/deskbunny.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | contents of `~/.tauri/deskbunny.key.password` |

Without signing:

- **macOS**: Gatekeeper blocks the first launch. Users right-click the app and
  choose **Open**, then confirm (on macOS 15+, allow it in System Settings >
  Privacy & Security). Or run
  `xattr -dr com.apple.quarantine /Applications/DeskBunny.app`.
- **Windows**: SmartScreen shows "Windows protected your PC". Users click
  **More info** > **Run anyway**.

## Notes for users (put these in the release notes)

The app opens a setup guide on first launch that handles USB access on Linux
and the Uno R4 Minima driver on Windows. Link users to
[docs/SETUP.md](SETUP.md) for the same steps by hand, and keep that file in
sync with `app/src-tauri/src/setup.rs` when either changes.
