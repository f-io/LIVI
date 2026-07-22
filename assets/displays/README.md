# RGB / VGA displays below the HDMI clock floor

EDID profiles for VGA panels whose pixel clock is below HDMI's 25 MHz minimum, which means they cannot be driven over HDMI as-is. The HDMI-PR setup script ([`scripts/install/pi/setup-hdmi-pr-display.sh`](../../scripts/install/pi/setup-hdmi-pr-display.sh)) drives them from a Raspberry Pi by working around that floor.

## How it works

It patches the vc4 KMS driver to apply HDMI pixel repetition to every mode under 25 MHz, 2x by default and 4x below 12.5 MHz. The repeated pixels raise the wire clock above the floor while userspace keeps the panel's native resolution. The script also forces the panel EDID so the Pi outputs exactly that timing.

## Naming

`<CAR>_<SYSTEM>_<PANEL>_<WIDTH>_<HEIGHT>.edid`

`<SYSTEM>` is the in-car system the display belongs to, as the manufacturer names it. Example: `VOLVO_RTI_SHARP_400_234.edid` is Volvo's RTI system (Road and Traffic Information), a Sharp 400x234 panel.

## Use

The installer asks for this on a Raspberry Pi and offers the profiles below, so a new
profile only has to be dropped into this directory to show up there. Answering no skips
the whole step.

By hand, copy the profile to the Pi and run:

```bash
bash scripts/install/pi/setup-hdmi-pr-display.sh --edid assets/displays/VOLVO_RTI_SHARP_400_234.edid
sudo reboot
```

## Profiles

| File | Display | Native timing | On the wire |
|------|---------|---------------|-------------|
| `VOLVO_RTI_SHARP_400_234.edid` | Volvo RTI, Sharp 400x234 | 400x234 @ 8.14 MHz | 4x to 32.56 MHz |
