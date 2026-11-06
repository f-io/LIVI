# RGB / VGA displays

EDID profiles for RGB/VGA panels. Most of them run below HDMI's 25 MHz pixel clock minimum, which means they cannot be driven over HDMI as-is. The HDMI-PR setup script ([`scripts/install/pi/setup-hdmi-pr-display.sh`](../../scripts/install/pi/setup-hdmi-pr-display.sh)) drives them from a Raspberry Pi by working around that floor.

## How it works

It patches the vc4 KMS driver to apply HDMI pixel repetition to every mode under 25 MHz, 2x by default and 4x below 12.5 MHz. The repeated pixels raise the wire clock above the floor while userspace keeps the panel's native resolution. The script also forces the panel EDID so the Pi outputs exactly that timing.

## Naming

`<CAR>_<SYSTEM>_<PANEL>_<WIDTH>_<HEIGHT>.edid`

`<SYSTEM>` is the in-car system the display belongs to, as the manufacturer names it. Example: `VOLVO_RTI_LQ065T5GG63_400_234.edid` is Volvo's RTI system (Road and Traffic Information), a Sharp 400x234 panel.

## Use

The installer asks for this on a Raspberry Pi and offers the profiles below, so a new
profile only has to be dropped into this directory to show up there. Answering no skips
the whole step.

By hand, copy the profile to the Pi and run:

```bash
bash scripts/install/pi/setup-hdmi-pr-display.sh --edid assets/displays/VOLVO_RTI_LQ065T5GG63_400_234.edid
sudo reboot
```

## Profiles

| File | Display | Native timing | On the wire | Provided by |
|------|---------|---------------|-------------|-------------|
| `AUDI_RNS-E_PU_193_LT065AB3D600_800_470i_60Hz.edid` | Audi RNS-E PU 193 EU, Toshiba LT065AB3D600 | 800x470i 60Hz @ 33.44 MHz | 1x, 33.44 MHz | [@niz93](https://github.com/niz93) |
| `MERCEDES_COMAND2_LQ5AW136_320_230p_60Hz.edid` | Mercedes COMAND 2, Sharp LQ5AW136 | 320x230p 60Hz @ 6.30 MHz | 4x to 25.2 MHz | [@niz93](https://github.com/niz93) |
| `MERCEDES_COMAND2_LQ5AW136_640_460i_30Hz.edid` | Mercedes COMAND 2, Sharp LQ5AW136 | 640x460i 30Hz @ 12.71 MHz | 2x to 25.42 MHz | [@niz93](https://github.com/niz93) |
| `VOLVO_RTI_LQ065T5GG63_400_234.edid` | Volvo RTI, Sharp LQ065T5GG63 | 400x234 @ 8.14 MHz | 4x to 32.56 MHz | [@f-io](https://github.com/f-io) |


