# Credits

As always, this is not the effort of a single person. Many thanks to everyone who
has contributed, tested, reported issues, and provided feedback,
especially to those who initially explored and experimented with these concepts.

This project was inspired by and informed through various open-source projects,
experiments, and community efforts in the CarPlay / Android Auto ecosystem.

The following projects were valuable references, inspiration, or learning resources
during development.

### Inspiration & Prior Art

- **BertoldVdb/WACResearch** - Bertold Van den Bergh
- **f1xpl/aasdk** - Michal Szwaj

### Legacy / dongle related

- **rhysmorgan134/node-carplay** by Rhys Morgan
- Various reverse-engineering projects, websites, and community write-ups

This list is likely incomplete, many thanks to everyone whose work contributed,
directly or indirectly.

## Third-Party Components

This application bundles the following third-party components.

### Electron

This application is built on **Electron**, which bundles Chromium, Node.js, V8 and
ffmpeg.

Electron is licensed under the **MIT License**. Chromium and its components are
licensed under the **BSD-3-Clause** and other compatible licenses.

https://www.electronjs.org/

### wlroots

This application bundles a **modified** build of **wlroots** (0.20). LIVI carries
local patches for host-output control and multi-output touch in the Wayland backend.

wlroots is licensed under the **MIT License**.

https://gitlab.freedesktop.org/wlroots/wlroots

### GStreamer

This application bundles parts of the **GStreamer multimedia framework**.

GStreamer is licensed under the **GNU Lesser General Public License (LGPL), version 2.1 or later**.
The license text is included in the `assets/gstreamer/LICENSES` directory.

https://gstreamer.freedesktop.org/

### node-usb-rs

This application bundles **node-usb-rs**.

node-usb-rs is licensed under the **MIT License**.

https://github.com/node-usb/node-usb-rs

### nusb

This application bundles **nusb** (compiled into the node-usb-rs prebuilt binaries).

nusb is dual-licensed under the **MIT OR Apache-2.0** license.

https://github.com/kevinmehall/nusb

### Roboto

This application bundles the **Roboto** font.

Roboto is licensed under the **SIL Open Font License 1.1 (OFL-1.1)**.

https://github.com/googlefonts/roboto
