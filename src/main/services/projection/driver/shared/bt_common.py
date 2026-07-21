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


def connect(adapter, mac, uuid=None, timeout=40):
    """With uuid, Device1.ConnectProfile(uuid); otherwise Device1.Connect."""
    dev = _dev_path(adapter, mac)
    call = ["--timeout=%d" % int(timeout), "call", "org.bluez", dev, "org.bluez.Device1"]
    if uuid:
        call += ["ConnectProfile", "s", uuid]
    else:
        call += ["Connect"]
    rc, _, err = _busctl(*call, timeout=timeout + 5)
    return rc == 0, err.strip()


def is_connect_in_progress(err):
    """BlueZ is already attempting this device, so the call was not a failure."""
    return "In Progress" in (err or "")


def is_timeout(err):
    return "TimeoutExpired" in (err or "") or "Connection timed out" in (err or "")


# Rounds of 'In Progress' after which BlueZ's attempt is treated as wedged.
_STUCK_ROUNDS = 3

# A wake-up is a short profile connect, so it fails fast when the phone is not there.
WAKE_TIMEOUT_S = 10
# Phones commonly answer only on the second or third try, so burst before backing off.
BURST_ATTEMPTS = 5
BURST_INTERVAL_S = 3.0


def disconnect(adapter, mac, timeout=10):
    rc, _, err = _busctl("call", "org.bluez", _dev_path(adapter, mac),
                         "org.bluez.Device1", "Disconnect", timeout=timeout)
    return rc == 0, err.strip()


def start_reconnect_worker(adapter, is_active, log, interval=2.0, stale_secs=10.0,
                           should_nudge=None, profile_for=None, max_backoff=60.0,
                           wake_timeout=WAKE_TIMEOUT_S, burst_attempts=BURST_ATTEMPTS,
                           burst_interval=BURST_INTERVAL_S):
    """Daemon thread. Every `interval`s, for each paired device (both branches skipped
    while `is_active()` is true):

    - disconnected -> Device1.Connect/ConnectProfile nudge.
    - connected but no session for `stale_secs` -> Device1.Disconnect.

    `should_nudge(mac)` selects which macs to nudge; `profile_for(mac)` gives the
    ConnectProfile UUID (None -> generic Connect).

    Waking a phone usually needs a couple of quick attempts, so a device gets a burst of
    `burst_attempts` tries `burst_interval` apart before its retry delay starts doubling
    up to `max_backoff`. A success resets both.

    Only ONE attempt runs at a time across all devices: the controller can page a single
    device at a time, so parallel attempts only starve each other."""

    inflight = set()      # macs with a nudge thread running (at most one)
    stale_since = {}      # mac -> monotonic time it went connected-but-no-session
    backoff = {}          # mac -> current retry delay in seconds
    next_try = {}         # mac -> monotonic time the next nudge is allowed
    stuck = {}            # mac -> consecutive 'In Progress' replies
    burst = {}            # mac -> attempts used in the current burst

    def _clear_pending(mac, why):
        """Abort BlueZ's own connection attempt, which otherwise blocks the device."""
        log(f"reconnect: {mac} clearing stuck connect attempt ({why})")
        disconnect(adapter, mac)

    def _connect_async(mac):
        try:
            uuid = profile_for(mac) if profile_for else None
            ok, err = connect(adapter, mac, uuid=uuid, timeout=wake_timeout)
            if ok:
                backoff.pop(mac, None)
                next_try.pop(mac, None)
                stuck.pop(mac, None)
                burst.pop(mac, None)
                log(f"reconnect: {mac} connected")
                return
            if is_connect_in_progress(err):
                # Not a failure, so it must not advance the backoff. But if BlueZ
                # stays busy for several rounds the attempt is wedged, so abort it.
                n = stuck.get(mac, 0) + 1
                stuck[mac] = n
                next_try[mac] = time.monotonic() + interval
                if n >= _STUCK_ROUNDS:
                    stuck.pop(mac, None)
                    _clear_pending(mac, f"{n} rounds in progress")
                else:
                    log(f"reconnect: {mac} already connecting, waiting")
                return
            stuck.pop(mac, None)
            if is_timeout(err):
                _clear_pending(mac, "connect timed out")
            tries = burst.get(mac, 0) + 1
            if tries < burst_attempts:
                burst[mac] = tries
                next_try[mac] = time.monotonic() + burst_interval
                log(f"reconnect: {mac} wake-up attempt {tries}/{burst_attempts} failed: {err}")
                return
            burst.pop(mac, None)
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
                        if inflight:
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
