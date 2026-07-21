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

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from shared.wifi_ap import AP_IP, deauth_all_clients, get_wlan_mac
from shared.config import BT_ADAPTER, BTNAME, CHANNEL, PASSPHRASE, SSID, WIFI_IFACE

AA_PORT = int(os.environ.get("LIVI_PORT", "5277"))

# ── Debug gate ────────────────────────────────────────────────────────────────
DEBUG = os.environ.get("DEBUG") == "1"


def dprint(*args, **kwargs) -> None:
    """print() that only fires when DEBUG=1."""
    if DEBUG:
        print(*args, **kwargs)




def _busctl_get_property(path: str, iface: str, prop: str,
                         timeout: float = 5.0) -> str:
    """Read a D-Bus property via `busctl get-property`. Returns "" on error."""
    try:
        result = subprocess.run(
            ["busctl", "--system", "get-property",
             "org.bluez", path, iface, prop],
            capture_output=True, text=True, timeout=timeout,
        )
        return result.stdout.strip() if result.returncode == 0 else ""
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        return ""




# Protobuf helpers ──────────────────────

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
    """WifiVersionRequest (msgId=4): WPP version 6.0 + AP frequency."""
    freq = _channel_to_freq_mhz(CHANNEL)
    freq_body = _encode_varint(freq)
    packed_channels = bytes([0x22]) + _encode_varint(len(freq_body)) + freq_body
    proto = _pb_varint(1, 6) + _pb_varint(2, 0) + packed_channels
    return struct.pack(">HH", len(proto), 4) + proto

def _build_wifi_start_request(ip: str, port: int) -> bytes:
    proto = _pb_string(1, ip) + _pb_varint(2, port)
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
    """WifiInfoResponse (msgId=3): SSID/key/BSSID + security/AP type."""
    proto = (
        _pb_string(1, ssid) +
        _pb_string(2, key)  +
        _pb_string(3, bssid) +
        _pb_varint(4, 8)    +   # security_mode = WPA2_PERSONAL
        _pb_varint(5, 0)        # access_point_type = STATIC
    )
    return struct.pack(">HH", len(proto), 3) + proto

# RFCOMM socket helpers ──────────────────────

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
    proto = _recv_exactly(sock, length) if length > 0 else b""
    return msg_id, proto

def _read_varint(buf: bytes, i: int) -> tuple[int, int]:
    val = 0
    shift = 0
    while True:
        b = buf[i]
        i += 1
        val |= (b & 0x7F) << shift
        if not b & 0x80:
            return val, i
        shift += 7

def _wpp_identity(data: bytes):
    """WifiVersionResponse -> (instanceId, serial).

    Field 3 (string) is the device serial, which is also the USB descriptor serial.
    Field 6 (message) -> field 1 (string) is the instanceId.
    """
    instance_id = ""
    serial = ""
    try:
        i, n = 0, len(data)
        while i < n:
            tag, i = _read_varint(data, i)
            field, wire = tag >> 3, tag & 7
            if wire == 2:
                ln, i = _read_varint(data, i)
                val = data[i:i + ln]
                i += ln
                if field == 3:
                    serial = val.decode("utf-8", "replace")
                elif field == 6:
                    j = 0
                    while j < len(val):
                        t2, j = _read_varint(val, j)
                        if t2 & 7 == 2:
                            l2, j = _read_varint(val, j)
                            v2 = val[j:j + l2]
                            j += l2
                            if t2 >> 3 == 1:
                                instance_id = v2.decode("utf-8", "replace")
                        elif t2 & 7 == 0:
                            _, j = _read_varint(val, j)
                        else:
                            break
            elif wire == 0:
                _, i = _read_varint(data, i)
            else:
                break
    except Exception:
        pass
    return instance_id, serial

def _emit_aa_device(mac: str, data: bytes) -> None:
    iid, serial = _wpp_identity(data)
    if iid or serial:
        dprint(f"[aa-bt] WPP: identified {mac.upper()} instanceId={iid or '-'} "
              f"serial={serial or '-'}", flush=True)
        _push_event({"event": "aa-device", "btMac": mac.upper(),
                     "instanceId": iid, "usbSerial": serial})


# Phones that already project over USB. They must never be invited onto the AP: a wired
# session outranks a wireless one.
_wired_phones: set = set()


def _set_wired_phones(arg: str) -> None:
    global _wired_phones
    try:
        ids = json.loads(arg) if arg.strip() else []
    except Exception:
        ids = []
    _wired_phones = {str(x).upper() for x in ids if x}
    dprint(f"[aa-bt] wired phones: {sorted(_wired_phones)}", flush=True)


def _is_wired(*ids) -> bool:
    return any(str(i).upper() in _wired_phones for i in ids if i)

# ── WiFi handshake ──────────────────────

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
                dprint(f"[aa-bt]   TCP server :{port} ready (LISTEN)", flush=True)
            return True
        if first:
            dprint(f"[aa-bt]   Waiting for TCP server :{port} to be LISTEN (up to {timeout_s:.0f}s)...", flush=True)
            first = False
        time.sleep(interval_s)
    dprint(f"[aa-bt]   TCP server not ready after {timeout_s:.0f}s — sending WifiStartRequest anyway", flush=True)
    return False




def _run_wifi_handshake(sock: socket.socket, mac: str):
    """
    WiFi credential exchange over RFCOMM.

    Full WPP sequence:
      0. HU → Phone  WifiVersionRequest   (msgId=4, version + freq)
      1. Phone → HU  WifiVersionResponse  (msgId=5, phone's version)
      2. HU → Phone  WifiStartRequest     (msgId=1, our IP + TCP port)
      3. Phone → HU  WifiInfoRequest      (msgId=2, empty — phone asks for AP creds)
      4. HU → Phone  WifiInfoResponse     (msgId=3, SSID/passphrase/BSSID)
      5. Phone → HU  WifiStartResponse    (msgId=7, ack)
      6. Phone → HU  WifiConnectionStatus (msgId=6, status=0 → joined AP)

    """
    try:
        bssid = get_wlan_mac(WIFI_IFACE)
        dprint(f"[aa-bt] WPP handshake → {mac} (AP {AP_IP}:{AA_PORT} SSID={SSID})",
              flush=True)

        _wait_for_tcp_server(AP_IP, AA_PORT, timeout_s=30.0)

        sock.sendall(_build_wifi_version_request())

        sock.settimeout(10.0)
        try:
            msg_id, data = _recv_frame(sock)
            if msg_id == 5:
                dprint(f"[aa-bt] WPP: WifiVersionResponse hex={data.hex()}",
                      flush=True)
                _emit_aa_device(mac, data)
                iid, serial = _wpp_identity(data)
                if _is_wired(mac, iid, serial):
                    dprint(f"[aa-bt] WPP: {mac} already projecting over USB — "
                          f"not offering the AP", flush=True)
                    return
                _pending = None
            else:
                _pending = (msg_id, data)
        except socket.timeout:
            dprint("[aa-bt] WPP: no WifiVersionResponse within 10s — continuing",
                  flush=True)
            _pending = None

        sock.sendall(_build_wifi_start_request(AP_IP, AA_PORT))

        sock.settimeout(30.0)
        wifi_status_seen = False
        while True:
            if _pending is not None:
                msg_id, data = _pending
                _pending = None
            else:
                try:
                    msg_id, data = _recv_frame(sock)
                except socket.timeout:
                    if wifi_status_seen:
                        sock.settimeout(60.0)
                        continue
                    dprint("[aa-bt] WPP: handshake timeout", flush=True)
                    break

            if msg_id == 2:      # WifiInfoRequest
                sock.sendall(_build_wifi_info_response(SSID, PASSPHRASE, bssid))

            elif msg_id == 6:    # WifiConnectionStatus
                status = data[1] if len(data) >= 2 else 0
                if status == 0:
                    dprint(f"[aa-bt] WPP: phone joined AP {SSID!r}", flush=True)
                else:
                    dprint(f"[aa-bt] WPP: phone-side AP join FAILED (status={status})",
                          flush=True)
                wifi_status_seen = True

            elif msg_id == 8:    # WPP ping (modern path) — echo as msgId=9
                pong = struct.pack(">HH", len(data), 9) + data
                sock.sendall(pong)

            elif msg_id == 5:    # late WifiVersionResponse
                dprint(f"[aa-bt] WPP: WifiVersionResponse hex={data.hex()}",
                      flush=True)
                _emit_aa_device(mac, data)

            elif msg_id != 7:
                dprint(f"[aa-bt] WPP: unknown msgId={msg_id} len={len(data)} "
                      f"hex={data.hex()}", flush=True)

    except ConnectionError as e:
        dprint(f"[aa-bt] RFCOMM disconnected during handshake: {e}", flush=True)
    except Exception as e:
        dprint(f"[aa-bt] RFCOMM handshake error: {e}", flush=True)
        traceback.print_exc()
    finally:
        try:
            sock.close()
        except Exception:
            pass

# ── HFP Hands-Free (car kit role) ─────────────────────────────────────────────
#
# HFP SLC sequence (HFP 1.8):
#   HF → AG : AT+BRSF=<HF features>
#   AG → HF : +BRSF:<AG features>  then  OK
#   (if both codec neg) HF → AG : AT+BAC=1,2   then OK
#   HF → AG : AT+CIND=?           (indicator mapping)
#   AG → HF : +CIND:<map>         then OK
#   HF → AG : AT+CIND?            (current indicator values)
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
            dprint(f"[hfp] SLC continuation from probe ({len(initial_data)} bytes in buffer)", flush=True)
        else:
            # HF must speak first: send AT+BRSF
            _send(f"AT+BRSF={_HFP_HF_FEATURES}\r")
            dprint(f"[hfp] >> AT+BRSF={_HFP_HF_FEATURES}", flush=True)

        buf = initial_data
        while True:
            # Only block on read if there is no complete line already in buf
            if b"\r" not in buf:
                ready, _, _ = select.select([fd], [], [], 300.0)
                if not ready:
                    dprint("[hfp] AT data timeout (300s) — disconnecting", flush=True)
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
                dprint(f"[hfp] << {line}", flush=True)

                if line.startswith("+BRSF:"):
                    try:
                        ag_features = int(line.split(":")[1].strip())
                    except ValueError:
                        pass
                    dprint(f"[hfp]    AG features: {ag_features}", flush=True)

                elif line == "OK":
                    if not hfp_slc_established:
                        _both_codec_neg = (
                            bool(_HFP_HF_FEATURES & (1 << 7)) and
                            bool(ag_features & (1 << 9))
                        )
                        if ag_features > 0 and _both_codec_neg and not _sent_bac:
                            _sent_bac = True
                            _send("AT+BAC=1,2\r")
                            dprint("[hfp] >> AT+BAC=1,2 (CVSD, mSBC)", flush=True)
                        elif ag_features > 0 and not _sent_cind_test:
                            _sent_cind_test = True
                            _send("AT+CIND=?\r")
                            dprint("[hfp] >> AT+CIND=?", flush=True)
                        elif _sent_cind_test and not _sent_cind_read:
                            _sent_cind_read = True
                            _send("AT+CIND?\r")
                            dprint("[hfp] >> AT+CIND?", flush=True)
                        elif _sent_cind_read and not _sent_cmer:
                            _sent_cmer = True
                            _send("AT+CMER=3,0,0,1\r")
                            dprint("[hfp] >> AT+CMER=3,0,0,1", flush=True)
                        elif _sent_cmer:
                            hfp_slc_established = True
                            dprint("[hfp] ✓ SLC established — Android Auto should trigger", flush=True)

                elif line.startswith("+CIND:"):
                    dprint(f"[hfp]    indicators: {line}", flush=True)

                elif line == "ERROR":
                    dprint("[hfp] AG returned ERROR", flush=True)

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
                    dprint(f"[hfp]    codecs: {line.split('=')[1]}", flush=True)
                    _send_ok()

                elif line.startswith("+BCS:"):
                    codec = line.split(":")[1].strip()
                    dprint(f"[hfp]    codec selected: {codec}", flush=True)
                    _send(f"AT+BCS={codec}\r")

                elif line == "ATA":
                    _send_ok()
                elif line == "AT+CHUP":
                    _send_ok()
                elif line.startswith("ATD"):
                    dprint(f"[hfp]    dial: {line[3:].rstrip(';')}", flush=True)
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

                elif line.startswith("+CIEV:") or line == "RING" or line.startswith("+CLIP:"):
                    dprint(f"[hfp]    unsolicited: {line}", flush=True)

                else:
                    dprint(f"[hfp]    unknown AT: {line!r} — sending OK", flush=True)
                    _send_ok()

    except Exception as e:
        dprint(f"[hfp] error: {e}", flush=True)
    finally:
        try:
            os.close(fd)
        except OSError:
            pass
        hfp_slc_established = False
        dprint("[hfp] disconnected", flush=True)


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
HSP_HS_UUID = "00001108-0000-1000-8000-00805f9b34fb"  # HSP Headset (we are HS)
HSP_AG_UUID = "00001112-0000-1000-8000-00805f9b34fb"  # HSP Audio Gateway (phone is AG)

# AA SDP record — passed inline as ServiceRecord on RegisterProfile so BlueZ
# publishes it directly via its D-Bus-driven SDP server (no /var/run/sdp,
# Contents:
#   0x0001 ServiceClassIDList    — only the 128-bit AA UUID
#   0x0004 ProtocolDescriptorList — L2CAP + RFCOMM channel 8
#   0x0005 BrowseGroupList        — PublicBrowseRoot
#   0x0100 ServiceName            — "Android Auto Wireless"

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

_open_sockets: list[int] = []  # raw fds for HFP/HSP, kept alive to hold the BR/EDR ACL

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

def _bt_is_connected(mac: str) -> bool:
    """Return True if the phone's BT is currently connected to us."""
    path = f"{ADAPTER_PATH}/dev_{mac.replace(':', '_')}"
    out = _busctl_get_property(path, "org.bluez.Device1", "Connected")
    # busctl prints booleans as "b true" / "b false"
    return out.lower().endswith("true")


def _find_hfp_ag_channel(mac: str) -> int:
    """Return a best-guess HFP AG RFCOMM channel for the phone."""
    return 1

def _connect_hfp_direct(mac: str) -> None:
    """Connect to phone's HFP AG via raw AF_BLUETOOTH/BTPROTO_RFCOMM socket."""
    global _hfp_raw_last_at

    with _hfp_raw_lock:
        now = time.monotonic()
        elapsed = now - _hfp_raw_last_at
        if elapsed < _HFP_RAW_COOLDOWN_SEC:
            dprint(f"[hfp] direct-connect cooldown ({elapsed:.1f}s < {_HFP_RAW_COOLDOWN_SEC:.0f}s)", flush=True)
            return
        _hfp_raw_last_at = now

    def _do() -> None:
        cached = _hfp_cached_channel.get(mac)
        if cached:
            candidates = [cached] + [c for c in range(1, 16) if c != cached]
            dprint(f"[hfp] probing channels for HFP AG on {mac} (cached ch={cached})", flush=True)
        else:
            first = _find_hfp_ag_channel(mac)
            candidates = [first] + [c for c in range(1, 16) if c != first]
            dprint(f"[hfp] probing {len(candidates)} channels for HFP AG on {mac} "
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

                # Quick probe: send AT+BRSF, wait up to 3s for +BRSF: response.
                probe = f"AT+BRSF={_HFP_HF_FEATURES}\r".encode()
                os.write(fd, probe)
                dprint(f"[hfp] ch={ch} connected — >> AT+BRSF={_HFP_HF_FEATURES} (probe)", flush=True)

                ready, _, _ = select.select([fd], [], [], 3.0)
                if not ready:
                    dprint(f"[hfp] ch={ch} no response to AT+BRSF — not HFP AG, skipping", flush=True)
                    os.close(fd)
                    fd = -1
                    continue

                initial_data = os.read(fd, 1024)
                if not initial_data:
                    dprint(f"[hfp] ch={ch} connection closed — skipping", flush=True)
                    try: os.close(fd)
                    except OSError: pass
                    fd = -1
                    continue

                dprint(f"[hfp] ch={ch} << {initial_data!r}", flush=True)
                if b"+BRSF" not in initial_data:
                    dprint(f"[hfp] ch={ch} no +BRSF in response — not HFP AG, skipping", flush=True)
                    os.close(fd)
                    fd = -1
                    continue

                # Found HFP AG — cache channel and launch full SLC thread
                _hfp_cached_channel[mac] = ch
                dprint(f"[hfp] ch={ch} HFP AG confirmed ✓ — starting SLC (cached)", flush=True)
                threading.Thread(target=_handle_hfp_rfcomm, args=(fd, mac, initial_data),
                                 daemon=True).start()
                return  # success

            except OSError as e:
                if e.errno == 111:  # ECONNREFUSED
                    dprint(f"[hfp] ch={ch} refused (not registered)", flush=True)
                elif e.errno == 16 and ch == cached:
                    dprint(f"[hfp] ch={ch} busy (SLC already active) — skipping probe", flush=True)
                    return
                else:
                    dprint(f"[hfp] ch={ch} error: {e}", flush=True)
                if fd >= 0:
                    try: os.close(fd)
                    except OSError: pass
                elif sock is not None:
                    try: sock.close()
                    except Exception: pass

        dprint(f"[hfp] all channels 1-15 exhausted for {mac} — HFP AG not found", flush=True)

    threading.Thread(target=_do, daemon=True).start()


# ── Device management helpers (BlueZ enumeration + actions) ───────────────────

def _device_path(mac: str) -> str:
    """Compose the BlueZ D-Bus device object path from a MAC address."""
    return f"{ADAPTER_PATH}/dev_{mac.replace(':', '_').upper()}"


def _busctl_parse_value(s: str) -> str | bool | int:
    """Parse a single-line busctl get-property output into a Python value."""
    s = s.strip()
    if not s:
        return ""
    sig, _, rest = s.partition(" ")
    rest = rest.strip()
    if sig == "s" or sig == "o":
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
    """Enumerate all paired BlueZ devices via busctl."""
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
    """Invoke a BlueZ method via busctl. Returns (ok, error_message)."""
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
    """Wake the phone via ConnectProfile(HSP_AG_UUID), with async retry."""
    target = mac.upper()
    max_attempts = 5
    interval_s = 3.0

    def _attempt() -> tuple[bool, str]:
        return _device_call(
            _device_path(mac), "org.bluez.Device1", "ConnectProfile",
            "s", HSP_AG_UUID,
            timeout=10.0,
        )

    dprint(f"[aa-bt] wake-up → {mac} (HSP_AG, up to {max_attempts} attempts)",
          flush=True)
    ok, err = _attempt()

    def _retry_loop() -> None:
        for attempt in range(2, max_attempts + 1):
            time.sleep(interval_s)
            if _last_phone_mac.upper() == target:
                dprint(f"[aa-bt] wake-up: AA RFCOMM up after {attempt - 1} retr"
                      f"{'y' if attempt - 1 == 1 else 'ies'}", flush=True)
                return
            _attempt()
        dprint(f"[aa-bt] wake-up: gave up after {max_attempts} attempts", flush=True)

    threading.Thread(target=_retry_loop, daemon=True,
                     name=f"wake-up-{mac.split(':')[-1]}").start()
    return ok, err


def _device_connect_full(mac: str) -> tuple[bool, str]:
    """Connect all auto-connect profiles (A2DP + HFP + HSP). Used for audio devices."""
    dprint(f"[aa-bt] connect-full → {mac} (Device1.Connect, all profiles)",
          flush=True)
    return _device_call(
        _device_path(mac), "org.bluez.Device1", "Connect",
        timeout=20.0,
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
      connect <mac>        → {"ok": true} or {"ok": false, "error": "..."}
      connect-full <mac>   → {"ok": true} or {"ok": false, "error": "..."}
      disconnect <mac>     → {"ok": true} or {"ok": false, "error": "..."}
      remove <mac>            → {"ok": true} or {"ok": false, "error": "..."}
      deauth-ap               → {"ok": true, "count": N}  (kicks Wi-Fi clients)
    """
    try:
        os.unlink(_AA_EVENT_SOCK)
    except OSError:
        pass

    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(_AA_EVENT_SOCK)
    srv.listen(8)
    os.chmod(_AA_EVENT_SOCK, 0o666)  # allow non-root TypeScript process to connect
    dprint(f"[aa-bt] event server listening on {_AA_EVENT_SOCK}", flush=True)

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
            if cmd == "connect-full":
                if not arg:
                    return {"ok": False, "error": "connect-full requires a MAC argument"}
                ok, err = _device_connect_full(arg)
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
            if cmd == "wired-phones":
                _set_wired_phones(arg)
                return {"ok": True}
            if cmd == "deauth-ap":
                count = deauth_all_clients()
                dprint(f"[aa-bt] deauth-ap: kicked {count} client(s)", flush=True)
                return {"ok": True, "count": count}
            return {"ok": False, "error": f"unknown command: {cmd!r}"}
        except Exception as e:
            traceback.print_exc()
            return {"ok": False, "error": f"{type(e).__name__}: {e}"}

    def _accept_loop() -> None:
        while True:
            try:
                conn, _ = srv.accept()
            except OSError:
                break

            def _handle(c: socket.socket) -> None:
                keep_open = False
                try:
                    raw = _read_request(c)
                    if not raw:
                        return
                    dprint(f"[aa-bt] sock req: {raw!r}", flush=True)

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
                    dprint(f"[aa-bt] sock handler error: {e}", flush=True)
                    try: _send_json(c, {"ok": False, "error": str(e)})
                    except Exception: pass
                finally:
                    if not keep_open:
                        try: c.close()
                        except Exception: pass

            threading.Thread(target=_handle, args=(conn,), daemon=True).start()

    threading.Thread(target=_accept_loop, daemon=True, name="event-server").start()


_transport_volume: dict[str, int] = {}


def _subscribe_device_signals(bus: dbus.Bus) -> None:
    """Push a JSON event to subscribers when any Device1 property changes."""
    def _on_props_changed(interface, changed, invalidated, path=""):
        if interface == "org.bluez.MediaTransport1" and "Volume" in changed:
            new_v = int(changed["Volume"])
            prev = _transport_volume.get(str(path))
            _transport_volume[str(path)] = new_v
            if prev is not None and new_v != prev:
                _push_event({
                    "event": "input",
                    "command": "volumeUp" if new_v > prev else "volumeDown"
                })
            return
        if interface != "org.bluez.Device1":
            return
        if "Connected" not in changed and "Paired" not in changed:
            return
        mac = ""
        if "/dev_" in path:
            mac = path.rsplit("/dev_", 1)[-1].replace("_", ":").upper()
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
    """Trigger HFP SLC via direct raw RFCOMM (bypasses BlueZ Profile API)."""
    _connect_hfp_direct(mac)


# ── D-Bus Profile objects ──────────────────────────────────────────────────────

class DummyProfile(dbus.service.Object):
    """HSP HS placeholder — accepts incoming RFCOMM and holds the fd open
    so the BR/EDR ACL doesn't tear down."""

    def __init__(self, bus, path):
        super().__init__(bus, path)

    @dbus.service.method(PROFILE_IFACE, in_signature="oha{sv}")
    def NewConnection(self, path, fd, properties):
        raw_fd = fd.take() if hasattr(fd, 'take') else int(fd)
        own_fd = os.dup(raw_fd)
        if hasattr(fd, 'take'):
            os.close(raw_fd)
        name = self.__dbus_object_path__.split("/")[-1]
        dprint(f"[aa-bt] {name}: connection from {path} (fd={own_fd})", flush=True)
        _open_sockets.append(own_fd)

    @dbus.service.method(PROFILE_IFACE, in_signature="o")
    def RequestDisconnection(self, path): pass

    @dbus.service.method(PROFILE_IFACE)
    def Release(self): pass


class HFPProfile(dbus.service.Object):
    """HFP Hands-Free (car kit) profile."""

    def __init__(self, bus, path):
        super().__init__(bus, path)

    @dbus.service.method(PROFILE_IFACE, in_signature="oha{sv}")
    def NewConnection(self, path, fd, properties):
        raw_fd = fd.take() if hasattr(fd, 'take') else int(fd)
        own_fd = os.dup(raw_fd)
        if hasattr(fd, 'take'):
            os.close(raw_fd)
        dprint(f"[hfp] NewConnection from {path} fd={own_fd}", flush=True)
        t = threading.Thread(target=_handle_hfp_rfcomm, args=(own_fd, str(path)), daemon=True)
        t.start()

    @dbus.service.method(PROFILE_IFACE, in_signature="o")
    def RequestDisconnection(self, path):
        dprint(f"[hfp] disconnect {path}", flush=True)

    @dbus.service.method(PROFILE_IFACE)
    def Release(self):
        pass


class BLEAd(dbus.service.Object):
    """BLE peripheral advertisement carrying the AA wireless UUID."""

    LE_AD_IFACE = "org.bluez.LEAdvertisement1"

    @dbus.service.method("org.freedesktop.DBus.Properties",
                         in_signature="ss", out_signature="v")
    def Get(self, iface, prop):
        return self.GetAll(iface)[prop]

    @dbus.service.method("org.freedesktop.DBus.Properties",
                         in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        return {
            "Type":         dbus.String("peripheral"),
            "ServiceUUIDs": dbus.Array([AA_UUID], signature="s"),
            "LocalName":    dbus.String(BTNAME),
        }

    @dbus.service.method(LE_AD_IFACE, in_signature="", out_signature="")
    def Release(self):
        pass


# ── BlueZ media player (AVRCP target) ────────────────────────────────────────

MEDIA_IFACE        = "org.bluez.Media1"
MPRIS_ROOT_IFACE   = "org.mpris.MediaPlayer2"
MEDIA_PLAYER_IFACE = "org.mpris.MediaPlayer2.Player"
LIVI_PLAYER_PATH   = "/livi/bt/player"

# AVC passthrough opcodes that arrive via Press()/Hold().
_AVC_KEY_MAP = {
    0x40: "powerToggle",
    0x41: "volumeUp",
    0x42: "volumeDown",
    0x43: "mute",
    0x44: "play",
    0x45: "stop",
    0x46: "pause",
    0x48: "rewind",
    0x49: "fastForward",
    0x4B: "next",
    0x4C: "previous",
    0x4E: "stop",
}


class LiviMediaPlayer(dbus.service.Object):
    def _emit(self, command: str) -> None:
        _push_event({"event": "input", "command": command})

    # ── org.mpris.MediaPlayer2 (root interface required by MPRIS 2.2) ──
    @dbus.service.method(MPRIS_ROOT_IFACE)
    def Raise(self): pass

    @dbus.service.method(MPRIS_ROOT_IFACE)
    def Quit(self): pass

    # ── org.mpris.MediaPlayer2.Player ──
    @dbus.service.method(MEDIA_PLAYER_IFACE)
    def Play(self):         self._emit("play")

    @dbus.service.method(MEDIA_PLAYER_IFACE)
    def Pause(self):        self._emit("pause")

    @dbus.service.method(MEDIA_PLAYER_IFACE)
    def PlayPause(self):    self._emit("playPause")

    @dbus.service.method(MEDIA_PLAYER_IFACE)
    def Stop(self):         self._emit("stop")

    @dbus.service.method(MEDIA_PLAYER_IFACE)
    def Next(self):         self._emit("next")

    @dbus.service.method(MEDIA_PLAYER_IFACE)
    def Previous(self):     self._emit("previous")

    @dbus.service.method(MEDIA_PLAYER_IFACE, in_signature="x")
    def Seek(self, offset):
        self._emit("fastForward" if int(offset) > 0 else "rewind")

    @dbus.service.method(MEDIA_PLAYER_IFACE, in_signature="ox")
    def SetPosition(self, track_id, position): pass

    @dbus.service.method(MEDIA_PLAYER_IFACE, in_signature="s")
    def OpenUri(self, uri): pass

    @dbus.service.method(dbus.PROPERTIES_IFACE, in_signature="ss", out_signature="v")
    def Get(self, interface, prop):
        if interface == MPRIS_ROOT_IFACE:
            return _MPRIS_ROOT_PROPS.get(prop, "")
        if interface == MEDIA_PLAYER_IFACE:
            return _MPRIS_PLAYER_PROPS.get(prop, "")
        return ""

    @dbus.service.method(dbus.PROPERTIES_IFACE, in_signature="s", out_signature="a{sv}")
    def GetAll(self, interface):
        if interface == MPRIS_ROOT_IFACE:
            return _MPRIS_ROOT_PROPS
        if interface == MEDIA_PLAYER_IFACE:
            return _MPRIS_PLAYER_PROPS
        return {}

    @dbus.service.method(dbus.PROPERTIES_IFACE, in_signature="ssv")
    def Set(self, interface, prop, value): pass

    @dbus.service.signal(dbus.PROPERTIES_IFACE, signature="sa{sv}as")
    def PropertiesChanged(self, interface, changed, invalidated): pass


_MPRIS_ROOT_PROPS = {
    "CanQuit":             False,
    "CanRaise":            False,
    "HasTrackList":        False,
    "Identity":            "LIVI",
    "SupportedUriSchemes": dbus.Array([], signature="s"),
    "SupportedMimeTypes":  dbus.Array([], signature="s"),
}

_MPRIS_PLAYER_PROPS = {
    "PlaybackStatus": "Playing",
    "LoopStatus":     "None",
    "Rate":           1.0,
    "Shuffle":        False,
    "Metadata":       dbus.Dictionary({}, signature="sv"),
    "Volume":         1.0,
    "Position":       dbus.Int64(0),
    "MinimumRate":    1.0,
    "MaximumRate":    1.0,
    "CanGoNext":      True,
    "CanGoPrevious":  True,
    "CanPlay":        True,
    "CanPause":       True,
    "CanSeek":        False,
    "CanControl":     True,
}


class AAProfile(dbus.service.Object):
    """AA RFCOMM profile — runs the 5-stage WiFi handshake."""

    def __init__(self, bus, path):
        super().__init__(bus, path)

    @dbus.service.method(PROFILE_IFACE, in_signature="oha{sv}")
    def NewConnection(self, path, fd, properties):
        try:
            # take() transfers fd ownership from dbus to us
            raw_fd = fd.take() if hasattr(fd, 'take') else int(fd)

            # Extract MAC from dbus device path: /org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF
            path_str = str(path)
            mac = path_str.split("/dev_")[-1].replace("_", ":") if "/dev_" in path_str else path_str
            dprint(f"[aa-bt] AA profile: RFCOMM connection from {mac} (fd={raw_fd})", flush=True)

            # Store device path and MAC for BT reconnect worker
            global _last_device_path, _last_phone_mac
            _last_device_path = path_str
            _last_phone_mac   = mac

            # Start HFP SLC probe immediately — runs in parallel with RFCOMM handshake.
            _trigger_hfp_slc_async(mac)

            # dup so the socket lifetime is fully under our control
            own_fd = os.dup(raw_fd)
            if hasattr(fd, 'take'):
                os.close(raw_fd)

            sock = socket.socket(fileno=own_fd)
            sock.setblocking(True)
            sock.settimeout(30.0)
            t = threading.Thread(target=_run_wifi_handshake, args=(sock, mac), daemon=True)
            t.start()
            dprint(f"[aa-bt] AA profile: handshake thread started", flush=True)
        except Exception as e:
            dprint(f"[aa-bt] AA profile: NewConnection error: {e}", flush=True)
            traceback.print_exc()

    @dbus.service.method(PROFILE_IFACE, in_signature="o")
    def RequestDisconnection(self, path):
        dprint("[aa-bt] AA profile: disconnect")

    @dbus.service.method(PROFILE_IFACE)
    def Release(self): pass


# ── Inline pairing agent ─────────────────





# ── Main ───────────────────────────────────────────────────────────────────────





class AaHandler:
    def __init__(self, ctx):
        self.ctx = ctx
        self._profile_manager = None
        self._ad_manager = None
        self._media_iface = None
        self._objs = {}

    def register(self):
        bus = self.ctx.bus
        profile_manager = dbus.Interface(
            bus.get_object(BLUEZ_SERVICE, BLUEZ_OBJ), PROFILE_MANAGER)
        self._profile_manager = profile_manager

        aa_obj = AAProfile(bus, "/livi/bt/aa")
        self._objs["aa"] = aa_obj
        try:
            profile_manager.UnregisterProfile("/livi/bt/aa")
        except Exception:
            pass
        try:
            profile_manager.RegisterProfile(aa_obj, AA_UUID, {
                "Name": "Android Auto Wireless",
                "Role": "server",
                "Channel": dbus.types.UInt16(8),
                "RequireAuthentication": False,
                "RequireAuthorization": False,
                "ServiceRecord": AA_SDP_RECORD,
            })
            dprint("[aa] registered AA RFCOMM profile (ch=8)")
        except dbus.exceptions.DBusException as e:
            dprint(f"[aa] AA profile registration failed: {e}")

        ble_ad_obj = BLEAd(bus, "/livi/bt/ble")
        self._objs["ble"] = ble_ad_obj
        try:
            objs = dbus.Interface(
                bus.get_object(BLUEZ_SERVICE, "/"),
                "org.freedesktop.DBus.ObjectManager").GetManagedObjects()
            ad_manager_path = next(
                (p for p, ifaces in objs.items()
                 if "org.bluez.LEAdvertisingManager1" in ifaces), None)
            if ad_manager_path:
                ad_manager = dbus.Interface(
                    bus.get_object(BLUEZ_SERVICE, ad_manager_path),
                    "org.bluez.LEAdvertisingManager1")
                try:
                    ad_manager.UnregisterAdvertisement("/livi/bt/ble")
                except dbus.exceptions.DBusException:
                    pass
                ad_manager.RegisterAdvertisement(
                    "/livi/bt/ble", {},
                    reply_handler=lambda: dprint("[aa] BLE advertisement registered", flush=True),
                    error_handler=lambda e: dprint(f"[aa] BLE advertisement error: {e}", flush=True))
                self._ad_manager = ad_manager
        except Exception as e:
            dprint(f"[aa] BLE advertisement setup failed: {e}")

        try:
            profile_manager.UnregisterProfile("/livi/bt/hfp")
        except Exception:
            pass
        hfp_obj = HFPProfile(bus, "/livi/bt/hfp")
        self._objs["hfp"] = hfp_obj
        try:
            profile_manager.RegisterProfile(hfp_obj, HFP_HF_UUID, {
                "Name": "HFP Hands-Free",
                "Role": "client",
                "RequireAuthentication": False,
                "RequireAuthorization": False,
                "Features": dbus.UInt16(0x009C),
                "Version": dbus.UInt16(0x0108),
            })
            dprint("[aa] registered HFP HF profile")
        except dbus.exceptions.DBusException as e:
            dprint(f"[aa] HFP profile: {e}")

        try:
            profile_manager.UnregisterProfile("/livi/bt/hsp_hs")
        except Exception:
            pass
        try:
            hsp_obj = DummyProfile(bus, "/livi/bt/hsp_hs")
            self._objs["hsp"] = hsp_obj
            profile_manager.RegisterProfile(hsp_obj, HSP_HS_UUID, {
                "Name": "HSP HS",
                "Role": "client",
                "RequireAuthentication": False,
                "RequireAuthorization": False,
            })
            dprint("[aa] registered HSP HS profile")
        except dbus.exceptions.DBusException as e:
            dprint(f"[aa] HSP profile: {e}")

        _start_event_server()
        _subscribe_device_signals(bus)

        try:
            self._objs["player"] = LiviMediaPlayer(bus, LIVI_PLAYER_PATH)
            media_iface = dbus.Interface(
                bus.get_object(BLUEZ_SERVICE, ADAPTER_PATH), MEDIA_IFACE)
            media_iface.RegisterPlayer(LIVI_PLAYER_PATH, _MPRIS_PLAYER_PROPS)
            self._media_iface = media_iface
            dprint(f"[aa] media player registered at {LIVI_PLAYER_PATH}")
        except Exception as e:
            dprint(f"[aa] media player setup failed: {e}")

    def handle_command(self, cmd, arg):
        return None

    def teardown(self):
        if self._media_iface is not None:
            try:
                self._media_iface.UnregisterPlayer(LIVI_PLAYER_PATH)
            except Exception:
                pass
        if self._ad_manager is not None:
            try:
                self._ad_manager.UnregisterAdvertisement("/livi/bt/ble")
            except Exception:
                pass
        if self._profile_manager is not None:
            for path in ("/livi/bt/aa", "/livi/bt/hfp", "/livi/bt/hsp_hs"):
                try:
                    self._profile_manager.UnregisterProfile(path)
                except Exception:
                    pass


def create(ctx):
    return AaHandler(ctx)
