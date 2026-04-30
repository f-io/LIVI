#!/usr/bin/env python3
"""
aa-bluetooth.py — Android Auto: WiFi AP + Bluetooth profile manager

Startup sequence:
  1. wifi_ap.setup_ap()  — kills NM/wpa_supplicant, brings up hostapd+dnsmasq.
  2. bluetoothd -P *     — drop-in disables BlueZ plugins so only the SDP
                           records we register through ProfileManager1 are
                           on the wire.
  3. rfkill unblock      — unblock the radio if it's soft-blocked.
  4. BlueZ D-Bus setup   — register AA RFCOMM profile (ServiceRecord inline),
                           HFP HF profile, pairing agent, and set Adapter1
                           Powered/Discoverable/Pairable.
  5. RFCOMM WiFi exchange — 5-stage credential handshake when the phone
                            connects on channel 8.

"""

import os
import sys
import json
import struct
import socket
import subprocess
import threading
import time
import traceback

import select

import dbus
import dbus.service
import dbus.mainloop.glib
from gi.repository import GLib

from wifi_ap import setup_ap, teardown_ap, get_wlan_mac, AP_IP
from config import SSID, PASSPHRASE, CHANNEL, PORT as AA_PORT, BTNAME, WIFI_IFACE, BT_ADAPTER

def _power_on_bt() -> None:
    """rfkill unblock; D-Bus Adapter1.Set('Powered',True) handles the rest."""
    try:
        subprocess.run(["rfkill", "unblock", "bluetooth"],
                       capture_output=True, check=False)
    except FileNotFoundError:
        pass


def _busctl_get_property(path: str, iface: str, prop: str,
                         timeout: float = 5.0) -> str:
    """
    Read a D-Bus property via `busctl get-property`. Returns "" on error.
    """
    try:
        result = subprocess.run(
            ["busctl", "--system", "get-property",
             "org.bluez", path, iface, prop],
            capture_output=True, text=True, timeout=timeout,
        )
        return result.stdout.strip() if result.returncode == 0 else ""
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        return ""

_DROPIN_DIR        = "/etc/systemd/system/bluetooth.service.d"
_DROPIN_CFG        = os.path.join(_DROPIN_DIR, "livi-noplugins.conf")

def _find_bluetoothd() -> str:
    """Return the bluetoothd binary path from the running service unit."""
    for p in ("/usr/libexec/bluetooth/bluetoothd",
              "/usr/lib/bluetooth/bluetoothd",
              "/usr/sbin/bluetoothd"):
        if os.path.isfile(p):
            return p
    try:
        out = subprocess.check_output(
            ["systemctl", "cat", "bluetooth.service"], text=True, stderr=subprocess.DEVNULL
        )
        for line in out.splitlines():
            line = line.strip()
            if line.startswith("ExecStart=") and "bluetoothd" in line:
                binary = line.split("=", 1)[1].split()[0]
                if os.path.isfile(binary):
                    return binary
    except Exception:
        pass
    raise RuntimeError("Cannot find bluetoothd binary")


def _ensure_noplugins_dropin() -> bool:
    """Write the `bluetoothd -P *` drop-in. Returns True if a restart is needed.

    Also unlinks the older `livi-compat.conf` drop-in if upgrading from the
    sdp_clean-era setup. Both files would otherwise stack ExecStart= overrides
    in alphabetical order and produce a daemon command line we don't want.
    """
    bluetoothd = _find_bluetoothd()
    content = (
        "[Service]\n"
        "ExecStart=\n"
        f"ExecStart={bluetoothd} -P *\n"
    )

    restart_needed = False

    try:
        current = open(_DROPIN_CFG).read() if os.path.exists(_DROPIN_CFG) else ""
    except OSError:
        current = ""

    if current != content:
        os.makedirs(_DROPIN_DIR, exist_ok=True)
        with open(_DROPIN_CFG, "w") as f:
            f.write(content)
        print(f"[aa-bt] wrote {_DROPIN_CFG} ({bluetoothd} -P *)")
        restart_needed = True
    else:
        print(f"[aa-bt] bluetoothd -P * drop-in already in place ({bluetoothd})")

    return restart_needed


_MAIN_CONF = "/etc/bluetooth/main.conf"
_BT_CLASS = "0x200418"  # Audio service + Audio/Video major + Headphones-slot minor


def _ensure_main_conf_class() -> bool:
    """Make sure [General] Class = 0x200418 is set in /etc/bluetooth/main.conf.

    BlueZ uses this as the base CoD; the service-class bits derived from
    registered profiles are OR'd on top. Without it the adapter ends up at
    0x600000 (only service bits), which Android does not classify as a
    carkit and so its phone-side auto-reconnect-to-known-carkit path never
    triggers after boot.

    Returns True iff the file was modified.
    """
    desired = f"Class = {_BT_CLASS}"
    try:
        original = open(_MAIN_CONF).read()
    except OSError as e:
        print(f"[aa-bt] {_MAIN_CONF} read failed: {e}", flush=True)
        return False

    lines = original.splitlines()
    out: list[str] = []
    in_general = False
    seen_class_in_general = False

    for line in lines:
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            # Leaving [General] without an active Class= line → inject.
            if in_general and not seen_class_in_general:
                out.append(desired)
                seen_class_in_general = True
            in_general = stripped == "[General]"
            out.append(line)
            continue

        if in_general:
            cleaned = stripped.lstrip("#").strip()
            if cleaned.startswith("Class") and "=" in cleaned:
                if seen_class_in_general:
                    continue  # drop duplicate
                out.append(desired)
                seen_class_in_general = True
                continue
        out.append(line)

    if in_general and not seen_class_in_general:
        out.append(desired)
        seen_class_in_general = True

    if not seen_class_in_general:
        # No [General] section at all — append one
        if out and out[-1].strip():
            out.append("")
        out.append("[General]")
        out.append(desired)

    new = "\n".join(out)
    if not new.endswith("\n"):
        new += "\n"
    if new == original:
        return False

    try:
        with open(_MAIN_CONF, "w") as f:
            f.write(new)
        print(f"[aa-bt] {_MAIN_CONF}: Class = {_BT_CLASS}", flush=True)
        return True
    except OSError as e:
        print(f"[aa-bt] {_MAIN_CONF} write failed: {e}", flush=True)
        return False


def setup_bluetoothd_dropin():
    """Ensure the -P * systemd drop-in AND main.conf Class are in place.
    Restart bluetoothd once if either changed."""
    dropin_changed = _ensure_noplugins_dropin()
    class_changed = _ensure_main_conf_class()
    if dropin_changed or class_changed:
        print("[aa-bt] restarting bluetoothd to apply config changes …", flush=True)
        subprocess.run(["systemctl", "daemon-reload"], check=False)
        subprocess.run(["systemctl", "restart", "bluetooth"], check=True)
        time.sleep(5)
        print("[aa-bt] bluetoothd restarted", flush=True)

# ── Protobuf helpers ──────────────────────

def _encode_varint(value: int) -> bytes:
    out = []
    while value > 0x7F:
        out.append((value & 0x7F) | 0x80)
        value >>= 7
    out.append(value & 0x7F)
    return bytes(out)

def _pb_string(field: int, s: str) -> bytes:
    tag  = _encode_varint((field << 3) | 2)
    data = s.encode()
    return tag + _encode_varint(len(data)) + data

def _pb_varint(field: int, value: int) -> bytes:
    return _encode_varint((field << 3) | 0) + _encode_varint(value)


def _build_wifi_version_request() -> bytes:
    """WifiVersionRequest (msgId=4): empty proto body."""
    return struct.pack(">HH", 0, 4)

def _build_wifi_start_request(ip: str, port: int) -> bytes:
    proto = _pb_string(1, ip) + _pb_varint(2, port)
    # Frame: [2B proto_len][2B msgId][proto] — length = proto bytes ONLY (not +2)
    return struct.pack(">HH", len(proto), 1) + proto

def _channel_to_freq_mhz(channel: int) -> int:
    """Convert 802.11 channel number to frequency in MHz."""
    if channel <= 13:
        return 2412 + (channel - 1) * 5   # 2.4 GHz
    if channel == 14:
        return 2484
    if 36 <= channel <= 177:
        return 5180 + (channel - 36) * 5  # 5 GHz
    return 5180  # fallback

def _build_wifi_info_response(ssid: str, key: str, bssid: str) -> bytes:
    proto = (
        _pb_string(1, ssid) +
        _pb_string(2, key)  +
        _pb_string(3, bssid) +
        _pb_varint(4, 8)    +   # securityMode = WPA2_PERSONAL
        _pb_varint(5, 1)        # AccessPointType = DYNAMIC
    )
    print(f"[aa-bt]   WifiInfoResponse: ssid={ssid!r} bssid={bssid} "
          f"securityMode=WPA2_PERSONAL(8) type=DYNAMIC(1)", flush=True)
    return struct.pack(">HH", len(proto), 3) + proto

# ── RFCOMM socket helpers ──────────────────────────────────────────────────────

def _recv_exactly(sock: socket.socket, n: int) -> bytes:
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("socket closed mid-read")
        buf += chunk
    return buf

def _recv_frame(sock: socket.socket) -> tuple[int, bytes]:
    header = _recv_exactly(sock, 4)
    length, msg_id = struct.unpack(">HH", header)
    # length = proto bytes only (does NOT include the 2-byte msgId)
    proto = _recv_exactly(sock, length) if length > 0 else b""
    return msg_id, proto

# ── WiFi handshake ─────────────────────────────────────────────────────────────

def _is_tcp_listening(port: int) -> bool:
    """Check if a TCP port is in LISTEN state via /proc/net/tcp (no connection created)."""
    hex_port = f"{port:04X}"
    try:
        with open("/proc/net/tcp") as f:
            for line in f:
                parts = line.split()
                if len(parts) > 3 and parts[3] == "0A":
                    if parts[1].endswith(f":{hex_port}"):
                        return True
    except OSError:
        pass
    return False


def _wait_for_tcp_server(ip: str, port: int, timeout_s: float = 30.0, interval_s: float = 0.3) -> bool:
    """Block until port is in LISTEN state or timeout expires."""
    deadline = time.monotonic() + timeout_s
    first = True
    while time.monotonic() < deadline:
        if _is_tcp_listening(port):
            if not first:
                print(f"[aa-bt]   TCP server :{port} ready (LISTEN)", flush=True)
            return True
        if first:
            print(f"[aa-bt]   Waiting for TCP server :{port} to be LISTEN (up to {timeout_s:.0f}s)...", flush=True)
            first = False
        time.sleep(interval_s)
    print(f"[aa-bt]   TCP server not ready after {timeout_s:.0f}s — sending WifiStartRequest anyway", flush=True)
    return False




def _run_wifi_handshake(sock: socket.socket, mac: str):
    """
    WiFi credential exchange over RFCOMM.

    Full WPP sequence on current phones:
      0. HU → Phone  WifiVersionRequest   (msgId=4, version + freq)
      1. Phone → HU  WifiVersionResponse  (msgId=5, phone's version)
      2. HU → Phone  WifiStartRequest     (msgId=1, our IP + TCP port)
      3. Phone → HU  WifiInfoRequest      (msgId=2, empty — phone asks for AP creds)
      4. HU → Phone  WifiInfoResponse     (msgId=3, SSID/passphrase/BSSID)
      5. Phone → HU  WifiStartResponse    (msgId=7, ack)
      6. Phone → HU  WifiConnectionStatus (msgId=6, status=0 → joined AP)

    """
    print(f"[aa-bt] handshake thread entered (mac={mac})", flush=True)
    try:
        bssid = get_wlan_mac(WIFI_IFACE)
        print(f"[aa-bt] RFCOMM connected — WiFi handshake", flush=True)
        print(f"[aa-bt]   HOST={AP_IP}:{AA_PORT}  SSID={SSID}  BSSID={bssid}", flush=True)

        _wait_for_tcp_server(AP_IP, AA_PORT, timeout_s=30.0)

        # Step 0: WifiVersionRequest (msgId=4, empty proto body)
        sock.sendall(_build_wifi_version_request())
        print("[aa-bt]   → WifiVersionRequest sent (msgId=4)", flush=True)

        sock.settimeout(10.0)
        try:
            msg_id, data = _recv_frame(sock)
            print(f"[aa-bt]   ← msgId={msg_id}  ({len(data)} bytes)", flush=True)
            if msg_id == 5:
                print(f"[aa-bt]   WifiVersionResponse: {data.hex()}", flush=True)
            else:
                print(f"[aa-bt]   ! Expected WifiVersionResponse(5), got msgId={msg_id} — continuing", flush=True)
                _pending = (msg_id, data)
        except socket.timeout:
            print("[aa-bt]   no WifiVersionResponse within 10s — continuing", flush=True)
            _pending = None
        else:
            _pending = None if msg_id == 5 else (msg_id, data)

        # Step 2: send WifiStartRequest
        sock.sendall(_build_wifi_start_request(AP_IP, AA_PORT))
        print("[aa-bt]   → WifiStartRequest sent (msgId=1)", flush=True)

        # Steps 3-6: receive up to 4 messages until WifiConnectionStatus
        sock.settimeout(30.0)
        iterations = 0
        max_iterations = 5
        while iterations < max_iterations:
            iterations += 1
            if _pending is not None:
                msg_id, data = _pending
                _pending = None
            else:
                msg_id, data = _recv_frame(sock)
                print(f"[aa-bt]   ← msgId={msg_id}  ({len(data)} bytes)", flush=True)

            if msg_id == 2:      # WifiInfoRequest — phone asks for AP credentials
                sock.sendall(_build_wifi_info_response(SSID, PASSPHRASE, bssid))
                print(f"[aa-bt]   → WifiInfoResponse sent (SSID={SSID!r})", flush=True)

            elif msg_id == 7:    # WifiStartResponse — ack
                print("[aa-bt]   WifiStartResponse (ack)", flush=True)

            elif msg_id == 6:    # WifiConnectionStatus — credentials delivered
                status = data[1] if len(data) >= 2 else 0
                if status == 0:
                    print(f"[aa-bt]   ✓ WifiConnectionStatus OK — phone joining {SSID!r}", flush=True)
                else:
                    print(f"[aa-bt]   ✗ WifiConnectionStatus FAIL (status={status})", flush=True)
                break

            elif msg_id == 5:    # late WifiVersionResponse
                print(f"[aa-bt]   WifiVersionResponse (late): {data.hex()}", flush=True)

            else:
                print(f"[aa-bt]   unknown msgId={msg_id} len={len(data)} — ignoring", flush=True)

        # The RFCOMM socket stays alive until BT disconnects.  Closing it here
        # causes WPP_SOCKET_IO_EXCEPTION → WPP restart → RFCOMM retry loop.
        print(f"[aa-bt]   handshake complete — holding RFCOMM open until BT disconnect", flush=True)
        sock.settimeout(5.0)
        while True:
            try:
                data = sock.recv(64)
                if not data:
                    print(f"[aa-bt]   RFCOMM closed by phone (BT disconnect)", flush=True)
                    break
            except socket.timeout:
                continue   # keepalive — loop back
            except OSError as e:
                print(f"[aa-bt]   RFCOMM hold ended: {e}", flush=True)
                break

    except ConnectionError as e:
        print(f"[aa-bt] RFCOMM disconnected during handshake: {e}", flush=True)
    except Exception as e:
        print(f"[aa-bt] RFCOMM handshake error: {e}", flush=True)
        traceback.print_exc()
    finally:
        try:
            sock.close()
        except Exception:
            pass

# ── HFP Hands-Free (car kit role) ─────────────────────────────────────────────
#
# HFP SLC sequence (HFP 1.8, mandatory steps):
#   HF → AG : AT+BRSF=<HF features>
#   AG → HF : +BRSF:<AG features>  then  OK
#   (if both codec neg) HF → AG : AT+BAC=1,2   then OK
#   HF → AG : AT+CIND=?           (indicator mapping)
#   AG → HF : +CIND:<map>         then OK
#   HF → AG : AT+CIND?            (current indicator values)  ← mandatory
#   AG → HF : +CIND:<values>      then OK
#   HF → AG : AT+CMER=3,0,0,1    (enable indicator events)
#   AG → HF : OK                  ← SLC established
#
# HFP HF feature bitmask:
#   Bit 2: CLI presentation, Bit 3: Voice recognition, Bit 4: Remote volume,
#   Bit 7: Codec negotiation (mSBC/CVSD) = 156
_HFP_HF_FEATURES = (1 << 2) | (1 << 3) | (1 << 4) | (1 << 7)


def _handle_hfp_rfcomm(fd: int, device_path: str, initial_data: bytes = b"") -> None:
    """Full HFP HF SLC setup + AT command response loop."""
    global hfp_slc_established

    import fcntl as _fcntl
    fl = _fcntl.fcntl(fd, _fcntl.F_GETFL)
    _fcntl.fcntl(fd, _fcntl.F_SETFL, fl & ~os.O_NONBLOCK)

    def _send(s: str) -> None:
        os.write(fd, s.encode("utf-8"))

    def _send_ok() -> None:
        _send("\r\nOK\r")

    # SLC state machine
    ag_features: int = 0
    _sent_bac:        bool = False
    _sent_cind_test:  bool = False
    _sent_cind_read:  bool = False
    _sent_cmer:       bool = False

    try:
        if initial_data:
            # Probe already sent AT+BRSF and read initial response
            print(f"[hfp] SLC continuation from probe ({len(initial_data)} bytes in buffer)", flush=True)
        else:
            # HF must speak first: send AT+BRSF
            _send(f"AT+BRSF={_HFP_HF_FEATURES}\r")
            print(f"[hfp] >> AT+BRSF={_HFP_HF_FEATURES}", flush=True)

        buf = initial_data
        while True:
            # Only block on read if there is no complete line already in buf
            if b"\r" not in buf:
                ready, _, _ = select.select([fd], [], [], 300.0)
                if not ready:
                    print("[hfp] AT data timeout (300s) — disconnecting", flush=True)
                    break
                data = os.read(fd, 1024)
                if not data:
                    break
                buf += data

            while b"\r" in buf:
                line_raw, buf = buf.split(b"\r", 1)
                line = line_raw.decode("utf-8", errors="replace").strip()
                if not line:
                    continue
                print(f"[hfp] << {line}", flush=True)

                # ── AG responses to our commands ─────────────────────────────
                if line.startswith("+BRSF:"):
                    try:
                        ag_features = int(line.split(":")[1].strip())
                    except ValueError:
                        pass
                    print(f"[hfp]    AG features: {ag_features}", flush=True)

                elif line == "OK":
                    if not hfp_slc_established:
                        # Drive SLC state machine forward
                        _both_codec_neg = (
                            bool(_HFP_HF_FEATURES & (1 << 7)) and
                            bool(ag_features & (1 << 9))
                        )
                        if ag_features > 0 and _both_codec_neg and not _sent_bac:
                            _sent_bac = True
                            _send("AT+BAC=1,2\r")
                            print("[hfp] >> AT+BAC=1,2 (CVSD, mSBC)", flush=True)
                        elif ag_features > 0 and not _sent_cind_test:
                            _sent_cind_test = True
                            _send("AT+CIND=?\r")
                            print("[hfp] >> AT+CIND=?", flush=True)
                        elif _sent_cind_test and not _sent_cind_read:
                            _sent_cind_read = True
                            _send("AT+CIND?\r")
                            print("[hfp] >> AT+CIND?", flush=True)
                        elif _sent_cind_read and not _sent_cmer:
                            _sent_cmer = True
                            _send("AT+CMER=3,0,0,1\r")
                            print("[hfp] >> AT+CMER=3,0,0,1", flush=True)
                        elif _sent_cmer:
                            hfp_slc_established = True
                            print("[hfp] ✓ SLC established — Android Auto should trigger", flush=True)

                elif line.startswith("+CIND:"):
                    print(f"[hfp]    indicators: {line}", flush=True)

                elif line == "ERROR":
                    print("[hfp] AG returned ERROR", flush=True)

                # ── AG-initiated AT commands (phone asks us) ─────────────────
                elif line.startswith("AT+BRSF="):
                    try:
                        ag_features = int(line.split("=")[1])
                    except ValueError:
                        pass
                    _send(f"\r\n+BRSF: {_HFP_HF_FEATURES}")
                    _send_ok()

                elif line == "AT+CIND=?":
                    _send('\r\n+CIND: ("service",(0,1)),("call",(0,1)),'
                          '("callsetup",(0-3)),("callheld",(0-2)),'
                          '("signal",(0-5)),("roam",(0,1)),("battchg",(0-5))')
                    _send_ok()

                elif line == "AT+CIND?":
                    _send("\r\n+CIND: 1,0,0,0,5,0,5")
                    _send_ok()

                elif line.startswith("AT+CMER="):
                    _send_ok()

                elif line.startswith("AT+CHLD=?"):
                    _send("\r\n+CHLD: (0,1,2,3)")
                    _send_ok()

                elif line.startswith("AT+BIND=?"):
                    _send("\r\n+BIND: (1,2)")
                    _send_ok()

                elif line.startswith("AT+BIND?"):
                    _send("\r\n+BIND: 1,1")
                    _send("\r\n+BIND: 2,1")
                    _send_ok()

                elif line.startswith("AT+BIND="):
                    _send_ok()

                elif line.startswith("AT+BAC="):
                    print(f"[hfp]    codecs: {line.split('=')[1]}", flush=True)
                    _send_ok()

                elif line.startswith("+BCS:"):
                    codec = line.split(":")[1].strip()
                    print(f"[hfp]    codec selected: {codec}", flush=True)
                    _send(f"AT+BCS={codec}\r")

                elif line == "ATA":
                    _send_ok()
                elif line == "AT+CHUP":
                    _send_ok()
                elif line.startswith("ATD"):
                    print(f"[hfp]    dial: {line[3:].rstrip(';')}", flush=True)
                    _send_ok()
                elif line.startswith("AT+BVRA="):
                    _send_ok()
                elif line.startswith("AT+VGS="):
                    _send_ok()
                elif line.startswith("AT+VGM="):
                    _send_ok()
                elif line.startswith("AT+NREC="):
                    _send_ok()
                elif line.startswith("AT+BTRH?"):
                    _send_ok()
                elif line.startswith("AT+CLIP="):
                    _send_ok()
                elif line.startswith("AT+CCWA="):
                    _send_ok()
                elif line.startswith("AT+CMEE="):
                    _send_ok()
                elif line.startswith("AT+CLCC"):
                    _send_ok()
                elif line.startswith("AT+COPS"):
                    if "=?" in line:
                        _send_ok()
                    elif "?" in line:
                        _send('\r\n+COPS: 0,0,"Carrier"')
                        _send_ok()
                    else:
                        _send_ok()
                elif line.startswith("AT+CNUM"):
                    _send_ok()

                # ── Unsolicited result codes (no response needed) ────────────
                elif line.startswith("+CIEV:") or line == "RING" or line.startswith("+CLIP:"):
                    print(f"[hfp]    unsolicited: {line}", flush=True)

                else:
                    # Unknown AT command — respond OK to prevent phone disconnect
                    print(f"[hfp]    unknown AT: {line!r} — sending OK", flush=True)
                    _send_ok()

    except Exception as e:
        print(f"[hfp] error: {e}", flush=True)
    finally:
        try:
            os.close(fd)
        except OSError:
            pass
        hfp_slc_established = False
        print("[hfp] disconnected", flush=True)


# ── D-Bus constants ────────────────────────────────────────────────────────────

PROFILE_IFACE   = "org.bluez.Profile1"
AGENT_IFACE     = "org.bluez.Agent1"
PROFILE_MANAGER = "org.bluez.ProfileManager1"
AGENT_MANAGER   = "org.bluez.AgentManager1"
BLUEZ_SERVICE   = "org.bluez"
BLUEZ_OBJ       = "/org/bluez"
ADAPTER_PATH    = f"/org/bluez/{BT_ADAPTER}"

AA_UUID  = "4de17a00-52cb-11e6-bdf4-0800200c9a66"
HFP_HF_UUID = "0000111e-0000-1000-8000-00805f9b34fb"  # HFP Hands-Free (car kit role)
HFP_AG_UUID = "0000111f-0000-1000-8000-00805f9b34fb"  # HFP Audio Gateway (phone role)
HSP_UUID    = "00001108-0000-1000-8000-00805f9b34fb"   # HSP Headset

# AA SDP record — passed inline as ServiceRecord on RegisterProfile so BlueZ
# publishes it directly via its D-Bus-driven SDP server (no /var/run/sdp,
# no libbluetooth helper). Contents:
#   0x0001 ServiceClassIDList    — only the 128-bit AA UUID (no SPP, no
#                                  16-bit fillers — Android's SDP parser
#                                  rejects mixed UUID sizes in one record)
#   0x0004 ProtocolDescriptorList — L2CAP + RFCOMM channel 8
#   0x0005 BrowseGroupList        — PublicBrowseRoot (so the phone finds it
#                                   on a generic SDP browse)
#   0x0100 ServiceName            — "Android Auto Wireless"
# Mirrors the iap2 approach (IAP_RECORD in iap2/transport/bluetooth.py).
AA_SDP_RECORD = """<?xml version="1.0" encoding="UTF-8" ?>
<record>
  <attribute id="0x0001">
    <sequence>
      <uuid value="4de17a00-52cb-11e6-bdf4-0800200c9a66" />
    </sequence>
  </attribute>
  <attribute id="0x0004">
    <sequence>
      <sequence>
        <uuid value="0x0100" />
      </sequence>
      <sequence>
        <uuid value="0x0003" />
        <uint8 value="0x08" />
      </sequence>
    </sequence>
  </attribute>
  <attribute id="0x0005">
    <sequence>
      <uuid value="0x1002" />
    </sequence>
  </attribute>
  <attribute id="0x0100">
    <text value="Android Auto Wireless" />
  </attribute>
</record>"""

_open_sockets: list = []  # holds HFP/HSP fds alive

# Device path and MAC of the last RFCOMM-connected phone.
_last_device_path: str = ""
_last_phone_mac:   str = ""

# HFP Service Level Connection state
hfp_slc_established: bool = False

# Set True only when HFP RegisterProfile succeeds.
_hfp_profile_registered: bool = False

# Raw RFCOMM HFP direct-connect state.
# Used to bypass BlueZ Profile API when UUID 0x111e is already registered.
_hfp_raw_lock  = threading.Lock()
_hfp_raw_last_at: float = 0.0
_HFP_RAW_COOLDOWN_SEC = 8.0   # min seconds between raw HFP connect attempts

# Cache of confirmed HFP AG RFCOMM channel per phone MAC.
# Once ch=3 is discovered, subsequent probes skip ch=1 and ch=2 entirely,
_hfp_cached_channel: dict[str, int] = {}  # mac → channel number

# ── BT reconnect worker ────────────────────────────────────────────────────────
# After the WPP credential exchange, RFCOMM closes and the TCP cleanup session
# ends quickly (~5s). The phone then needs a BT event to trigger Android Auto
# for the real session. Without active BT reconnect, the BT ACL link goes idle
# and drops. The phone then says "HU is not present" and gives up.
#
#   Monitor BT connection state. When the phone is known but disconnected,
#   reconnect BT from our side (Device1.Connect). This keeps BT alive and
#   triggers Android Auto on the phone, which leads to a real AA session.
#
# This is a periodic background service (like a daemon), NOT a one-shot timer.
# Interval: 5s — fast enough for 3-5s reconnect target without being aggressive.

def _bt_is_connected(mac: str) -> bool:
    """Return True if the phone's BT is currently connected to us.

    Reads org.bluez.Device1.Connected via busctl. Replaces the previous
    `bluetoothctl info <mac>` text-grep, which depended on bluetoothctl's
    output format and on a tool that may not be present on minimal images.
    busctl ships with systemd → always available where BlueZ runs.
    """
    path = f"{ADAPTER_PATH}/dev_{mac.replace(':', '_')}"
    out = _busctl_get_property(path, "org.bluez.Device1", "Connected")
    # busctl prints booleans as "b true" / "b false"
    return out.lower().endswith("true")


def _find_hfp_ag_channel(mac: str) -> int:
    """Return a best-guess HFP AG RFCOMM channel for the phone.

    Modern bluez-utils no longer ships `sdptool`, and there's no equivalent
    in btmgmt for browsing a remote device's SDP records (BlueZ keeps that
    capability internal to ConnectProfile). Doing it ourselves would mean
    pulling in libbluetooth-dev + a custom helper just for one channel
    number, which isn't worth it: the probe loop in _connect_hfp_direct
    already iterates ch=1..15 and caches the first one that answers
    AT+BRSF, so the second connect attempt onward goes straight to the
    right channel.

    Channel 1 is what Pixel 8 (and most current Android phones) use for HFP
    AG, so it's a strong first guess.
    """
    return 1


def _connect_hfp_direct(mac: str) -> None:
    """Connect to phone's HFP AG via raw AF_BLUETOOTH/BTPROTO_RFCOMM socket.

    Bypasses BlueZ's Profile API entirely (UUID 0x111e is already registered
    internally by BlueZ and can't be overridden). Opens a kernel-level RFCOMM
    socket directly to the phone, then runs _handle_hfp_rfcomm in a thread
    for the full HFP SLC AT command exchange.

    Once SLC is established: hfp_slc_established=True → Android Auto
    auto-starts on the phone → next TCP session is real (not cleanup).

    Has an internal cooldown to prevent rapid retry spam.
    """
    global _hfp_raw_last_at

    with _hfp_raw_lock:
        now = time.monotonic()
        elapsed = now - _hfp_raw_last_at
        if elapsed < _HFP_RAW_COOLDOWN_SEC:
            print(f"[hfp] direct-connect cooldown ({elapsed:.1f}s < {_HFP_RAW_COOLDOWN_SEC:.0f}s)", flush=True)
            return
        _hfp_raw_last_at = now

    def _do() -> None:
        # Use cached channel if available (avoids 3s ch=2 probe timeout on retry).
        cached = _hfp_cached_channel.get(mac)
        if cached:
            candidates = [cached] + [c for c in range(1, 16) if c != cached]
            print(f"[hfp] probing channels for HFP AG on {mac} (cached ch={cached})", flush=True)
        else:
            # try the most common default first, then scan the rest. Once a
            # channel succeeds it's cached so subsequent connects skip the scan.
            first = _find_hfp_ag_channel(mac)
            candidates = [first] + [c for c in range(1, 16) if c != first]
            print(f"[hfp] probing {len(candidates)} channels for HFP AG on {mac} "
                  f"(default→{first}, no SDP browse)", flush=True)

        for ch in candidates:
            sock = None
            fd = -1
            try:
                sock = socket.socket(socket.AF_BLUETOOTH, socket.SOCK_STREAM,
                                     socket.BTPROTO_RFCOMM)
                sock.settimeout(5.0)
                sock.connect((mac, ch))
                fd = sock.detach()   # transfer ownership to fd
                sock = None

                # Quick probe: send AT+BRSF, wait up to 3 s for +BRSF: response.
                # Channels that accept the RFCOMM connection but aren't HFP AG
                # will be silent — we time out and move on rather than blocking.
                probe = f"AT+BRSF={_HFP_HF_FEATURES}\r".encode()
                os.write(fd, probe)
                print(f"[hfp] ch={ch} connected — >> AT+BRSF={_HFP_HF_FEATURES} (probe)", flush=True)

                ready, _, _ = select.select([fd], [], [], 3.0)
                if not ready:
                    print(f"[hfp] ch={ch} no response to AT+BRSF — not HFP AG, skipping", flush=True)
                    os.close(fd)
                    fd = -1
                    continue

                initial_data = os.read(fd, 1024)
                if not initial_data:
                    print(f"[hfp] ch={ch} connection closed — skipping", flush=True)
                    try: os.close(fd)
                    except OSError: pass
                    fd = -1
                    continue

                print(f"[hfp] ch={ch} << {initial_data!r}", flush=True)
                if b"+BRSF" not in initial_data:
                    print(f"[hfp] ch={ch} no +BRSF in response — not HFP AG, skipping", flush=True)
                    os.close(fd)
                    fd = -1
                    continue

                # Found HFP AG — cache channel and launch full SLC thread
                _hfp_cached_channel[mac] = ch
                print(f"[hfp] ch={ch} HFP AG confirmed ✓ — starting SLC (cached)", flush=True)
                threading.Thread(target=_handle_hfp_rfcomm, args=(fd, mac, initial_data),
                                 daemon=True).start()
                return  # success

            except OSError as e:
                if e.errno == 111:  # ECONNREFUSED — channel not registered, skip silently
                    print(f"[hfp] ch={ch} refused (not registered)", flush=True)
                elif e.errno == 16 and ch == cached:
                    # EBUSY on the cached channel → HFP SLC already active on that channel.
                    # No need to probe further — SLC is already established.
                    print(f"[hfp] ch={ch} busy (SLC already active) — skipping probe", flush=True)
                    return
                else:
                    print(f"[hfp] ch={ch} error: {e}", flush=True)
                if fd >= 0:
                    try: os.close(fd)
                    except OSError: pass
                elif sock is not None:
                    try: sock.close()
                    except Exception: pass

        print(f"[hfp] all channels 1-15 exhausted for {mac} — HFP AG not found", flush=True)

    threading.Thread(target=_do, daemon=True).start()


# ── Device management helpers (BlueZ enumeration + actions) ───────────────────

def _device_path(mac: str) -> str:
    """Compose the BlueZ D-Bus device object path from a MAC address."""
    return f"{ADAPTER_PATH}/dev_{mac.replace(':', '_').upper()}"


def _busctl_parse_value(s: str) -> str | bool | int:
    """Parse a single-line busctl get-property output into a Python value.

    busctl prints the d-bus signature followed by the value, e.g.:
      's "Pixel 8"'         → "Pixel 8"
      'b true' / 'b false'  → True / False
      'u 5898764'           → 5898764
      'i -1'                → -1
    Anything we don't understand is returned as the trimmed raw string.
    """
    s = s.strip()
    if not s:
        return ""
    sig, _, rest = s.partition(" ")
    rest = rest.strip()
    if sig == "s" or sig == "o":
        # quoted string; strip surrounding double quotes if present
        if rest.startswith('"') and rest.endswith('"'):
            return rest[1:-1]
        return rest
    if sig == "b":
        return rest.lower() == "true"
    if sig in ("u", "i", "y", "n", "q", "x", "t"):
        try:
            return int(rest)
        except ValueError:
            return 0
    return rest


def _is_device_path(path: str) -> bool:
    """True iff path looks like /org/bluez/hciX/dev_AA_BB_CC_DD_EE_FF."""
    if "/dev_" not in path:
        return False
    suffix = path.rsplit("/dev_", 1)[1]
    parts = suffix.split("_")
    if len(parts) != 6:
        return False
    for p in parts:
        if len(p) != 2:
            return False
        try:
            int(p, 16)
        except ValueError:
            return False
    return True


def _list_paired_devices() -> list[dict]:
    """Enumerate all paired BlueZ devices via busctl.

    Worker-thread safe by design: every D-Bus interaction is a separate
    subprocess, so we never share a libdbus connection with the GLib main
    loop running on the main thread. Slower than ObjectManager.GetManagedObjects
    but reliable in this concurrency model.
    """
    # 1. Enumerate object paths under org.bluez.
    try:
        r = subprocess.run(
            ["busctl", "--system", "tree", "--list", "org.bluez"],
            capture_output=True, text=True, timeout=5,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired):
        return []
    if r.returncode != 0:
        return []

    device_paths = [ln.strip() for ln in r.stdout.splitlines()
                    if _is_device_path(ln.strip())]

    # 2. Pull the properties we care about for each device.
    interesting = ("Address", "Name", "Alias", "Paired",
                   "Connected", "Trusted", "Class")
    devices: list[dict] = []
    for path in device_paths:
        props: dict = {}
        for name in interesting:
            raw = _busctl_get_property(path, "org.bluez.Device1", name)
            if raw:
                props[name] = _busctl_parse_value(raw)

        if not bool(props.get("Paired", False)):
            continue

        display_name = (props.get("Name") or props.get("Alias") or "")
        cod = props.get("Class", 0)
        if not isinstance(cod, int):
            try: cod = int(cod)
            except (TypeError, ValueError): cod = 0

        devices.append({
            "mac":       str(props.get("Address", "")).upper(),
            "name":      str(display_name),
            "connected": bool(props.get("Connected", False)),
            "trusted":   bool(props.get("Trusted", False)),
            "class":     cod,
            "path":      path,
        })

    # Stable ordering: connected first, then alphabetical by name, then by MAC.
    devices.sort(key=lambda d: (not d["connected"], d["name"].lower(), d["mac"]))
    return devices


def _device_call(path: str, iface: str, method: str,
                 *busctl_args: str, timeout: float = 15.0) -> tuple[bool, str]:
    """Invoke a BlueZ method via busctl. Returns (ok, error_message).

    busctl is used in worker threads instead of dbus-python for one-shot
    method calls so we don't have to manage another private SystemBus just
    to make a single Connect/Disconnect call.
    """
    cmd = ["busctl", "--system", "call", "org.bluez", path, iface, method,
           *busctl_args]
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return False, f"timeout calling {iface}.{method}"
    except FileNotFoundError:
        return False, "busctl not installed"
    if r.returncode == 0:
        return True, ""
    return False, (r.stderr or r.stdout).strip()


def _device_connect(mac: str) -> tuple[bool, str]:
    """BlueZ Device1.Connect — establishes BT ACL + connects all profiles."""
    return _device_call(
        _device_path(mac), "org.bluez.Device1", "Connect",
        timeout=30.0,
    )


def _device_disconnect(mac: str) -> tuple[bool, str]:
    """BlueZ Device1.Disconnect — tears down the ACL."""
    return _device_call(_device_path(mac), "org.bluez.Device1", "Disconnect")


def _device_remove(mac: str) -> tuple[bool, str]:
    """BlueZ Adapter1.RemoveDevice — unpairs and forgets the device."""
    return _device_call(
        ADAPTER_PATH, "org.bluez.Adapter1", "RemoveDevice",
        "o", _device_path(mac),
    )


# ── IPC server (TS ↔ Python device management) ───────────────────────────────

_AA_EVENT_SOCK = "/tmp/aa-bt.sock"

# Persistent event-stream subscribers. Connections from the TS side that issued
# the `subscribe` command stay open here; we push one JSON line per BlueZ
# device-state change. Mutated only from the GLib main thread (where the
# signal callback runs) and from the accept-loop worker threads — guarded
# by _subscribers_lock.
_subscribers: list[socket.socket] = []
_subscribers_lock = threading.Lock()


def _push_event(payload: dict) -> None:
    """Send one JSON line to every active subscriber, drop dead ones."""
    line = (json.dumps(payload) + "\n").encode()
    with _subscribers_lock:
        dead: list[socket.socket] = []
        for s in _subscribers:
            try:
                s.sendall(line)
            except OSError:
                dead.append(s)
        for s in dead:
            try: _subscribers.remove(s)
            except ValueError: pass
            try: s.close()
            except Exception: pass


def _read_request(c: socket.socket, max_bytes: int = 4096,
                  timeout_s: float = 2.0) -> str:
    """Read one newline-terminated request from the client socket."""
    c.settimeout(timeout_s)
    buf = b""
    while b"\n" not in buf and len(buf) < max_bytes:
        try:
            chunk = c.recv(min(1024, max_bytes - len(buf)))
        except socket.timeout:
            break
        if not chunk:
            break
        buf += chunk
    return buf.decode(errors="replace").strip()


def _start_event_server() -> None:
    """Unix-socket request/response server for the TS-side device-management UI.

    One newline-terminated request per connection. Server replies with one
    newline-terminated JSON line, then closes.

    Commands:
      list_paired
        → {"ok": true, "devices": [
              {"mac": "AA:BB:CC:DD:EE:FF", "name": "Pixel 8",
               "connected": false, "trusted": true, "class": 5898764,
               "path": "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF"},
              ...
           ]}
      connect <mac>     → {"ok": true} or {"ok": false, "error": "..."}
      disconnect <mac>  → {"ok": true} or {"ok": false, "error": "..."}
      remove <mac>      → {"ok": true} or {"ok": false, "error": "..."}
    """
    try:
        os.unlink(_AA_EVENT_SOCK)
    except OSError:
        pass

    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(_AA_EVENT_SOCK)
    srv.listen(8)
    os.chmod(_AA_EVENT_SOCK, 0o666)  # allow non-root TypeScript process to connect
    print(f"[aa-bt] event server listening on {_AA_EVENT_SOCK}", flush=True)

    def _send_json(c: socket.socket, payload: dict) -> None:
        try:
            c.sendall((json.dumps(payload) + "\n").encode())
        except OSError:
            pass

    def _dispatch_command(cmd: str, arg: str) -> dict:
        """Run a device-management command. Always returns a JSON-able dict."""
        try:
            if cmd == "list_paired":
                return {"ok": True, "devices": _list_paired_devices()}
            if cmd == "connect":
                if not arg:
                    return {"ok": False, "error": "connect requires a MAC argument"}
                ok, err = _device_connect(arg)
                return {"ok": ok} if ok else {"ok": False, "error": err}
            if cmd == "disconnect":
                if not arg:
                    return {"ok": False, "error": "disconnect requires a MAC argument"}
                ok, err = _device_disconnect(arg)
                return {"ok": ok} if ok else {"ok": False, "error": err}
            if cmd == "remove":
                if not arg:
                    return {"ok": False, "error": "remove requires a MAC argument"}
                ok, err = _device_remove(arg)
                return {"ok": ok} if ok else {"ok": False, "error": err}
            return {"ok": False, "error": f"unknown command: {cmd!r}"}
        except Exception as e:
            traceback.print_exc()
            return {"ok": False, "error": f"{type(e).__name__}: {e}"}

    def _accept_loop() -> None:
        while True:
            try:
                conn, _ = srv.accept()
            except OSError:
                break  # server socket closed (shutdown)

            def _handle(c: socket.socket) -> None:
                # `keep_open` switches off the auto-close so a `subscribe`
                # connection survives past _handle's lifetime and stays in
                # the _subscribers list for event pushes.
                keep_open = False
                try:
                    raw = _read_request(c)
                    if not raw:
                        return
                    print(f"[aa-bt] sock req: {raw!r}", flush=True)

                    parts = raw.split(maxsplit=1)
                    cmd = parts[0]
                    arg = parts[1].strip() if len(parts) > 1 else ""

                    if cmd == "subscribe":
                        with _subscribers_lock:
                            _subscribers.append(c)
                        _send_json(c, {"ok": True, "subscribed": True})
                        keep_open = True
                        return

                    resp = _dispatch_command(cmd, arg)
                    _send_json(c, resp)

                except Exception as e:
                    print(f"[aa-bt] sock handler error: {e}", flush=True)
                    try: _send_json(c, {"ok": False, "error": str(e)})
                    except Exception: pass
                finally:
                    if not keep_open:
                        try: c.close()
                        except Exception: pass

            threading.Thread(target=_handle, args=(conn,), daemon=True).start()

    threading.Thread(target=_accept_loop, daemon=True, name="event-server").start()


def _subscribe_device_signals(bus: dbus.Bus) -> None:
    """Push a JSON event to subscribers when any Device1 property changes.

    Runs on the GLib main thread. Forward only the bits the TS side acts on:
    Connected and Paired flips drive list refreshes, anything else is ignored.
    """
    def _on_props_changed(interface, changed, invalidated, path=""):
        if interface != "org.bluez.Device1":
            return
        if "Connected" not in changed and "Paired" not in changed:
            return
        mac = ""
        if "/dev_" in path:
            mac = path.rsplit("/dev_", 1)[-1].replace("_", ":").upper()
        # When a device transitions to connected, mark it Trusted so BlueZ
        # persists it to /var/lib/bluetooth and accepts incoming reconnects
        # after reboot without re-pairing.
        if changed.get("Connected") is True:
            try:
                obj = bus.get_object("org.bluez", str(path))
                dbus.Interface(obj, dbus.PROPERTIES_IFACE).Set(
                    "org.bluez.Device1", "Trusted", True
                )
            except Exception:
                pass
        _push_event({"event": "device_changed", "mac": mac, "path": path})

    bus.add_signal_receiver(
        _on_props_changed,
        signal_name="PropertiesChanged",
        dbus_interface="org.freedesktop.DBus.Properties",
        path_keyword="path",
    )


def _trigger_hfp_slc_async(mac: str) -> None:
    """Trigger HFP SLC via direct raw RFCOMM (bypasses BlueZ Profile API).

    Called right after WifiConnectionStatus — the Pixel 8 retries RFCOMM
    ~3-4 seconds later. Raw RFCOMM connect + full AT exchange takes ~1-2s.
    This gives enough time for SLC to be established before the next RFCOMM
    so the phone says "user intends to start projection" → real TCP session.
    """
    _connect_hfp_direct(mac)


def _bt_reconnect_worker() -> None:
    """Daemon thread: keep phone BT + HFP SLC alive after pairing.

      • already_connected AND hfp_slc_established  → skip (all good)
      • already_connected AND NOT hfp_slc_established → try HFP SLC directly
        (ACL up but SLC not established — probe HFP channel, no BT disconnect)
      • not connected → ConnectProfile(HFP_AG)

    BT is NEVER disconnected here. Android Auto requires BT to stay connected
    (HFP for phone calls, profile continuity, CDM stability).
    """
    interval_s = 5.0
    while True:
        time.sleep(interval_s)

        mac = _last_phone_mac
        path = _last_device_path
        if not mac or not path:
            continue
        try:
            already_connected = _bt_is_connected(mac)

            if already_connected and hfp_slc_established:
                # All good — HFP SLC is up, leave connection alone
                continue

            if already_connected:
                # Phone is connected but HFP SLC not established.
                # Try raw RFCOMM HFP direct connect (has its own 8s cooldown).
                _connect_hfp_direct(mac)
                continue   # Don't also run ConnectProfile — causes br-connection-busy

            # Not connected: reconnect via ConnectProfile(HFP_AG).
            print(f"[aa-bt] BT not connected — ConnectProfile(HFP_AG) → {mac}", flush=True)
            result = subprocess.run(
                [
                    "busctl", "--system", "call",
                    "org.bluez", path,
                    "org.bluez.Device1", "ConnectProfile",
                    "s", HFP_AG_UUID,
                ],
                capture_output=True, text=True, timeout=15,
            )
            if result.returncode == 0:
                print(f"[aa-bt] ConnectProfile(HFP_AG) OK", flush=True)
            else:
                err = (result.stderr or result.stdout).strip()
                print(f"[aa-bt] ConnectProfile failed: {err}", flush=True)
        except Exception as e:
            print(f"[aa-bt] BT reconnect error: {e}", flush=True)


# ── RFCOMM state ──────────────────────────────────────────────────────────────
# No cooldown needed: the RFCOMM socket is kept open after WifiConnectionStatus.
# While open, BlueZ channel 8 is occupied → the phone
# cannot open a second RFCOMM connection → no WPP_SOCKET_IO_EXCEPTION loop.
# The socket closes naturally on BT disconnect, at which point the phone will
# reconnect via RFCOMM for the next session.

# ── D-Bus Profile objects ──────────────────────────────────────────────────────

class DummyProfile(dbus.service.Object):
    """HFP AG / HSP HS — hold fd open. Android Auto app requires HFP presence."""

    def __init__(self, bus, path):
        super().__init__(bus, path)

    @dbus.service.method(PROFILE_IFACE, in_signature="oha{sv}")
    def NewConnection(self, path, fd, properties):
        name = self.__dbus_object_path__.split("/")[-1]
        print(f"[aa-bt] {name}: connection from {path}")
        sock = socket.fromfd(int(fd), socket.AF_UNIX, socket.SOCK_STREAM)
        sock.setblocking(True)
        _open_sockets.append(sock)

    @dbus.service.method(PROFILE_IFACE, in_signature="o")
    def RequestDisconnection(self, path): pass

    @dbus.service.method(PROFILE_IFACE)
    def Release(self): pass


class HFPProfile(dbus.service.Object):
    """HFP Hands-Free (car kit) profile.

    The phone connects HFP to us when it sees a known car device.
    Establishing HFP SLC triggers Android Auto on the phone, which sets
    user intention and causes the next TCP session to be a REAL session
    (not a cleanup/credential-verification session).
    """

    def __init__(self, bus, path):
        super().__init__(bus, path)

    @dbus.service.method(PROFILE_IFACE, in_signature="oha{sv}")
    def NewConnection(self, path, fd, properties):
        raw_fd = fd.take() if hasattr(fd, 'take') else int(fd)
        own_fd = os.dup(raw_fd)
        if hasattr(fd, 'take'):
            os.close(raw_fd)
        print(f"[hfp] NewConnection from {path} fd={own_fd}", flush=True)
        t = threading.Thread(target=_handle_hfp_rfcomm, args=(own_fd, str(path)), daemon=True)
        t.start()

    @dbus.service.method(PROFILE_IFACE, in_signature="o")
    def RequestDisconnection(self, path):
        print(f"[hfp] disconnect {path}", flush=True)

    @dbus.service.method(PROFILE_IFACE)
    def Release(self):
        pass


class AAProfile(dbus.service.Object):
    """AA RFCOMM profile — runs the 5-stage WiFi handshake."""

    def __init__(self, bus, path):
        super().__init__(bus, path)

    @dbus.service.method(PROFILE_IFACE, in_signature="oha{sv}")
    def NewConnection(self, path, fd, properties):
        try:
            # take() transfers fd ownership from dbus to us so it won't be
            # closed when the UnixFd object is GC'd. If older dbus-python
            # doesn't have take(), fall back to int(fd).
            raw_fd = fd.take() if hasattr(fd, 'take') else int(fd)

            # Extract MAC from dbus device path: /org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF
            path_str = str(path)
            mac = path_str.split("/dev_")[-1].replace("_", ":") if "/dev_" in path_str else path_str
            print(f"[aa-bt] AA profile: RFCOMM connection from {mac} (fd={raw_fd})", flush=True)

            # Store device path and MAC for BT reconnect worker
            global _last_device_path, _last_phone_mac
            _last_device_path = path_str
            _last_phone_mac   = mac

            # Start HFP SLC probe immediately — runs in parallel with RFCOMM handshake.
            # The phone checks HFP SLC state ~200ms after RFCOMM connects, which is too
            # fast for us to establish SLC before that check. But starting early means
            # SLC is up ASAP, so any subsequent reconnect or session 2 sees it active.
            # The probe has an 8s cooldown so concurrent calls from the reconnect worker
            # won't cause duplicate probes.
            _trigger_hfp_slc_async(mac)

            # dup so the socket lifetime is fully under our control
            own_fd = os.dup(raw_fd)
            if hasattr(fd, 'take'):
                os.close(raw_fd)           # we own it now; close the original

            sock = socket.socket(fileno=own_fd)
            sock.setblocking(True)
            sock.settimeout(30.0)
            t = threading.Thread(target=_run_wifi_handshake, args=(sock, mac), daemon=True)
            t.start()
            print(f"[aa-bt] AA profile: handshake thread started", flush=True)
        except Exception as e:
            print(f"[aa-bt] AA profile: NewConnection error: {e}", flush=True)
            traceback.print_exc()

    @dbus.service.method(PROFILE_IFACE, in_signature="o")
    def RequestDisconnection(self, path):
        print("[aa-bt] AA profile: disconnect")

    @dbus.service.method(PROFILE_IFACE)
    def Release(self): pass


# ── Inline pairing agent (was a separate bt-agent.py process) ─────────────────
# Mirrors iap2/transport/bluetooth.py Agent class.

class PairingAgent(dbus.service.Object):

    @dbus.service.method(AGENT_IFACE, in_signature="", out_signature="")
    def Release(self): pass

    @dbus.service.method(AGENT_IFACE, in_signature="os", out_signature="")
    def AuthorizeService(self, device, uuid):
        print(f"[aa-bt] AuthorizeService {device} → accepted")

    @dbus.service.method(AGENT_IFACE, in_signature="o", out_signature="s")
    def RequestPinCode(self, device):
        return "0000"

    @dbus.service.method(AGENT_IFACE, in_signature="o", out_signature="u")
    def RequestPasskey(self, device):
        return dbus.UInt32(0)

    @dbus.service.method(AGENT_IFACE, in_signature="ouq", out_signature="")
    def DisplayPasskey(self, device, passkey, entered):
        print(f"[aa-bt] DisplayPasskey {passkey:06d}")

    @dbus.service.method(AGENT_IFACE, in_signature="os", out_signature="")
    def DisplayPinCode(self, device, pincode):
        print(f"[aa-bt] DisplayPinCode {pincode}")

    @dbus.service.method(AGENT_IFACE, in_signature="ou", out_signature="")
    def RequestConfirmation(self, device, passkey):
        # Mark Trusted so BlueZ accepts later incoming reconnects without
        # re-prompting. busctl-based to stay decoupled from the agent's bus.
        subprocess.run(
            ["busctl", "--system", "set-property",
             "org.bluez", str(device), "org.bluez.Device1",
             "Trusted", "b", "true"],
            capture_output=True, check=False, timeout=3,
        )
        print(f"[aa-bt] RequestConfirmation {passkey:06d} → confirmed+trusted")

    @dbus.service.method(AGENT_IFACE, in_signature="o", out_signature="")
    def RequestAuthorization(self, device):
        print(f"[aa-bt] RequestAuthorization → accepted")

    @dbus.service.method(AGENT_IFACE, in_signature="", out_signature="")
    def Cancel(self): pass




# ── Main ───────────────────────────────────────────────────────────────────────

def main():
    if os.geteuid() != 0:
        print("ERROR: must run as root: sudo python3 bt/aa-bluetooth.py")
        sys.exit(1)

    import ctypes, signal

    PR_SET_PDEATHSIG = 1
    try:
        libc = ctypes.CDLL("libc.so.6", use_errno=True)
        if libc.prctl(PR_SET_PDEATHSIG, signal.SIGTERM, 0, 0, 0) != 0:
            print(f"[aa-bt] prctl(PR_SET_PDEATHSIG) failed errno={ctypes.get_errno()}")
    except OSError as e:
        print(f"[aa-bt] could not load libc for prctl: {e}")

    # Forward-declare so the handler closes over the right object once we
    # construct the MainLoop further below.
    _loop_holder = {"loop": None}
    def _on_term(_signum, _frame):
        loop = _loop_holder["loop"]
        if loop is not None and loop.is_running():
            print("[aa-bt] received SIGTERM — quitting main loop")
            loop.quit()
        else:
            # No loop yet — just raise so any in-flight setup unwinds into
            # the outer try/finally that the caller wraps around main().
            raise KeyboardInterrupt()
    signal.signal(signal.SIGTERM, _on_term)
    signal.signal(signal.SIGHUP,  _on_term)  # also exits on parent SIGHUP

    # ── 0. D-Bus main loop — must be set before ANY dbus call ─────────────────
    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)

    # ── 1. WiFi AP ────────────────────────────────────────────────────────────
    ap_ready = setup_ap()
    setup_bluetoothd_dropin()
    _power_on_bt()

    import atexit
    _bluez_handles = {"profile_manager": None, "agent_manager": None, "agent": None}

    def _set_advertising(state: bool) -> None:
        """Flip Adapter1.Discoverable+Pairable via busctl. Best-effort."""
        flag = "true" if state else "false"
        for prop in ("Discoverable", "Pairable"):
            subprocess.run(
                ["busctl", "--system", "set-property",
                 "org.bluez", ADAPTER_PATH, "org.bluez.Adapter1",
                 prop, "b", flag],
                capture_output=True, check=False, timeout=3,
            )

    def _atexit_cleanup():
        print("[aa-bt] atexit cleanup running")
        _set_advertising(False)
        pm = _bluez_handles["profile_manager"]
        if pm is not None:
            for path in ("/livi/bt/aa", "/livi/bt/hfp"):
                try: pm.UnregisterProfile(path)
                except Exception: pass
        am = _bluez_handles["agent_manager"]
        ag = _bluez_handles["agent"]
        if am is not None and ag is not None:
            try: am.UnregisterAgent(ag)
            except Exception: pass
        teardown_ap()
    atexit.register(_atexit_cleanup)

    # ── 3. BlueZ ───────────────────────────────────────────────────────────────
    bus = dbus.SystemBus(private=True)
    bluez = bus.get_object(BLUEZ_SERVICE, BLUEZ_OBJ)

    # Register AA RFCOMM profile.
    profile_manager = dbus.Interface(bluez, PROFILE_MANAGER)
    _bluez_handles["profile_manager"] = profile_manager

    aa_obj = AAProfile(bus, "/livi/bt/aa")

    def _register_aa_profile():
        return profile_manager.RegisterProfile(aa_obj, AA_UUID, {
            'Name':                  'Android Auto Wireless',
            'Role':                  'server',
            'Channel':               dbus.types.UInt16(8),
            'RequireAuthentication': False,
            'RequireAuthorization':  False,
            'ServiceRecord':         AA_SDP_RECORD,
        })

    try:
        profile_manager.UnregisterProfile("/livi/bt/aa")
        print("[aa-bt] unregistered stale AA profile (previous run)")
    except Exception:
        pass  # not registered on our connection — that's fine

    try:
        _register_aa_profile()
    except dbus.exceptions.DBusException as e:
        if 'already registered' not in str(e).lower():
            raise
        print("[aa-bt] AA UUID held by stale bus — restarting bluetoothd to evict it")
        subprocess.run(["systemctl", "restart", "bluetooth"], check=False)

        rebound = False
        deadline = time.monotonic() + 15.0
        while time.monotonic() < deadline:
            time.sleep(0.25)
            try:
                bus = dbus.SystemBus(private=True)
                bluez = bus.get_object(BLUEZ_SERVICE, BLUEZ_OBJ)
                profile_manager = dbus.Interface(bluez, PROFILE_MANAGER)
                _bluez_handles["profile_manager"] = profile_manager
                # Smoke test — fails until BlueZ has fully come back.
                dbus.Interface(bluez, dbus.PROPERTIES_IFACE).GetAll("org.bluez.AgentManager1")
                rebound = True
                break
            except Exception:
                continue
        if not rebound:
            print("[aa-bt] bluetoothd did not come back within 15s — giving up this spawn")
            raise

        aa_obj = AAProfile(bus, "/livi/bt/aa")
        _register_aa_profile()
    print("[aa-bt] registered AA RFCOMM profile (ch=8)")

    # HFP Hands-Free profile — car kit role (Pi = HF, Phone = AG)
    try:
        profile_manager.UnregisterProfile("/livi/bt/hfp")
        print("[aa-bt] unregistered stale HFP profile (previous run)")
    except Exception:
        pass  # Not registered on our connection — that's fine

    global _hfp_profile_registered
    hfp_obj = HFPProfile(bus, "/livi/bt/hfp")
    try:
        profile_manager.RegisterProfile(hfp_obj, HFP_HF_UUID, {
            'Name':                  'HFP Hands-Free',
            'Role':                  'client',
            'RequireAuthentication': False,
            'RequireAuthorization':  False,
            'Features':              dbus.UInt16(0x009C),  # CLI+VR+Vol+CodecNeg
            'Version':               dbus.UInt16(0x0108),  # HFP 1.8
        })
        _hfp_profile_registered = True
        print("[aa-bt] registered HFP HF profile ✓")
    except dbus.exceptions.DBusException as e:
        msg = str(e).lower()
        if "already registered" in msg or "notpermitted" in msg:
            print("[aa-bt] HFP HF profile is reserved by BlueZ — using raw "
                  "RFCOMM direct-connect path (SLC unaffected)")
        else:
            print(f"[aa-bt] HFP profile registration unexpected error: {e}")

    agent = PairingAgent(bus, "/livi/bt/agent")
    agent_manager = dbus.Interface(bluez, AGENT_MANAGER)
    _bluez_handles["agent"] = agent
    _bluez_handles["agent_manager"] = agent_manager
    agent_manager.RegisterAgent(agent, "KeyboardDisplay")
    agent_manager.RequestDefaultAgent(agent)
    print("[aa-bt] pairing agent registered")

    adapter = dbus.Interface(
        bus.get_object(BLUEZ_SERVICE, ADAPTER_PATH),
        dbus.PROPERTIES_IFACE,
    )

    def _adapter_set(prop: str, value, attempts: int = 12, base_delay: float = 0.25):
        """Adapter1.Set wrapper with retry on transient Busy. ~5 s max wait."""
        delay = base_delay
        last: Exception | None = None
        for i in range(attempts):
            try:
                adapter.Set("org.bluez.Adapter1", prop, value)
                if i > 0:
                    print(f"[aa-bt] adapter Set({prop}) succeeded after {i} retries",
                          flush=True)
                return
            except dbus.exceptions.DBusException as e:
                last = e
                if "org.bluez.Error.Busy" not in str(e):
                    raise
                time.sleep(delay)
                delay = min(delay * 1.5, 0.6)
        # Out of attempts — re-raise the last Busy so the caller still sees it.
        assert last is not None
        raise last

    _adapter_set("Alias",               BTNAME)
    _adapter_set("DiscoverableTimeout", dbus.UInt32(0))
    _adapter_set("Powered",             True)
    # Only enable Discoverable/Pairable if the AP is fully serving
    if ap_ready:
        _adapter_set("Discoverable",    True)
        _adapter_set("Pairable",        True)
    else:
        print("[aa-bt] AP not ready — leaving BT non-discoverable/non-pairable")

    props = adapter.GetAll("org.bluez.Adapter1")
    print(f"[aa-bt] adapter: {props.get('Name','?')} ({props.get('Address','?')}) "
          f"discoverable={ap_ready}")

    # ── Start event server + BlueZ signal subscription ──────────────────────
    _start_event_server()
    _subscribe_device_signals(bus)

    # ── Start BT reconnect worker ─────────────────────────────────────────────
    t = threading.Thread(target=_bt_reconnect_worker, daemon=True)
    t.start()
    print("[aa-bt] BT reconnect worker started (5s interval)")

    print()
    print("=== LIVI Android Auto stack running ===")
    print(f"  WiFi : SSID={SSID!r}  IP={AP_IP}  port={AA_PORT}")
    print(f"  BT   : {BTNAME}  (discoverable)")
    print(f"  RFCOMM: held open after handshake")
    print()
    print("[aa-bt] Waiting for phone — TCP server is hosted in-process by LIVI")
    print()

    loop = GLib.MainLoop()
    _loop_holder["loop"] = loop          # signal handler wires loop.quit() through
    try:
        loop.run()
    except KeyboardInterrupt:
        print("\n[aa-bt] shutting down...")
    finally:
        print("[aa-bt] tearing down (hostapd, dnsmasq, BT profile)...")
        _set_advertising(False)
        for path in ("/livi/bt/aa", "/livi/bt/hfp"):
            try:
                profile_manager.UnregisterProfile(path)
            except Exception:
                pass
        try:
            agent_manager.UnregisterAgent(agent)
        except Exception:
            pass
        teardown_ap()


if __name__ == "__main__":
    main()
