# Uno R4 WinUSB driver (Windows)

`renesas.inf` + `renesas.cat` are copied unmodified from
[arduino/ArduinoCore-renesas](https://github.com/arduino/ArduinoCore-renesas) `drivers/`
(MIT, see `LICENSE-ArduinoCore-renesas.txt`). The `.cat` is Arduino's signed
catalog, so the files must stay byte-identical.

They bind WinUSB to the Uno R4 Minima DFU bootloader (2341:0369) and the
sketch's DFU-runtime interface (2341:0069 MI_02). DeskBunny's setup guide
installs them with `pnputil /add-driver renesas.inf /install` (elevated).
Other boards need no driver on Windows 10/11.
