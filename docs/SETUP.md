# Setting up DeskBunny

DeskBunny opens a setup guide the first time it runs. It checks what your
computer needs, fixes it with one click where it can, and waits for your board
to show up. You can open it again any time with the **?** button next to the
DeskBunny name, top left.

This page is the same guide in writing, for when you'd rather do it by hand.
It covers the software only. Wiring the OLED is in the firmware notes.

**What you need:** your DeskBunny (an Arduino with the 0.96" SSD1306 OLED wired
up) and a USB cable that carries data. Some phone cables only charge.

## 1. Install and open DeskBunny

Download the installer for your system from the latest release.

| System | File | First launch |
|---|---|---|
| Windows | `.msi` or `-setup.exe` | If SmartScreen says "Windows protected your PC", click **More info** > **Run anyway**. |
| macOS | `.dmg` | Drag DeskBunny to Applications. If macOS won't open it, right-click it and choose **Open**, or allow it in **System Settings > Privacy & Security**. |
| Linux | `.AppImage`, `.deb` or `.rpm` | AppImage: `chmod +x DeskBunny*.AppImage`, then run it. |

## 2. Prepare your computer

### Linux

Linux only lets root use USB serial ports and Arduino bootloaders by default.
In the guide, click **Allow USB access** and enter your password. DeskBunny
installs a small udev rule. You don't need to join a group or log out.

To do it by hand:

```sh
sudo tee /etc/udev/rules.d/70-deskbunny.rules >/dev/null <<'EOF'
# DeskBunny: USB access for Arduino boards
SUBSYSTEMS=="usb", ATTRS{idVendor}=="2341", TAG+="uaccess", ENV{ID_MM_DEVICE_IGNORE}="1"
SUBSYSTEM=="usb", ATTR{idVendor}=="2341", TAG+="uaccess", MODE="0660"
SUBSYSTEMS=="usb", ATTRS{idVendor}=="1a86", ATTRS{idProduct}=="7523", TAG+="uaccess", ENV{ID_MM_DEVICE_IGNORE}="1"
SUBSYSTEMS=="usb", ATTRS{idVendor}=="0403", ATTRS{idProduct}=="6001", TAG+="uaccess", ENV{ID_MM_DEVICE_IGNORE}="1"
EOF
sudo udevadm control --reload-rules && sudo udevadm trigger
```

Then unplug the board and plug it back in.

- `uaccess` gives the person logged in at the desktop access to the device.
- `ID_MM_DEVICE_IGNORE` stops ModemManager probing the port during an upload.
- The rule also covers the Uno R4 Minima's DFU bootloader. Joining
  `dialout` or `uucp` would not cover that, because the bootloader is not a
  serial port.

**Using a group instead?** Adding yourself to `dialout` (Debian, Ubuntu,
Fedora) or `uucp` (Arch) also works for serial boards. It only takes effect
once every program in your session restarts, and on some desktops logging out
is not enough, so reboot. It does not help the Uno R4 Minima.

**Nano clone disappears right after you plug it in (Ubuntu, Debian)?** The
`brltty` braille driver claims CH340 adapters. The guide warns you if it is
installed. If you don't use a braille display:

```sh
sudo apt remove brltty
```

### Windows

- Uno, Nano, Mega, Leonardo, Micro and Uno R4 WiFi work as they are on
  Windows 10 and 11.
- **Uno R4 Minima:** its bootloader needs Arduino's signed WinUSB driver,
  which Windows does not install by itself. In the guide, click **Install
  driver** and accept the permission prompt. DeskBunny ships the driver
  unmodified from
  [ArduinoCore-renesas](https://github.com/arduino/ArduinoCore-renesas/tree/main/drivers).

  To do it by hand, in an administrator terminal:

  ```bat
  pnputil /add-driver "C:\Program Files\DeskBunny\drivers\windows\renesas.inf" /install
  ```

  If you ever installed "Arduino UNO R4 Boards" in the Arduino IDE, you already
  have this driver.
- **Nano clones (CH340):** Windows Update usually installs the driver the first
  time you plug one in. Use driver **3.5.2019.1**, not WCH's current 3.8 or 3.9:
  those fail to open the port on many clone boards ("A device attached to the
  system is not functioning"). The setup guide warns when 3.8+ is installed.
  To downgrade:
  1. Download "wch.cn - Ports - 3.5.2019.1" from the
     [Microsoft Update Catalog](https://www.catalog.update.microsoft.com/Search.aspx?q=wch.cn%203.5.2019.1).
  2. `expand ch341ser.cab -F:* ch341ser`
  3. In an administrator terminal: `pnputil /enum-drivers` to find the
     `oemNN.inf` whose original name is `ch341ser.inf`, then
     `pnputil /delete-driver oemNN.inf /uninstall`.
  4. `pnputil /add-driver ch341ser\ch341ser.inf /install`, re-plug the board and
     refresh the port list (the COM number may change).

  Windows Update may install the newer driver again later.

### macOS

- Nothing to install for any supported board, including the Uno R4.
- **Nano clones (CH340):** built in since macOS 11 Big Sur. On older versions,
  install the [WCH driver](https://www.wch-ic.com/downloads/CH34XSER_MAC_ZIP.html).
- **Apple silicon laptops:** the first time you plug in a board, macOS asks
  "Allow accessory to connect?". Choose **Allow**, or the board never shows up.

## 3. Plug in your DeskBunny

Plug the board straight into the computer, not into a hub or a monitor's USB
port. The guide (and the port list at the bottom of the main window) picks it
up within a couple of seconds.

| Board | Detected as |
|---|---|
| Uno R3, Mega, Leonardo, Micro, Uno R4 Minima, Uno R4 WiFi | its name, automatically |
| Nano clone (CH340 / FTDI) | "Nano (new bootloader)". If flashing can't reach the bootloader, pick **Nano (old bootloader)** |
| Pro Mini (on a USB-serial adapter) | pick it by hand |

**Check what's on it** asks the board which face it's running. A board that
has never been flashed with DeskBunny just won't answer yet.

## 4. Flash a face

Pick a face, check the port and board at the bottom, and press **Flash to
DeskBunny**. The board keeps the face after you unplug it.

## When something goes wrong

| Message | What to do |
|---|---|
| "isn't allowed to use USB devices" | Linux: open the guide and click **Allow USB access**, then re-plug the board. |
| "No response from bootloader" | Pick the right board. For a Nano clone, try **Nano (old bootloader)**. Close the Arduino IDE Serial Monitor. |
| "did not enter its DFU bootloader" (Uno R4 Minima) | Double-press the reset button so the LED fades in and out, then flash again. |
| "needs Arduino's Uno R4 driver" (Windows) | Open the guide and click **Install driver**. |
| Board never appears | Try another cable (it must carry data) and another USB port. On macOS, allow the accessory. |
