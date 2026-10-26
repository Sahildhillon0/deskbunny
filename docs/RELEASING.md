# Releasing DeskBunny

Releases are built by `.github/workflows/release.yml`. Pushing a `vX.Y.Z` tag
builds the firmware once, builds installers on Linux, Windows and macOS, and
creates a **draft** GitHub release. Nothing is public until you publish it.

## Cut a release

1. Make sure `main` is green in CI. The release workflow does not run the test
   suites again; it only builds.
2. Bump the version to `X.Y.Z` in all of these, in one commit:
   - `app/src-tauri/tauri.conf.json`: `"version"`
   - `app/src-tauri/Cargo.toml`: `[package] version` (run `cargo check` so
     `Cargo.lock` follows)
   - `app/package.json`: `"version"` (run `npm install --package-lock-only` in
     `app/` so `package-lock.json` follows)
   - the firmware version define in `firmware/` (printed in the boot line
     `DESKBUNNY <fw_version> ...` and written to `manifest.json` as
     `fwVersion`). Bump it only when the firmware changed; the app version
     and the firmware version may differ.
3. Tag and push:

   ```sh
   git tag -a vX.Y.Z -m "DeskBunny vX.Y.Z"
   git push origin vX.Y.Z
   ```

   The `preflight` job fails if the tag does not match the versions in
   `tauri.conf.json` and `package.json`.
4. Wait for the `Release` workflow. It produces a draft release named
   `DeskBunny vX.Y.Z` with the installers from each OS, then attaches the
   firmware images and `manifest.json`.
5. Review the draft on GitHub: download one installer per OS, flash a board,
   write the release notes, then **Publish**.

To rebuild a draft for an existing tag (for example after adding a signing
secret), run the workflow by hand from the Actions tab
(`Release` > `Run workflow`) and enter the tag. Delete the old draft first, or
the new assets are added next to the old ones.

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

### Linux: serial port access

Flashing needs read/write access to `/dev/ttyUSB*` and `/dev/ttyACM*`. Add the
user to the serial group, then log out and back in:

```sh
# Debian, Ubuntu, Mint, Fedora
sudo usermod -aG dialout "$USER"
# Arch, Manjaro
sudo usermod -aG uucp "$USER"
```

The Uno R4 Minima bootloader is a USB DFU device (`2341:0369`), not a serial
port. Allow it with a udev rule:

```sh
echo 'SUBSYSTEM=="usb", ATTRS{idVendor}=="2341", ATTRS{idProduct}=="0369", MODE="0666"' |
  sudo tee /etc/udev/rules.d/60-deskbunny.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
```

### Windows: Uno R4 Minima (DFU)

The Uno R4 Minima is flashed over USB DFU, which needs the **WinUSB** driver
bound to the bootloader device (`2341:0369`). If the Arduino IDE with the
"Arduino UNO R4 Boards" core was ever installed, the driver is already there.
Otherwise: double-tap the board's reset button so it enters the bootloader,
open [Zadig](https://zadig.akeo.ie/), select the DFU device, choose **WinUSB**
and click **Install Driver**. The classic AVR boards (Uno, Nano, Mega,
Leonardo, Micro) and the Uno R4 WiFi use the standard serial drivers; CH340
clones may need the WCH CH340 driver.
