"""Shared BlueZ device management + reconnect worker for the LIVI wireless helpers.

Protocol-agnostic (no AA/CP specifics): enumerate paired devices, check connection,
Device1.Connect (wakes the phone and re-establishes its auto-connect profiles),
disconnect, and a periodic reconnect worker that keeps a bonded phone connected so it
auto-reconnects after an app restart / reboot instead of waiting for the phone.

Written to become the shared module of the unified BT/WiFi helper; it lives under the
CP tree for now only so it stages/packages without new plumbing.
"""

import subprocess
import threading
import time


def _busctl(*args, timeout=10):
    try:
        r = subprocess.run(
            ["busctl", "--system", *args],
            capture_output=True, text=True, timeout=timeout,
        )
        return r.returncode, r.stdout, r.stderr
    except (FileNotFoundError, subprocess.TimeoutExpired) as e:
        return 1, "", repr(e)


def _dev_path(adapter, mac):
    return f"/org/bluez/{adapter}/dev_" + mac.upper().replace(":", "_")


def _get_property(path, iface, prop, timeout=5):
    rc, out, _ = _busctl("get-property", "org.bluez", path, iface, prop, timeout=timeout)
    return out.strip() if rc == 0 else ""


def _quoted(s):
    # busctl prints strings as:  s "value"
    return s.split('"')[1] if '"' in s else ""


def _is_device_path(path):
    return "/dev_" in path and path.count("/") >= 4


def is_connected(adapter, mac):
    return _get_property(_dev_path(adapter, mac), "org.bluez.Device1", "Connected").lower().endswith("true")


def list_paired(adapter, timeout=5):
    """Return [{mac, name, connected}] for every paired BlueZ device."""
    rc, out, _ = _busctl("tree", "--list", "org.bluez", timeout=timeout)
    if rc != 0:
        return []
    devices = []
    for line in out.splitlines():
        path = line.strip()
        if not _is_device_path(path):
            continue
        if not _get_property(path, "org.bluez.Device1", "Paired").lower().endswith("true"):
            continue
        mac = _quoted(_get_property(path, "org.bluez.Device1", "Address")).upper()
        if not mac:
            continue
        devices.append({
            "mac": mac,
            "name": _quoted(_get_property(path, "org.bluez.Device1", "Alias")),
            "connected": _get_property(path, "org.bluez.Device1", "Connected").lower().endswith("true"),
        })
    return devices


def connect(adapter, mac, timeout=15):
    """Device1.Connect: wake the phone and (re)establish its auto-connect profiles."""
    rc, _, err = _busctl("call", "org.bluez", _dev_path(adapter, mac),
                         "org.bluez.Device1", "Connect", timeout=timeout)
    return rc == 0, err.strip()


def disconnect(adapter, mac, timeout=10):
    rc, _, err = _busctl("call", "org.bluez", _dev_path(adapter, mac),
                         "org.bluez.Device1", "Disconnect", timeout=timeout)
    return rc == 0, err.strip()


def wifi_has_station(iface, ctrl="/var/run/hostapd", timeout=3):
    """True if any client is associated with our hostapd AP. Once the phone joins Wi-Fi
    the projection session lives there (the BT/iAP2 link drops), so this is the real
    'session up' signal for the reconnect worker — leave BT alone while it is True.
    hostapd_cli list_sta prints one station MAC per line, nothing when none/AP down."""
    for exe in ("/usr/sbin/hostapd_cli", "/sbin/hostapd_cli", "hostapd_cli"):
        try:
            r = subprocess.run([exe, "-p", ctrl, "-i", iface, "list_sta"],
                               capture_output=True, text=True, timeout=timeout)
        except FileNotFoundError:
            continue
        except subprocess.TimeoutExpired:
            return False
        return any(":" in ln for ln in r.stdout.splitlines())
    return False


def start_reconnect_worker(adapter, is_active, log, interval=5.0, stale_ticks=2):
    """Daemon thread that gets a bonded phone back onto a fresh session after an app
    restart / reboot, so it doesn't hang until the user manually reconnects. Two cases,
    both skipped while `is_active()` reports a session is up:

    - paired + DISCONNECTED -> Device1.Connect (wake it).
    - paired + CONNECTED but no session (a stale link from a previous app instance):
      after `stale_ticks` consecutive ticks with no session (so a genuinely fresh
      connect, which sets the session flag within a tick, is never dropped) ->
      Device1.Disconnect. The bonded+trusted phone then reconnects fresh to our new
      profile, exactly like the manual BT toggle. Next tick it is disconnected and gets
      the Device1.Connect above."""

    stale = {}          # mac -> consecutive "connected but no session" ticks
    last_connect = {}   # mac -> monotonic time of the last Device1.Connect nudge
    connect_cooldown = 45.0  # Device1.Connect is async + slow; never overlap/spam it

    def _connect_async(mac):
        # Fire-and-forget: Device1.Connect blocks for a cold phone and bluez keeps
        # trying after our timeout, so run it off the loop and just log the outcome.
        ok, err = connect(adapter, mac, timeout=25)
        log(f"reconnect: {mac} " + ("connected" if ok else f"connect nudge failed: {err}"))

    def _loop():
        while True:
            time.sleep(interval)
            try:
                active = is_active()
                for d in list_paired(adapter):
                    mac = d["mac"]
                    if not d["connected"]:
                        stale[mac] = 0
                        if active:
                            continue
                        # A disconnected CarPlay phone auto-connects on its own when it is
                        # ready; just give it an occasional gentle nudge (no spam).
                        now = time.monotonic()
                        if now - last_connect.get(mac, -1e9) < connect_cooldown:
                            continue
                        last_connect[mac] = now
                        log(f"reconnect: {mac} paired+disconnected -> Device1.Connect (nudge)")
                        threading.Thread(target=_connect_async, args=(mac,), daemon=True).start()
                        continue
                    # connected
                    if active:
                        stale[mac] = 0
                        continue
                    stale[mac] = stale.get(mac, 0) + 1
                    if stale[mac] >= stale_ticks:
                        log(f"reconnect: {mac} connected but no session -> Disconnect (clear stale)")
                        disconnect(adapter, mac)
                        stale[mac] = 0
            except Exception as e:
                log(f"reconnect worker error: {e!r}")

    threading.Thread(target=_loop, daemon=True, name="bt-reconnect").start()
