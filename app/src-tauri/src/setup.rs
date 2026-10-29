//! First-run setup guide: per-OS checks for what DeskBunny needs to reach a
//! board over USB, plus the one-click fixes the guide offers. Software only:
//! wiring the OLED is covered by the firmware docs.
//!
//! Fixes that need admin rights go through the OS's own prompt (pkexec on
//! Linux, UAC on Windows). Every fix also carries the equivalent manual
//! commands, shown in the guide when the one-click path is unavailable.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

/// Marker written when the guide is finished or skipped; bump the suffix to
/// show the guide again after a release that adds steps.
const DONE_MARKER: &str = "setup-complete-v1";

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum CheckState {
    /// Ready, nothing to do.
    Ok,
    /// Something to fix; `action` and/or `commands` say how.
    Action,
    /// Might cause trouble for some boards.
    Warn,
    /// Good to know, nothing to check (Windows/macOS only so far).
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    Info,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SetupAction {
    pub id: String,
    pub label: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SetupLink {
    pub id: String,
    pub label: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SetupCheck {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub state: CheckState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<SetupAction>,
    /// Manual fallback: shell commands the user can copy.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<SetupLink>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    /// "linux" | "windows" | "macos"
    pub os: &'static str,
    pub os_label: String,
    pub first_run: bool,
    pub checks: Vec<SetupCheck>,
}

fn check(id: &str, title: &str, detail: impl Into<String>, state: CheckState) -> SetupCheck {
    SetupCheck {
        id: id.into(),
        title: title.into(),
        detail: detail.into(),
        state,
        action: None,
        commands: Vec::new(),
        link: None,
    }
}

fn marker_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join(DONE_MARKER))
}

pub fn status(app: &AppHandle) -> SetupStatus {
    let first_run = marker_path(app).is_none_or(|p| !p.exists());
    let mut checks = platform::checks(app);
    checks.push(match crate::firmware::load(app) {
        Ok(fw) => check(
            "firmware",
            "Firmware",
            format!(
                "DeskBunny firmware {} is built in for {} boards.",
                fw.manifest.fw_version,
                fw.manifest.boards.len()
            ),
            CheckState::Ok,
        ),
        Err(e) => check("firmware", "Firmware", e, CheckState::Warn),
    });
    SetupStatus {
        os: platform::OS,
        os_label: platform::os_label(),
        first_run,
        checks,
    }
}

pub fn finish(app: &AppHandle) -> Result<(), String> {
    let path = marker_path(app).ok_or("No config directory for DeskBunny.")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Could not save setup state: {e}"))?;
    }
    std::fs::write(&path, b"done\n").map_err(|e| format!("Could not save setup state: {e}"))
}

pub fn run(app: &AppHandle, action: &str) -> Result<String, String> {
    platform::run(app, action)
}

/// Links the guide may open. The webview passes an id, never a URL.
const LINKS: &[(&str, &str)] = &[
    (
        "wch_macos",
        "https://www.wch-ic.com/downloads/CH34XSER_MAC_ZIP.html",
    ),
    (
        "arduino_r4_drivers",
        "https://github.com/arduino/ArduinoCore-renesas/tree/main/drivers",
    ),
];

pub fn open_link(app: &AppHandle, id: &str) -> Result<(), String> {
    let url = LINKS
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, u)| *u)
        .ok_or_else(|| format!("Unknown link '{id}'."))?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("Could not open the browser: {e}. The address is {url}"))
}

/// One installed `ch341ser.inf` (WCH CH340) driver package.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Debug, PartialEq)]
struct Ch340Package {
    /// The driver store name, `oemNN.inf`.
    published: String,
    version: String,
}

/// Every `ch341ser.inf` package in `pnputil /enum-drivers` output. Field labels
/// are localised, so each block is matched by the inf names and a four-part
/// version token, never by label.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn ch340_packages(enum_drivers: &str) -> Vec<Ch340Package> {
    let text = enum_drivers.replace("\r\n", "\n");
    text.split("\n\n")
        .filter(|block| block.to_ascii_lowercase().contains("ch341ser.inf"))
        .filter_map(|block| {
            let toks: Vec<&str> = block.split_whitespace().collect();
            let published = toks.iter().find(|t| {
                let t = t.to_ascii_lowercase();
                t.starts_with("oem")
                    && t.ends_with(".inf")
                    && t[3..t.len() - 4].parse::<u32>().is_ok()
            })?;
            let version = toks.iter().find(|t| {
                let parts: Vec<&str> = t.split('.').collect();
                parts.len() == 4 && parts.iter().all(|p| p.parse::<u32>().is_ok())
            })?;
            Some(Ch340Package {
                published: published.to_string(),
                version: version.to_string(),
            })
        })
        .collect()
}

/// PowerShell run elevated by "Install CH340 driver": stage the good driver,
/// delete every other ch341ser package (so Windows can't pick it again), then
/// bind the good one to plugged-in boards. Exit 10/11/12 = which step failed.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn ch340_install_script(inf: &str, remove: &[String]) -> String {
    let inf = inf.replace('\'', "''");
    let mut s = String::from("$ok = @(0, 259, 3010)\r\n");
    s += &format!(
        "pnputil /add-driver '{inf}'\r\nif ($ok -notcontains $LASTEXITCODE) {{ exit 10 }}\r\n"
    );
    for oem in remove {
        s += &format!(
            "pnputil /delete-driver '{}' /uninstall /force\r\nif ($ok -notcontains $LASTEXITCODE) {{ exit 11 }}\r\n",
            oem.replace('\'', "''")
        );
    }
    s += &format!("pnputil /add-driver '{inf}' /install\r\n$c = $LASTEXITCODE\r\nif ($ok -notcontains $c) {{ exit 12 }}\r\nif ($c -eq 3010) {{ exit 3010 }}\r\nexit 0\r\n");
    s
}

/// WCH's 3.8 (2023) and later drivers fail to open many clone CH340 chips.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn ch340_driver_broken(version: &str) -> bool {
    let mut it = version.split('.').map(|n| n.parse::<u32>().unwrap_or(0));
    let (major, minor) = (it.next().unwrap_or(0), it.next().unwrap_or(0));
    (major, minor) >= (3, 8)
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::io::Write;
    use std::path::Path;
    use std::process::{Command, Stdio};

    pub const OS: &str = "linux";

    pub const RULE_PATH: &str = "/etc/udev/rules.d/70-deskbunny.rules";

    /// `uaccess` gives the logged-in desktop user access (no group, no relogin);
    /// `ID_MM_DEVICE_IGNORE` keeps ModemManager from probing the port mid-upload.
    pub const RULE: &str = r#"# DeskBunny: USB access for Arduino boards (installed by DeskBunny's setup guide)
# Arduino boards, including bootloaders (Uno R4 Minima DFU 2341:0369)
SUBSYSTEMS=="usb", ATTRS{idVendor}=="2341", TAG+="uaccess", ENV{ID_MM_DEVICE_IGNORE}="1"
SUBSYSTEM=="usb", ATTR{idVendor}=="2341", TAG+="uaccess", MODE="0660"
# Common Nano clones: CH340 and FTDI
SUBSYSTEMS=="usb", ATTRS{idVendor}=="1a86", ATTRS{idProduct}=="7523", TAG+="uaccess", ENV{ID_MM_DEVICE_IGNORE}="1"
SUBSYSTEMS=="usb", ATTRS{idVendor}=="0403", ATTRS{idProduct}=="6001", TAG+="uaccess", ENV{ID_MM_DEVICE_IGNORE}="1"
"#;

    const RULE_DIRS: &[&str] = &[
        "/etc/udev/rules.d",
        "/usr/lib/udev/rules.d",
        "/lib/udev/rules.d",
        "/run/udev/rules.d",
    ];

    pub fn os_label() -> String {
        std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|s| pretty_name(&s))
            .unwrap_or_else(|| "Linux".into())
    }

    pub(super) fn pretty_name(os_release: &str) -> Option<String> {
        os_release
            .lines()
            .find_map(|l| l.strip_prefix("PRETTY_NAME="))
            .map(|v| v.trim_matches('"').to_string())
    }

    /// True if a rules file grants user access to Arduino (2341) devices: ours,
    /// Arduino IDE's 60-arduino-renesas.rules, or a distro/user equivalent.
    pub(super) fn rules_grant_arduino(text: &str) -> bool {
        text.lines()
            .map(str::trim)
            .filter(|l| !l.starts_with('#'))
            .any(|l| {
                l.contains("2341")
                    && (l.contains("uaccess") || l.contains("0666") || l.contains("plugdev"))
            })
    }

    fn usb_access_installed() -> bool {
        RULE_DIRS.iter().any(|dir| {
            std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "rules"))
                .any(|e| std::fs::read_to_string(e.path()).is_ok_and(|t| rules_grant_arduino(&t)))
        })
    }

    /// brltty (Ubuntu/Debian default) claims CH340 adapters as braille displays.
    fn brltty_claims_ch340() -> bool {
        RULE_DIRS.iter().any(|dir| {
            std::fs::read_to_string(Path::new(dir).join("85-brltty.rules"))
                .is_ok_and(|t| t.contains("1a86") && t.contains("7523"))
        })
    }

    pub fn manual_commands() -> Vec<String> {
        vec![
            format!("sudo tee {RULE_PATH} >/dev/null <<'EOF'\n{}EOF", RULE),
            "sudo udevadm control --reload-rules && sudo udevadm trigger".into(),
            "# then unplug the board and plug it back in".into(),
        ]
    }

    pub fn checks(_app: &AppHandle) -> Vec<SetupCheck> {
        let mut out = Vec::new();
        if usb_access_installed() {
            out.push(check(
                "usb_access",
                "USB access",
                "A udev rule lets you reach Arduino boards without root. Re-plug the board once if it was connected while the rule was added.",
                CheckState::Ok,
            ));
        } else {
            let mut c = check(
                "usb_access",
                "USB access",
                "Linux only lets root talk to USB serial ports and bootloaders by default. DeskBunny installs a small udev rule so your user can, with no group changes and no logging out. You will be asked for your password.",
                CheckState::Action,
            );
            c.action = Some(SetupAction {
                id: "install_udev".into(),
                label: "Allow USB access".into(),
            });
            c.commands = manual_commands();
            out.push(c);
        }
        if brltty_claims_ch340() {
            let mut c = check(
                "brltty",
                "Braille driver (brltty)",
                "brltty is installed and grabs CH340-based Nano clones as soon as they are plugged in, so the port disappears. If you don't use a braille display, remove it.",
                CheckState::Warn,
            );
            c.commands = vec![
                "sudo apt remove brltty".into(),
                "# or keep it and stop it claiming USB serial adapters:".into(),
                "sudo systemctl mask brltty-udev.service && sudo systemctl stop brltty-udev.service"
                    .into(),
            ];
            out.push(c);
        }
        out
    }

    pub fn run(_app: &AppHandle, action: &str) -> Result<String, String> {
        match action {
            "install_udev" => install_udev(),
            _ => Err(format!("Unknown setup action '{action}'.")),
        }
    }

    fn install_udev() -> Result<String, String> {
        let script = format!(
            "install -m 0644 /dev/stdin {RULE_PATH} && udevadm control --reload-rules && udevadm trigger --subsystem-match=usb --subsystem-match=tty"
        );
        let mut child = Command::new("pkexec")
            .args(["sh", "-c", &script])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                format!("Could not ask for administrator rights ({e}). Run the commands below in a terminal instead.")
            })?;
        child
            .stdin
            .take()
            .ok_or("Could not talk to pkexec.")?
            .write_all(RULE.as_bytes())
            .map_err(|e| format!("Could not pass the rule to pkexec: {e}"))?;
        let out = child
            .wait_with_output()
            .map_err(|e| format!("pkexec failed: {e}"))?;
        match out.status.code() {
            Some(0) => Ok("USB access allowed. If your board is plugged in, unplug it and plug it back in.".into()),
            // pkexec: 126 = dialog dismissed / not authorized, 127 = no polkit agent.
            Some(126) => Err("The password prompt was cancelled. Try again, or run the commands below in a terminal.".into()),
            Some(127) => Err("No password prompt is available (no polkit agent running). Run the commands below in a terminal.".into()),
            _ => Err(format!(
                "Installing the rule failed: {}. Run the commands below in a terminal.",
                String::from_utf8_lossy(&out.stderr).trim()
            )),
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use tauri::path::BaseDirectory;

    pub const OS: &str = "windows";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn os_label() -> String {
        "Windows".into()
    }

    fn driver_inf(app: &AppHandle) -> Option<PathBuf> {
        app.path()
            .resolve("drivers/windows/renesas.inf", BaseDirectory::Resource)
            .ok()
            .filter(|p| p.is_file())
            .or_else(|| {
                cfg!(debug_assertions).then(|| {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("drivers/windows/renesas.inf")
                })
            })
    }

    /// `pnputil /enum-drivers` lists installed third-party drivers by their
    /// original .inf name; it works without elevation.
    fn enum_drivers() -> Option<String> {
        let out = Command::new("pnputil")
            .arg("/enum-drivers")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn r4_driver_installed(drivers: Option<&str>) -> Option<bool> {
        Some(drivers?.to_ascii_lowercase().contains("renesas.inf"))
    }

    /// Microsoft Update Catalog copy of WCH's CH340 driver 3.5.2019.1 ("wch.cn -
    /// Ports - 3.5.2019.1", Windows - Client 21H2 and later), signed by Microsoft
    /// Windows Hardware Compatibility Publisher. The file name carries its SHA-1.
    const CH340_CAB_URL: &str = "https://catalog.s.download.windowsupdate.com/c/msdownload/update/driver/drvs/2021/12/ch341ser_10_9b63440a38b2e0761916bf7fc0d8a2af2edb3a6c.cab";
    const CH340_CAB_SHA256: &str =
        "a58c7688d5b313a19b029c8fbf61341d238129de231bb57f245f4ac4c5c92d16";
    const CH340_GOOD: &str = "3.5.2019.1";

    pub fn checks(app: &AppHandle) -> Vec<SetupCheck> {
        let mut out = vec![check(
            "serial",
            "USB serial",
            "Windows 10 and 11 include the driver for Uno, Nano, Mega, Leonardo, Micro and Uno R4 WiFi boards. Nothing to install.",
            CheckState::Ok,
        )];
        let inf = driver_inf(app);
        let drivers = enum_drivers();
        match r4_driver_installed(drivers.as_deref()) {
            Some(true) => out.push(check(
                "r4_driver",
                "Uno R4 Minima driver",
                "Installed. DeskBunny can flash the Uno R4 Minima.",
                CheckState::Ok,
            )),
            state => {
                let mut c = check(
                    "r4_driver",
                    "Uno R4 Minima driver",
                    if state.is_none() {
                        "Only needed for the Uno R4 Minima: its bootloader needs Arduino's signed WinUSB driver. DeskBunny could not tell whether it is installed; installing again is harmless. Windows will ask for permission."
                    } else {
                        "Only needed for the Uno R4 Minima: its bootloader needs Arduino's signed WinUSB driver, which Windows doesn't install by itself. Windows will ask for permission."
                    },
                    CheckState::Action,
                );
                if let Some(p) = &inf {
                    c.action = Some(SetupAction {
                        id: "install_r4_driver".into(),
                        label: "Install driver".into(),
                    });
                    c.commands = vec![format!(
                        "pnputil /add-driver \"{}\" /install   # in an administrator terminal",
                        p.display()
                    )];
                }
                c.link = Some(SetupLink {
                    id: "arduino_r4_drivers".into(),
                    label: "Arduino's driver files".into(),
                });
                out.push(c);
            }
        }
        out.push(ch340_check(drivers.as_deref()));
        out
    }

    fn ch340_check(drivers: Option<&str>) -> SetupCheck {
        let pkgs = drivers.map(ch340_packages).unwrap_or_default();
        let broken: Vec<&str> = pkgs
            .iter()
            .filter(|p| ch340_driver_broken(&p.version))
            .map(|p| p.version.as_str())
            .collect();
        let mut c = if let Some(v) = broken.first() {
            check(
                "ch340",
                "Nano clones (CH340)",
                format!("CH340 driver {v} is installed. WCH's drivers 3.8 and newer can't open the port on many clone boards (\"A device attached to the system is not functioning\"). Replace it with 3.5.2019.1: DeskBunny downloads Microsoft's signed copy, removes the other CH340 drivers and installs it. Windows will ask for permission. Windows Update may bring the newer driver back later; if so, press the button again."),
                CheckState::Warn,
            )
        } else if let Some(p) = pkgs
            .iter()
            .find(|p| p.version == CH340_GOOD)
            .or(pkgs.first())
        {
            check(
                "ch340",
                "Nano clones (CH340)",
                format!(
                    "CH340 driver {} is installed. DeskBunny can flash CH340 clone boards.",
                    p.version
                ),
                CheckState::Ok,
            )
        } else {
            check(
                "ch340",
                "Nano clones (CH340)",
                "Only needed for clone boards with a CH340 USB chip. Installs CH340 driver 3.5.2019.1 (Microsoft's signed copy); WCH's newer 3.8 and 3.9 drivers fail on many clone boards. Windows will ask for permission.",
                CheckState::Info,
            )
        };
        c.action = Some(SetupAction {
            id: "install_ch340_driver".into(),
            label: if broken.is_empty() {
                "Install CH340 driver".into()
            } else {
                format!("Replace with driver {CH340_GOOD}")
            },
        });
        c.commands = vec![
            format!("curl.exe -L -o ch341ser.cab {CH340_CAB_URL}"),
            "expand ch341ser.cab -F:* ch341ser".into(),
            "pnputil /enum-drivers   # note the oemNN.inf of every other ch341ser.inf".into(),
            "pnputil /delete-driver oemNN.inf /uninstall /force   # in an administrator terminal, for each".into(),
            "pnputil /add-driver ch341ser\\ch341ser.inf /install".into(),
        ];
        c
    }

    /// Downloads the Microsoft-signed CH340 driver 3.5.2019.1, checks its hash, and
    /// in one elevated step removes every other ch341ser package and installs it.
    fn install_ch340_driver() -> Result<String, String> {
        let dir = std::env::temp_dir().join("deskbunny-ch340");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
        let cab = dir.join("ch341ser.cab");
        let ps_quote = |p: &std::path::Path| p.display().to_string().replace('\'', "''");

        log::info!("CH340: downloading {CH340_CAB_URL}");
        let script = format!(
            "$ProgressPreference = 'SilentlyContinue'; Invoke-WebRequest -UseBasicParsing -Uri '{CH340_CAB_URL}' -OutFile '{}'",
            ps_quote(&cab)
        );
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("Could not start PowerShell: {e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            log::warn!("CH340: download failed: {err}");
            return Err("Could not download the CH340 driver from Microsoft. Are you online? You can also use the commands below.".into());
        }
        let bytes = std::fs::read(&cab)
            .map_err(|e| format!("Could not read the downloaded driver: {e}"))?;
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if hash != CH340_CAB_SHA256 {
            log::warn!("CH340: SHA-256 {hash} does not match {CH340_CAB_SHA256}");
            return Err("The downloaded CH340 driver is not the expected file, so DeskBunny did not install it. Try again later.".into());
        }

        let status = Command::new("expand")
            .arg(&cab)
            .arg("-F:*")
            .arg(&dir)
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| format!("Could not start expand: {e}"))?;
        let inf = dir.join("ch341ser.inf");
        if !status.success() || !inf.is_file() {
            return Err("Could not unpack the CH340 driver.".into());
        }

        let remove: Vec<String> = enum_drivers()
            .map(|d| ch340_packages(&d))
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.version != CH340_GOOD)
            .map(|p| p.published)
            .collect();
        log::info!("CH340: installing {CH340_GOOD}, removing {remove:?}");
        let ps1 = dir.join("install.ps1");
        std::fs::write(
            &ps1,
            ch340_install_script(&inf.display().to_string(), &remove),
        )
        .map_err(|e| format!("Could not write the install script: {e}"))?;

        // One UAC prompt for every pnputil step; declining throws (exit 1).
        let elevate = format!(
            "$p = Start-Process -FilePath powershell.exe -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','\"{}\"' -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode",
            ps_quote(&ps1)
        );
        let status = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &elevate])
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| format!("Could not start PowerShell: {e}"))?;
        log::info!("CH340: install script exited with {:?}", status.code());
        let _ = std::fs::remove_dir_all(&dir);
        match status.code() {
            Some(0) => Ok(format!("CH340 driver {CH340_GOOD} installed. Unplug the board, plug it back in, and pick its port again (the COM number may change).")),
            Some(3010) => Ok(format!("CH340 driver {CH340_GOOD} installed. Restart Windows to finish.")),
            Some(10) => Err("Windows refused to add the CH340 driver. Try the commands below in an administrator terminal.".into()),
            Some(11) => Err("DeskBunny added driver 3.5.2019.1 but could not remove the newer CH340 driver. Unplug every CH340 board, then try again.".into()),
            Some(12) => Err("The driver was added, but Windows could not attach it to the board. Unplug the board, plug it back in, and try again.".into()),
            _ => Err("The driver was not installed (the permission prompt was declined or failed). Try again, or run the commands below in an administrator terminal.".into()),
        }
    }

    pub fn run(app: &AppHandle, action: &str) -> Result<String, String> {
        match action {
            "install_r4_driver" => {
                let inf = driver_inf(app).ok_or("The driver files are missing from this build.")?;
                let inf = inf.display().to_string().replace('\'', "''");
                // Start-Process -Verb RunAs shows the UAC prompt; declining throws (exit 1).
                let script = format!(
                    "$p = Start-Process -FilePath pnputil.exe -ArgumentList '/add-driver','\"{inf}\"','/install' -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode"
                );
                let status = Command::new("powershell")
                    .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                    .creation_flags(CREATE_NO_WINDOW)
                    .status()
                    .map_err(|e| format!("Could not start PowerShell: {e}"))?;
                // pnputil: 0 ok, 259 = added but no matching device plugged in, 3010 = reboot pending.
                match status.code() {
                    Some(0) | Some(259) => Ok("Driver installed. Unplug the Uno R4 Minima and plug it back in.".into()),
                    Some(3010) => Ok("Driver installed. Restart Windows to finish.".into()),
                    _ => Err("The driver was not installed (the permission prompt was declined or failed). Try again, or run the command below in an administrator terminal.".into()),
                }
            }
            "install_ch340_driver" => install_ch340_driver(),
            _ => Err(format!("Unknown setup action '{action}'.")),
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::process::Command;

    pub const OS: &str = "macos";

    fn product_version() -> Option<String> {
        let out = Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    pub fn os_label() -> String {
        product_version()
            .map(|v| format!("macOS {v}"))
            .unwrap_or_else(|| "macOS".into())
    }

    pub fn checks(_app: &AppHandle) -> Vec<SetupCheck> {
        let mut out = vec![check(
            "serial",
            "USB serial",
            "macOS includes everything needed for Arduino boards, including the Uno R4. Nothing to install.",
            CheckState::Ok,
        )];
        let major =
            product_version().and_then(|v| v.split('.').next().and_then(|m| m.parse::<u32>().ok()));
        if major.is_some_and(|m| m < 11) {
            let mut c = check(
                "ch340",
                "Nano clones (CH340)",
                "macOS before 11 (Big Sur) has no CH340 driver. Install it from the chip maker if you use a Nano clone.",
                CheckState::Action,
            );
            c.link = Some(SetupLink {
                id: "wch_macos".into(),
                label: "CH340 driver (WCH)".into(),
            });
            out.push(c);
        } else {
            out.push(check(
                "ch340",
                "Nano clones (CH340)",
                "Built into macOS 11 and later.",
                CheckState::Ok,
            ));
        }
        if cfg!(target_arch = "aarch64") {
            out.push(check(
                "accessory",
                "Allow the accessory",
                "On Apple silicon laptops macOS asks \"Allow accessory to connect?\" the first time a board is plugged in. Choose Allow, or the board never shows up.",
                CheckState::Info,
            ));
        }
        out
    }

    pub fn run(_app: &AppHandle, action: &str) -> Result<String, String> {
        Err(format!("Unknown setup action '{action}'."))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_ch340_packages() {
        let out = "Microsoft PnP Utility\r\n\r\nPublished Name:     oem7.inf\r\nOriginal Name:      renesas.inf\r\nDriver Version:     03/03/2020 10.48.0.0\r\n\r\nPublished Name:     oem25.inf\r\nOriginal Name:      ch341ser.inf\r\nProvider Name:      wch.cn\r\nClass GUID:         {4d36e978-e325-11ce-bfc1-08002be10318}\r\nDriver Version:     09/16/2024 3.9.2024.9\r\n\r\nVeröffentlichter Name: oem3.inf\r\nOriginalname:      ch341ser.inf\r\nTreiberversion:    01/30/2019 3.5.2019.1\r\n";
        let pkgs = super::ch340_packages(out);
        assert_eq!(
            pkgs,
            vec![
                super::Ch340Package {
                    published: "oem25.inf".into(),
                    version: "3.9.2024.9".into()
                },
                super::Ch340Package {
                    published: "oem3.inf".into(),
                    version: "3.5.2019.1".into()
                },
            ]
        );
        assert!(super::ch340_packages("Published Name: oem7.inf\nOriginal Name: renesas.inf\nDriver Version: 1/1/2020 10.48.0.0").is_empty());
        assert!(super::ch340_driver_broken("3.9.2024.9"));
        assert!(super::ch340_driver_broken("3.8.2023.2"));
        assert!(!super::ch340_driver_broken("3.5.2019.1"));
        assert!(!super::ch340_driver_broken("3.7.2022.1"));
    }

    #[test]
    fn ch340_script_stages_removes_then_installs() {
        let s = super::ch340_install_script(
            r"C:\Users\O'Neil\AppData\Local\Temp\deskbunny-ch340\ch341ser.inf",
            &["oem25.inf".into()],
        );
        let add = s.find("pnputil /add-driver 'C:\\Users\\O''Neil").unwrap();
        let del = s
            .find("pnputil /delete-driver 'oem25.inf' /uninstall /force")
            .unwrap();
        let install = s.find("ch341ser.inf' /install").unwrap();
        assert!(add < del && del < install, "{s}");
        assert!(s.contains("exit 10") && s.contains("exit 11") && s.contains("exit 12"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn detects_arduino_rules() {
        use super::platform::*;
        assert!(rules_grant_arduino(RULE));
        assert!(rules_grant_arduino(
            r#"SUBSYSTEMS=="usb", ATTRS{idVendor}=="2341", MODE:="0666""#
        ));
        assert!(!rules_grant_arduino(
            r#"# SUBSYSTEMS=="usb", ATTRS{idVendor}=="2341", TAG+="uaccess""#
        ));
        assert!(!rules_grant_arduino(
            r#"SUBSYSTEMS=="usb", ATTRS{idVendor}=="1a86", TAG+="uaccess""#
        ));
        assert_eq!(
            pretty_name("NAME=\"Arch\"\nPRETTY_NAME=\"Arch Linux\"\n").as_deref(),
            Some("Arch Linux")
        );
    }

    #[test]
    fn links_are_https() {
        assert!(super::LINKS.iter().all(|(_, u)| u.starts_with("https://")));
    }
}
