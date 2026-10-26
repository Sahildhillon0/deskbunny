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
        "wch_windows",
        "https://www.wch-ic.com/downloads/CH341SER_EXE.html",
    ),
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
    fn r4_driver_installed() -> Option<bool> {
        let out = Command::new("pnputil")
            .arg("/enum-drivers")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        Some(
            String::from_utf8_lossy(&out.stdout)
                .to_ascii_lowercase()
                .contains("renesas.inf"),
        )
    }

    pub fn checks(app: &AppHandle) -> Vec<SetupCheck> {
        let mut out = vec![check(
            "serial",
            "USB serial",
            "Windows 10 and 11 include the driver for Uno, Nano, Mega, Leonardo, Micro and Uno R4 WiFi boards. Nothing to install.",
            CheckState::Ok,
        )];
        let inf = driver_inf(app);
        match r4_driver_installed() {
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
        let mut ch340 = check(
            "ch340",
            "Nano clones (CH340)",
            "Windows Update usually installs the CH340 driver the first time a clone is plugged in. If no COM port appears for your Nano clone, install the driver from the chip maker.",
            CheckState::Info,
        );
        ch340.link = Some(SetupLink {
            id: "wch_windows".into(),
            label: "CH340 driver (WCH)".into(),
        });
        out.push(ch340);
        out
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
