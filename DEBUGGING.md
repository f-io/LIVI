# Debugging LIVI

How to turn on diagnostics and where to read the logs when something misbehaves.

## Process layout

LIVI runs as several processes, which is why the flags below take effect in
different places. Every flag is an environment variable, and the child processes
inherit whatever you set before the LIVI command:

```
LIVI_GST_DEBUG=1 ./LIVI*.AppImage
```

- **core** is `livi-core`. It drives USB, sessions, the projection drivers and the
  settings, and starts the other processes.
- **UI** is the `LIVI` window (Slint). On Linux core starts it, on macOS it starts
  core. It talks to core over a unix socket.
- **livi-output** is a standalone native binary that runs the GStreamer video
  pipeline. Core spawns it and talks to it over a unix socket.
- **livi-compositor** is LIVI's own Wayland compositor on Linux. Core spawns it.
- **helper** is `livi-helperd`, a native binary core runs as root. It sets up
  Bluetooth, Wi-Fi and the access point for native Android Auto and CarPlay.

## Where the logs go

- **stdout / stderr**: everything is printed to the console, timestamped. When LIVI
  is started from the autostart entry, the output is redirected to `LIVI.log` next
  to the AppImage.
- **livi-output crash backtrace**: on a pipeline crash the child writes
  `livi-output-crash.log` next to the AppImage (or the working directory), and
  core prints it as well.

## Debug flags

### General

| Flag      | Effect                                                     |
| --------- | ---------------------------------------------------------- |
| `DEBUG=1` | General debug logging. Also enables the helper debug flag. |
| `TRACE=1` | Trace level logging.                                       |

### Video and GStreamer

| Flag                        | Effect                                                                                                                                                                                                                                                            |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `LIVI_GST_DEBUG=1`          | GStreamer debug logging with Pi-tuned categories (`v4l2codecs-decoder:6,v4l2codecs-h265dec:6,waylandsink:5,wl_dmabuf:6`). Any other value is used verbatim as the GStreamer debug string, for example `LIVI_GST_DEBUG="waylandsink:6"` or `LIVI_GST_DEBUG="*:5"`. |
| `LIVI_GST_SWDEC=1`          | Force software decoding (avdec_*), bypassing the hardware decoder. Use it to tell a broken hardware decoder apart from a broken stream.                                                                                                                           |
| `LIVI_GST_SINK=<element>`   | Override the video sink (default `waylandsink` on Linux).                                                                                                                                                                                                         |
| `LIVI_GST_PRELOAD=<lib.so>` | LD_PRELOAD a library into the livi-output child only.                                                                                                                                                                                                             |

### Compositor and Wayland

| Flag                   | Effect                                                                                  |
| ---------------------- | --------------------------------------------------------------------------------------- |
| `LIVI_WLR_DEBUG=1`     | Raise the compositor log level to debug.                                                |
| `LIVI_DEBUG_BG=1`      | Paint a magenta debug background in the compositor, to see the layout and video planes. |
| `LIVI_NO_COMPOSITOR=1` | Start without the nested compositor.                                                    |

### UI

| Flag                          | Effect                                                                                    |
| ----------------------------- | ----------------------------------------------------------------------------------------- |
| `LIVI_NO_UI=1`                | Run livi-core without starting the UI.                                                    |
| `LIVI_UI_CMD=<command>`       | Start this shell command as the UI instead.                                               |
| `LIVI_UI_SIZE=<w>x<h>`        | Open the UI window at this size, e.g. `320x200`.                                          |
| `LIVI_UI_PAGE=<page>`         | Open the UI on `projection`, `media`, `telemetry` or `settings`.                          |
| `LIVI_UI_SETTINGS=<path>`     | With the settings page, open this settings path, e.g. `general/connections`.              |
| `LIVI_UI_SNAPSHOT=<file.png>` | Save a picture of the UI window 3 s after start, `LIVI_UI_SNAPSHOT_MS` sets another wait. |
| `LIVI_UI_STATE=<file.json>`   | Show this core state without a core, e.g. `native/crates/livi-ui/fixtures/dab.json`.      |

### CarPlay, Bluetooth and Wi-Fi helper

| Flag              | Effect                                                          |
| ----------------- | --------------------------------------------------------------- |
| `LIVI_CP_DEBUG=1` | Verbose CarPlay and helper logging (also enabled by `DEBUG=1`). |

## Common workflows

**Video does not show up.** Look at the hardware path and its log first, then
isolate against software decode:

```
LIVI_GST_DEBUG=1 ./LIVI*.AppImage        # what the decoder and sink report
LIVI_GST_SWDEC=1 ./LIVI*.AppImage        # does software decode work at all
```

If software decode works and hardware does not, the hardware decoder or the GPU
behind it is the problem, not the stream.

**Touch or layout problems, or a black video plane.**

```
LIVI_WLR_DEBUG=1 ./LIVI*.AppImage        # compositor, seat and input events
LIVI_DEBUG_BG=1 ./LIVI*.AppImage         # make the plane layout visible
```

**CarPlay, Bluetooth or Wi-Fi bring-up.**

```
LIVI_CP_DEBUG=1 ./LIVI*.AppImage
```
