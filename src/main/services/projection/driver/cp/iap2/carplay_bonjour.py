import socket
import threading
import time

import dbus
import avahi
from gi.repository import GLib

from shared.config import AIRPLAY_PORT, PK, PI

SERVICE_NAME = "LIVI"

# /ctrl-int/1/connect probe state: the phone often isn't ready to accept the probe
# the instant it joins Wi-Fi, so we retry quickly in a background thread instead of
# waiting for Avahi to re-announce (which only happens ~a minute later).
_connect_lock = threading.Lock()
_connect_states = {}
_addr_running = set()
_rebrowse_fn = None


def _cstate(ckey):
    with _connect_lock:
        return _connect_states.setdefault(ckey, {"done": False})


def reset():
    """Clear the /ctrl-int/1/connect probe latch so the next discovery re-probes.
    Called when a phone connects over BT, before it rejoins Wi-Fi. The actual
    re-discovery on reconnect is driven by kick() on the Wi-Fi join."""
    with _connect_lock:
        for st in _connect_states.values():
            st["done"] = False
        _addr_running.clear()


_kicking = False


def kick():
    """The phone rejoined the AP and re-advertises _carplay-ctrl with a NEW port.
    Avahi still serves the stale cached port (~120s TTL), so re-browse in a short
    loop: a fresh ServiceBrowser queries the network, the phone answers with its
    current port, and the next probe hits it. Stops once a connect succeeds. Only
    probes while the latch is clear, so it is a no-op during a live session."""
    GLib.idle_add(_start_kick)


def _start_kick():
    global _kicking
    if _kicking:
        return False
    _kicking = True
    rounds = [15]

    def _tick():
        global _kicking
        if (_connect_states and all(st["done"] for st in _connect_states.values())) or rounds[0] <= 0:
            _kicking = False
            return False
        rounds[0] -= 1
        try:
            if _rebrowse_fn is not None:
                _rebrowse_fn()
        except Exception as e:
            print(f"[cp] re-browse failed: {e!r}", flush=True)
        return True

    _tick()
    GLib.timeout_add(1500, _tick)
    return False


def _connect_worker(address, port, ifname, mac_int, ckey, akey):
    """Probe /ctrl-int/1/connect with quick retries until the phone accepts it and
    opens the reverse control connection to our :7000."""
    st = _cstate(ckey)

    is_v6 = ":" in address
    host = address.split("%", 1)[0]
    scope = 0
    if is_v6 and "%" in address:
        try:
            scope = socket.if_nametoindex(address.split("%", 1)[1])
        except OSError:
            scope = 0
    hosthdr = f"[{host}]:{port}" if is_v6 else f"{host}:{port}"
    req = (
        f"GET /ctrl-int/1/connect HTTP/1.1\r\n"
        f"Host: {hosthdr}\r\n"
        f"User-Agent: AirPlay/280.33.8\r\n"
        f"AirPlay-Receiver-Device-ID: {mac_int}\r\n"
        f"Connection: close\r\n\r\n"
    ).encode()
    try:
        for attempt in range(1, 8):
            if st["done"]:
                return
            sock = None
            try:
                fam = socket.AF_INET6 if is_v6 else socket.AF_INET
                sock = socket.socket(fam, socket.SOCK_STREAM)
                sock.settimeout(3)
                if ifname:
                    sock.setsockopt(socket.SOL_SOCKET, socket.SO_BINDTODEVICE, ifname.encode())
                sock.connect((host, port, 0, scope) if is_v6 else (host, port))
                sock.sendall(req)
                data = sock.recv(256)
                if not data:
                    raise OSError("empty response")
                first = data.split(b"\r\n", 1)[0].decode("ascii", "replace")
                print(f"[cp] /ctrl-int/1/connect -> {first!r} via {ifname or '?'} (attempt {attempt})", flush=True)
                with _connect_lock:
                    st["done"] = True
                return
            except ConnectionRefusedError as e:
                # Nothing is listening: this advertised port is stale. Stop so a
                # fresh re-browse (kick) can pick up the phone's current port.
                print(f"[cp] /ctrl-int/1/connect attempt {attempt} refused: {e!r}", flush=True)
                return
            except Exception as e:
                print(f"[cp] /ctrl-int/1/connect attempt {attempt} failed: {e!r}", flush=True)
                time.sleep(1)
            finally:
                if sock is not None:
                    try:
                        sock.close()
                    except Exception:
                        pass
    finally:
        with _connect_lock:
            _addr_running.discard(akey)


def _publish(bus, server, name, txt):
    # Register the _airplay._tcp service, tolerating an Avahi name collision.
    # On restart the just-evicted old helper's entry group can still linger in
    # avahi-daemon; retry the same name until that ghost expires, then fall back
    # to an alternative name as a last resort.
    group = dbus.Interface(bus.get_object(avahi.DBUS_NAME, server.EntryGroupNew()),
                           avahi.DBUS_INTERFACE_ENTRY_GROUP)
    for attempt in range(16):
        try:
            group.AddService(avahi.IF_UNSPEC, avahi.PROTO_UNSPEC, dbus.UInt32(0),
                             name, "_airplay._tcp", '', '', dbus.UInt16(AIRPLAY_PORT),
                             avahi.string_array_to_txt_array(txt))
            group.Commit()
            if name != SERVICE_NAME:
                print(f"[cp] avahi published as '{name}'", flush=True)
            return group
        except dbus.exceptions.DBusException as e:
            if "Collision" not in str(e):
                raise
            try:
                group.Reset()
            except dbus.exceptions.DBusException:
                pass
            if attempt < 10:
                time.sleep(0.5)
            else:
                name = str(server.GetAlternativeServiceName(name))
    raise RuntimeError("avahi: could not publish _airplay._tcp (name collision persisted)")


def start_service(device_id):
    bus = dbus.SystemBus()
    server = dbus.Interface(bus.get_object(avahi.DBUS_NAME, avahi.DBUS_PATH_SERVER), avahi.DBUS_INTERFACE_SERVER)

    # features2 must be 0x61 = isCarplay(0x01) | carPlayControl(0x20) |
    # CoreUtilsPairingAndEncryption(0x40). pi/pk carry the AirPlay-2 pairing
    # identity; without them (and the 0x40 bit) the iPhone refuses :7000.
    txt = [
        f"deviceID={device_id}",
        "features=0x44540380,0x61",
        "flags=0x4",
        "model=LIVI",
        "srcvers=280.33.8",
        "protovers=1.1",
    ]
    if PI:
        txt.append(f"pi={PI}")
    if PK:
        txt.append(f"pk={PK}")

    _publish(bus, server, SERVICE_NAME, txt)

    def on_service(interface, protocol, name, type, domain, flags):
        try:
            _ifn = socket.if_indextoname(int(interface))
        except Exception:
            _ifn = str(interface)
        print(f"[cp] bonjour: _carplay-ctrl ItemNew name={name} iface={_ifn} proto={protocol}", flush=True)
        try:
            (interface, protocol, name, type, domain, host, aprotocol,
             address, port, txt, flags) = server.ResolveService(
                interface, protocol, name, type, domain, avahi.PROTO_UNSPEC, 0)
            txt = [''.join(str(t) for t in entry) for entry in txt]
            address = str(address)

            # CarPlay wireless uses IPv6 link-local; bracket it and add the
            # interface zone id for fe80:: reachability.
            is_v6 = int(aprotocol) == int(avahi.PROTO_INET6)
            try:
                ifname = socket.if_indextoname(int(interface))
            except OSError:
                ifname = ""
            if is_v6:
                if not address.lower().startswith("fe80"):
                    print(f"[cp] ignoring non-link-local v6 {address}", flush=True)
                    return
                if ifname:
                    address = f"{address}%{ifname}"
                url_host = f"[{address}]"
            else:
                if address.startswith("169.254."):
                    print(f"[cp] ignoring link-local v4 {address}", flush=True)
                    return
                url_host = address
            print(f"[cp] found phone _carplay-ctrl {'v6' if is_v6 else 'v4'} at {url_host}:{port} txt={txt}", flush=True)

            # The phone's own BT MAC is advertised as id= in the service TXT; this
            # links the WiFi IP to its stable identity for the device registry.
            phone_bt = next((t[3:].strip().lower() for t in txt if t.startswith("id=")), "")
            if phone_bt:
                try:
                    from iap2 import livi_sock
                    livi_sock.push({"type": "device", "src": "bonjour",
                                    "btMac": phone_bt, "ip": address.split("%")[0]})
                except Exception:
                    pass

            mac_int = int(device_id.replace(":", ""), 16)
            # The "device-found" probe (below, in a retry thread) is what makes the
            # phone open the reverse control connection back to our :7000. Spawn one
            # retry worker per discovery, guarded so re-announces don't pile up.
            ckey = phone_bt or url_host
            st = _cstate(ckey)
            akey = (ckey, address)
            with _connect_lock:
                if st["done"] or akey in _addr_running:
                    return
                _addr_running.add(akey)
            threading.Thread(
                target=_connect_worker, args=(address, port, ifname, mac_int, ckey, akey), daemon=True
            ).start()
        except Exception as e:
            print(f"[cp] resolve/connect setup failed: {e!r}", flush=True)

    browser = None
    receiver = None

    def _rebrowse():
        nonlocal browser, receiver
        if receiver is not None:
            try:
                receiver.remove()
            except Exception:
                pass
        if browser is not None:
            try:
                server.ServiceBrowserFree(browser)
            except Exception:
                pass
        browser = server.ServiceBrowserNew(
            avahi.IF_UNSPEC, avahi.PROTO_UNSPEC, '_carplay-ctrl._tcp', 'local', 0)
        receiver = bus.add_signal_receiver(on_service, "ItemNew", path=browser)

    global _rebrowse_fn
    _rebrowse_fn = _rebrowse
    _rebrowse()
