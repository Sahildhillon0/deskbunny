# Releasing DeskBunny

Releases are published by `.github/workflows/release.yml` straight to the
repo's **Releases** page, where anyone can download the installers.

It runs on every push to `main`. It publishes a release only when the app
version has no release yet; otherwise it stops after a few seconds. To ship a
new version, bump it and push.

## Cut a release

1. Make sure `main` is green in CI. The release workflow only builds; it does
   not re-run the test suites.
2. Bump the version everywhere at once and push:

   ```sh
   scripts/bump-version.sh 0.2.0
   git commit -am "release: v0.2.0" && git push
   ```

   The script updates `tauri.conf.json`, `Cargo.toml`/`Cargo.lock`,
   `app/package.json`/`package-lock.json` and the firmware's `DB_FW_VERSION`.
   The `preflight` job refuses to release if they disagree.
3. The `Release` workflow then:
   1. creates a draft release `DeskBunny v0.2.0`;
   2. builds the firmware once and installers on Linux, Windows and macOS;
   3. attaches them plus the `.hex`/`.bin` images and `manifest.json`;
   4. publishes the release as **Latest**, which creates the `v0.2.0` tag.

   If any build fails, the draft stays hidden. Fix the problem and push again:
   the next run reuses the same draft.

Other ways to start it:

- Push a `vX.Y.Z` tag that matches the app version.
- Run it from the Actions tab (`Release` > `Run workflow`), optionally with a
  branch, tag or commit.

To rebuild an already published version (for example after adding a signing
secret), delete that release and its tag on GitHub, then re-run the workflow.

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
| `TAURI_SIGNING_PRIVATE_KEY` | updater key from `npx tauri signer generate` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | password of the updater key, if any |

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
