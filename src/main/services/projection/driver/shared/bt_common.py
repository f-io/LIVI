"""Shared BlueZ device management + reconnect worker for the LIVI wireless helpers.

Enumerate paired devices, Device1.Connect/ConnectProfile, Device1.Disconnect, and a
periodic worker that nudges bonded phones back onto a session.
"""

import os
import subprocess
import threading
import time

_DBG = os.environ.get("LIVI_CP_DEBUG", os.environ.get("DEBUG", "")) not in ("", "0", "false", "False")


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


def connect(adapter, mac, uuid=None, timeout=15):
    """With uuid, Device1.ConnectProfile(uuid); otherwise Device1.Connect."""
    dev = _dev_path(adapter, mac)
    if uuid:
        rc, _, err = _busctl("call", "org.bluez", dev,
                             "org.bluez.Device1", "ConnectProfile", "s", uuid, timeout=timeout)
    else:
        rc, _, err = _busctl("call", "org.bluez", dev,
                             "org.bluez.Device1", "Connect", timeout=timeout)
    return rc == 0, err.strip()


def disconnect(adapter, mac, timeout=10):
    rc, _, err = _busctl("call", "org.bluez", _dev_path(adapter, mac),
                         "org.bluez.Device1", "Disconnect", timeout=timeout)
    return rc == 0, err.strip()


def start_reconnect_worker(adapter, is_active, log, interval=2.0, stale_secs=10.0,
                           should_nudge=None, profile_for=None, max_backoff=60.0):
    """Daemon thread. Every `interval`s, for each paired device (both branches skipped
    while `is_active()` is true):

    - disconnected -> Device1.Connect/ConnectProfile nudge (one in flight per device).
    - connected but no session for `stale_secs` -> Device1.Disconnect.

    `should_nudge(mac)` selects which macs to nudge; `profile_for(mac)` gives the
    ConnectProfile UUID (None -> generic Connect).

    A failed nudge doubles that device's retry delay up to `max_backoff`, so a phone
    that is out of range falls back to one attempt per minute instead of hammering
    BlueZ. A success resets it."""

    inflight = set()      # macs with a nudge thread running
    stale_since = {}      # mac -> monotonic time it went connected-but-no-session
    backoff = {}          # mac -> current retry delay in seconds
    next_try = {}         # mac -> monotonic time the next nudge is allowed

    def _connect_async(mac):
        try:
            uuid = profile_for(mac) if profile_for else None
            ok, err = connect(adapter, mac, uuid=uuid, timeout=25)
            if ok:
                backoff.pop(mac, None)
                next_try.pop(mac, None)
                log(f"reconnect: {mac} connected")
            else:
                delay = min(max_backoff, max(interval, backoff.get(mac, interval) * 2))
                backoff[mac] = delay
                next_try[mac] = time.monotonic() + delay
                log(f"reconnect: {mac} connect nudge failed: {err} (retry in {delay:.0f}s)")
        finally:
            inflight.discard(mac)

    def _loop():
        while True:
            time.sleep(interval)
            try:
                active = is_active()
                paired = list_paired(adapter)
                if _DBG:
                    log("reconnect tick: paired=%d active=%s [%s]" % (
                        len(paired), active,
                        ", ".join("%s:%s" % (d["mac"], "conn" if d["connected"] else "disc")
                                  for d in paired)))
                for d in paired:
                    mac = d["mac"]
                    if not d["connected"]:
                        stale_since.pop(mac, None)
                        if active:
                            if _DBG:
                                log("reconnect: %s skip (is_active true)" % mac)
                            continue
                        if should_nudge and not should_nudge(mac):
                            if _DBG:
                                log("reconnect: %s skip (not known for enabled protocol)" % mac)
                            continue
                        if mac in inflight:
                            continue
                        if time.monotonic() < next_try.get(mac, 0.0):
                            continue
                        inflight.add(mac)
                        log(f"reconnect: {mac} paired+disconnected -> nudge")
                        threading.Thread(target=_connect_async, args=(mac,), daemon=True).start()
                        continue
                    # connected
                    if active or (should_nudge and not should_nudge(mac)):
                        stale_since.pop(mac, None)
                        continue
                    t0 = stale_since.setdefault(mac, time.monotonic())
                    if time.monotonic() - t0 >= stale_secs:
                        log(f"reconnect: {mac} connected but no session -> Disconnect (clear stale)")
                        disconnect(adapter, mac)
                        stale_since.pop(mac, None)
            except Exception as e:
                log(f"reconnect worker error: {e!r}")

    threading.Thread(target=_loop, daemon=True, name="bt-reconnect").start()
