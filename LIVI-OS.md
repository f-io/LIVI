# LIVI OS

LIVI OS is a standalone operating system for embedded hardware, built on mainline Linux.

## Firmware

| Firmware    | SoC              | Wi-Fi module | Bluetooth             | Wi-Fi                       | PHY rate   | Display  | Kernel |
| ----------- | ---------------- | ------------ | --------------------- | --------------------------- | ---------- | -------- | ------ |
| `f133b_xfh` | Allwinner F133-B | AIC8800D40L  | Bluetrum AB5301B[^bt] | Wi-Fi 6, 1x1, 5 GHz, 40 MHz | 287 Mbit/s | 452×1280 | 7.3.6  |

[^bt]: The module on the unit's UART also carries the audio to the amplifier and the knob.

## Install

The provisioning tool for LIVI OS is not available yet.

## Web interface

<http://livi-os.local/> or <http://10.10.0.1/> on the unit's Wi-Fi.

## Updating

In the web interface under **Firmware**, **Check** looks for a newer version and **Update**
installs it. With **Nightly** on it checks the nightly builds instead of the latest release. Leave
the unit alone while it flashes. It reboots on its own when it is done.

The [firmware page](https://f-io.github.io/LIVI/firmware/) has every firmware file, so you can also

upload one by hand.

## Examples

### F133-XFH - RISCV, 64MB DDR2, 16MB SPI NOR

<p align="center">
  <img src="docs/media/livi-os/f133_xfh.png" alt="Cluster on LIVI OS" width="70%" />
</p>

## If something goes wrong

A unit whose system does not come up on first boot stays in a rescue system after the next restart.
It is reachable over USB at `telnet 10.10.10.1`, the logs are under `/tmp`. A unit that does not
start at all can still be recovered over the SoC's FEL mode, open an
[issue](https://github.com/f-io/LIVI/issues) for that.
