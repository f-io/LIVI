# Debugging LIVI

How to turn on diagnostics and where to read the logs when something misbehaves.

## Process layout

LIVI runs as several processes, which is why the flags below take effect in
different places. Every flag is an environment variable, and the child processes
inherit whatever you set before the LIVI command:

```
LIVI_GST_DEBUG=1 ./LIVI*.AppImage
```

- **main** is the Electron main process. It drives the app, USB, sessions and the
  projection drivers.
- **gst-host** is a standalone native binary that runs the GStreamer video
  pipeline on Linux. The main process spawns it and talks to it over a unix socket.
- **livi-compositor** is the nested wlroots compositor on Linux. The main process
  spawns it.
- **helper** is a Python process that sets up Bluetooth and Wi-Fi for native
  Android Auto and CarPlay.

## Where the logs go

- **stdout / stderr**: everything is printed to the console, timestamped. When LIVI
  is started from the autostart entry, the output is redirected to `LIVI.log` next
  to the AppImage.
- **gst-host crash backtrace**: on a pipeline crash the child writes
  `livi-gst-host-crash.log` next to the AppImage (or the working directory), and
  the main process prints it as well.

## Debug flags

### General

| Flag | Effect |
| --- | --- |
| `DEBUG=1` | General debug logging. Also enables the helper debug flag. |
| `TRACE=1` | Trace level logging. |

### Video and GStreamer

| Flag | Effect |
| --- | --- |
| `LIVI_GST_DEBUG=1` | GStreamer debug logging with Pi-tuned categories (`v4l2codecs-decoder:6,v4l2codecs-h265dec:6,waylandsink:5,wl_dmabuf:6`). Any other value is used verbatim as the GStreamer debug string, for example `LIVI_GST_DEBUG="waylandsink:6"` or `LIVI_GST_DEBUG="*:5"`. |
| `LIVI_GST_SWDEC=1` | Force software decoding (avdec_*), bypassing the hardware decoder. Use it to tell a broken hardware decoder apart from a broken stream. |
| `LIVI_GST_SINK=<element>` | Override the video sink (default `waylandsink` on Linux). |
| `LIVI_GST_PRELOAD=<lib.so>` | LD_PRELOAD a library into the gst-host child only. |

### Compositor and Wayland

| Flag | Effect |
| --- | --- |
| `LIVI_WLR_DEBUG=1` | Raise the nested wlroots compositor log level to `WLR_DEBUG`. |
| `LIVI_DEBUG_BG=1` | Paint a magenta debug background in the compositor, to see the layout and video planes. |
| `LIVI_NO_COMPOSITOR=1` | Start without the nested compositor. |

### CarPlay, Bluetooth and Wi-Fi helper

| Flag | Effect |
| --- | --- |
| `LIVI_CP_DEBUG=1` | Verbose CarPlay and helper logging (also enabled by `DEBUG=1`). |
| `LIVI_CP_SYSLOG=1` | Tap the CarPlay helper output into syslog. |

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
