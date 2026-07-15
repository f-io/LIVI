"""wifi_presence.py — watch hostapd for station join/leave and resolve the IP.

Attaches to the hostapd control interface and receives unsolicited
AP-STA-CONNECTED / AP-STA-DISCONNECTED events the instant a phone joins or
leaves the AP, each carrying the station's WiFi MAC. AP-STA-CONNECTED fires at
association, before DHCP, so the join IP is resolved with a short retry and
cached; the disconnect reuses the cached IP (the neigh entry may already be
gone). LIVI correlates the IP against the active session's peer, so only the
streaming device can end the session.
"""

import os
import socket
import subprocess
import threading
import time

HOSTAPD_CTRL_DIR = "/var/run/hostapd"
DEFAULT_LEASE = "/var/lib/misc/dnsmasq.leases"


def _read_leases(leasefile):
    """MAC (lowercase) -> IP from a dnsmasq lease file: '<expiry> <mac> <ip> ...'."""
    out = {}
    try:
        with open(leasefile) as f:
            for line in f:
                parts = line.split()
                if len(parts) >= 3:
                    out[parts[1].lower()] = parts[2]
    except OSError:
        pass
    return out


def resolve_ip(mac, leasefile):
    """Best-effort MAC -> IPv4 via the dnsmasq lease (our path + default) then the
    kernel neighbour table. Empty string if nothing is known yet."""
    mac = mac.lower()
    for lf in (leasefile, DEFAULT_LEASE):
        ip = _read_leases(lf).get(mac, "")
        if ip:
            return ip
    try:
        neigh = subprocess.check_output(["ip", "-4", "neigh", "show"], text=True, timeout=2)
        for line in neigh.splitlines():
            parts = line.split()
            if "lladdr" in parts:
                j = parts.index("lladdr")
                if j + 1 < len(parts) and parts[j + 1].lower() == mac and parts[0]:
                    return parts[0]
    except Exception:
        pass
    return ""


def start(iface, leasefile, on_event, log):
    """Run a daemon thread that calls on_event('joined'|'left', mac, ip) on AP
    station changes. Reconnects if hostapd isn't up yet or the socket drops."""
    ctrl_path = os.path.join(HOSTAPD_CTRL_DIR, iface)
    local_path = "/tmp/livi-wifi-presence-%d.sock" % os.getpid()
    sta_ip = {}

    def _emit_join(mac):
        ip = ""
        for _ in range(12):  # DHCP lands a beat after association
            ip = resolve_ip(mac, leasefile)
            if ip:
                break
            time.sleep(0.5)
        sta_ip[mac] = ip
        on_event("joined", mac, ip)

    def _handle_line(line):
        body = line.split(">", 1)[-1] if ">" in line else line
        if not (body.startswith("AP-STA-CONNECTED") or body.startswith("AP-STA-DISCONNECTED")):
            return
        parts = body.split()
        if len(parts) < 2:
            return
        mac = parts[1].lower()
        if "DISCONNECTED" in parts[0]:
            ip = sta_ip.pop(mac, "") or resolve_ip(mac, leasefile)
            try:
                on_event("left", mac, ip)
            except Exception as e:
                log("[wifi] on_event failed:", repr(e))
        else:
            threading.Thread(target=_emit_join, args=(mac,), daemon=True).start()

    def _run():
        while True:
            sock = None
            try:
                if not os.path.exists(ctrl_path):
                    time.sleep(2)
                    continue
                sock = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
                try:
                    os.unlink(local_path)
                except OSError:
                    pass
                sock.bind(local_path)
                sock.connect(ctrl_path)
                sock.settimeout(5)
                sock.send(b"ATTACH")
                try:
                    sock.recv(256)  # drain the ATTACH ack
                except socket.timeout:
                    pass
                sock.settimeout(None)
                log("[wifi] hostapd events attached")
                while True:
                    data = sock.recv(2048).decode("utf-8", "replace")
                    if not data:
                        raise OSError("hostapd control socket closed")
                    for line in data.splitlines():
                        _handle_line(line)
            except Exception as e:
                log("[wifi] hostapd attach lost, retrying:", repr(e))
                time.sleep(3)
            finally:
                if sock is not None:
                    try:
                        sock.close()
                    except Exception:
                        pass
                try:
                    os.unlink(local_path)
                except OSError:
                    pass

    threading.Thread(target=_run, daemon=True, name="wifi-presence").start()
