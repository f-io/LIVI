import asyncio
import ctypes
import os
import signal
import subprocess
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

from shared import bt_common, wifi_ap
from shared.config import BT_ADAPTER, BTNAME, WIFI_IFACE

AA_WIRELESS = os.environ.get("LIVI_AA_WIRELESS") == "1"
CP_WIRELESS = os.environ.get("LIVI_CP_WIRELESS", "1") == "1"
ADAPTER_PATH = "/org/bluez/" + BT_ADAPTER

def log(*args):
    print("[livi-helper]", *args, flush=True)

class SharedCtx:
    def __init__(self, bus, loop):
        self.bus = bus
        self.adapter = BT_ADAPTER
        self.adapter_path = ADAPTER_PATH
        self.loop = loop
        self.log = log
        self._session = {"v": False}
        self.set_aa_wireless = None
        self.set_cp_wireless = None

    def set_session_active(self, active):
        self._session["v"] = bool(active)

    def session_active(self):
        return self._session["v"]

AGENT_IFACE = "org.bluez.Agent1"

class PairingAgent(dbus.service.Object):
    @dbus.service.method(AGENT_IFACE, in_signature="", out_signature="")
    def Release(self):
        pass

    @dbus.service.method(AGENT_IFACE, in_signature="os", out_signature="")
    def AuthorizeService(self, device, uuid):
        pass

    @dbus.service.method(AGENT_IFACE, in_signature="o", out_signature="s")
    def RequestPinCode(self, device):
        return "0000"

    @dbus.service.method(AGENT_IFACE, in_signature="o", out_signature="u")
    def RequestPasskey(self, device):
        return dbus.UInt32(0)

    @dbus.service.method(AGENT_IFACE, in_signature="ouq", out_signature="")
    def DisplayPasskey(self, device, passkey, entered):
        pass

    @dbus.service.method(AGENT_IFACE, in_signature="os", out_signature="")
    def DisplayPinCode(self, device, pincode):
        pass

    @dbus.service.method(AGENT_IFACE, in_signature="ou", out_signature="")
    def RequestConfirmation(self, device, passkey):
        pass

    @dbus.service.method(AGENT_IFACE, in_signature="o", out_signature="")
    def RequestAuthorization(self, device):
        pass

    @dbus.service.method(AGENT_IFACE, in_signature="", out_signature="")
    def Cancel(self):
        pass

def _prepare_process():
    """Require root, die with the parent (PR_SET_PDEATHSIG), and SIGKILL any stale
    helper before touching wlan0/BlueZ."""
    if os.geteuid() != 0:
        log("ERROR: must run as root")
        sys.exit(1)
    try:
        ctypes.CDLL("libc.so.6", use_errno=True).prctl(1, signal.SIGTERM, 0, 0, 0)
    except OSError:
        pass
    keep = {os.getpid(), os.getppid()}
    evicted = False
    for pat in ("livi-helper.py", "aa-bluetooth.py", "cp-bluetooth.py"):
        try:
            pids = subprocess.check_output(["pgrep", "-f", pat], text=True).split()
        except subprocess.CalledProcessError:
            pids = []
        for pid_s in pids:
            try:
                pid = int(pid_s)
            except ValueError:
                continue
            if pid in keep:
                continue
            try:
                os.kill(pid, signal.SIGKILL)
                log(f"evicted stale helper pid {pid} ({pat})")
                evicted = True
            except OSError:
                pass
    if evicted:
        time.sleep(1.0)

def _power_on_bt():
    try:
        subprocess.run(["rfkill", "unblock", "bluetooth"], capture_output=True, check=False)
    except FileNotFoundError:
        pass

_DROPIN_DIR = "/etc/systemd/system/bluetooth.service.d"
_DROPIN_CFG = os.path.join(_DROPIN_DIR, "livi-no-sap.conf")
_MAIN_CONF = "/etc/bluetooth/main.conf"
_BT_CLASS = "0x200418"

def _find_bluetoothd():
    for p in ("/usr/libexec/bluetooth/bluetoothd", "/usr/lib/bluetooth/bluetoothd",
              "/usr/sbin/bluetoothd"):
        if os.path.isfile(p):
            return p
    try:
        out = subprocess.check_output(
            ["systemctl", "cat", "bluetooth.service"], text=True, stderr=subprocess.DEVNULL)
        for line in out.splitlines():
            line = line.strip()
            if line.startswith("ExecStart=") and "bluetoothd" in line:
                binary = line.split("=", 1)[1].split()[0]
                if os.path.isfile(binary):
                    return binary
    except Exception:
        pass
    raise RuntimeError("Cannot find bluetoothd binary")

def _write_sap_dropin():
    """Write bluetoothd --noplugin=sap so RFCOMM channel 8 is free for AAP."""
    content = f"[Service]\nExecStart=\nExecStart={_find_bluetoothd()} --noplugin=sap\n"
    changed = False
    if os.path.isdir(_DROPIN_DIR):
        try:
            for entry in os.listdir(_DROPIN_DIR):
                if not entry.startswith("livi-") or not entry.endswith(".conf"):
                    continue
                path = os.path.join(_DROPIN_DIR, entry)
                if os.path.abspath(path) == os.path.abspath(_DROPIN_CFG):
                    continue
                try:
                    os.remove(path)
                    changed = True
                except OSError:
                    pass
        except OSError:
            pass
    try:
        current = open(_DROPIN_CFG).read() if os.path.exists(_DROPIN_CFG) else ""
    except OSError:
        current = ""
    if current != content:
        os.makedirs(_DROPIN_DIR, exist_ok=True)
        with open(_DROPIN_CFG, "w") as f:
            f.write(content)
        changed = True
    return changed

def _ensure_main_conf_class():
    """Set [General] Class = 0x200418 in /etc/bluetooth/main.conf."""
    desired = f"Class = {_BT_CLASS}"
    try:
        original = open(_MAIN_CONF).read()
    except OSError:
        return False
    out = []
    in_general = False
    seen = False
    for line in original.splitlines():
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            if in_general and not seen:
                out.append(desired)
                seen = True
            in_general = stripped == "[General]"
            out.append(line)
            continue
        if in_general:
            cleaned = stripped.lstrip("#").strip()
            if cleaned.startswith("Class") and "=" in cleaned:
                if seen:
                    continue
                out.append(desired)
                seen = True
                continue
        out.append(line)
    if in_general and not seen:
        out.append(desired)
        seen = True
    if not seen:
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
        return True
    except OSError:
        return False

def _setup_bluetoothd():
    changed = _write_sap_dropin()
    changed = _ensure_main_conf_class() or changed
    if changed:
        log("restarting bluetoothd to apply --noplugin=sap + device class")
        subprocess.run(["systemctl", "daemon-reload"], check=False)
        subprocess.run(["systemctl", "restart", "bluetooth"], check=False)
        time.sleep(5)

def main():
    _prepare_process()
    DBusGMainLoop(set_as_default=True)

    aio = asyncio.new_event_loop()
    threading.Thread(target=aio.run_forever, daemon=True, name="cp-asyncio").start()

    bus = dbus.SystemBus()
    ctx = SharedCtx(bus, aio)

    def _adapter_props():
        return dbus.Interface(bus.get_object("org.bluez", ADAPTER_PATH), dbus.PROPERTIES_IFACE)

    handlers = []
    from bt import cp_handler
    h = cp_handler.create(ctx)
    h.register()
    handlers.append(h)
    log("CarPlay (iAP2) handler registered (wired CarPlay live)")

    # AA BT profile switch
    aa_ref = {"h": None}

    def _toggle_aa(enabled):
        try:
            if enabled and aa_ref["h"] is None:
                from bt import aa_handler
                ah = aa_handler.create(ctx)
                ah.register()
                aa_ref["h"] = ah
                handlers.append(ah)
                log("Android Auto (AAP) handler registered")
            elif not enabled and aa_ref["h"] is not None:
                ah = aa_ref["h"]
                aa_ref["h"] = None
                if ah in handlers:
                    handlers.remove(ah)
                ah.teardown()
                log("Android Auto (AAP) handler unregistered")
        except Exception as e:
            log("AAP toggle failed:", repr(e))
        return False

    stopping = threading.Event()
    agent_ref = {"agent": None}
    ws = {"active": False, "busy": False, "worker": False,
          "want_aa": AA_WIRELESS, "want_cp": CP_WIRELESS}

    def _want_wireless():
        return ws["want_aa"] or ws["want_cp"]

    def _apply_protocols():
        h.set_cp_wireless(ws["active"] and ws["want_cp"])
        _toggle_aa(ws["active"] and ws["want_aa"])

    def _sync_wireless():
        if stopping.is_set() or ws["busy"]:
            return False
        if _want_wireless() and not ws["active"]:
            ws["busy"] = True
            log("wireless: bringing up (aa=%s cp=%s)" % (ws["want_aa"], ws["want_cp"]))
            threading.Thread(target=_wireless_bringup, daemon=True,
                             name="wireless-bringup").start()
        elif not _want_wireless() and ws["active"]:
            ws["busy"] = True
            log("wireless: tearing down (wired sessions stay up)")
            _teardown_wireless()
        else:
            _apply_protocols()
        return False

    def _wireless_bringup():
        _power_on_bt()
        _setup_bluetoothd()
        if stopping.is_set():
            return
        ap_ready = False
        try:
            log(f"bringing up Wi-Fi AP (iface={WIFI_IFACE})")
            ap_ready = wifi_ap.setup_ap()
        except Exception as e:
            log("Wi-Fi AP setup failed:", repr(e))
        log("Wi-Fi AP ready" if ap_ready else "Wi-Fi AP NOT ready")
        if stopping.is_set():
            try:
                wifi_ap.teardown_ap()
            except Exception:
                pass
            return
        GLib.idle_add(_bringup_done, ap_ready)

    def _bringup_done(ap_ready):
        ws["busy"] = False
        if stopping.is_set():
            return False
        try:
            adapter = _adapter_props()
            adapter.Set("org.bluez.Adapter1", "Alias", BTNAME)
            adapter.Set("org.bluez.Adapter1", "DiscoverableTimeout", dbus.UInt32(0))
            adapter.Set("org.bluez.Adapter1", "Powered", True)

            if agent_ref["agent"] is None:
                agent = PairingAgent(bus, "/org/bluez/livi_agent")
                agent_manager = dbus.Interface(
                    bus.get_object("org.bluez", "/org/bluez"), "org.bluez.AgentManager1")
                agent_manager.RegisterAgent(agent, "KeyboardDisplay")
                agent_manager.RequestDefaultAgent(agent)
                agent_ref["agent"] = agent

            ws["active"] = True
            _apply_protocols()

            if ap_ready:
                adapter.Set("org.bluez.Adapter1", "Discoverable", True)
                adapter.Set("org.bluez.Adapter1", "Pairable", True)
                log("BT advertising enabled (discoverable + pairable)")
            else:
                adapter.Set("org.bluez.Adapter1", "Discoverable", False)
                log("BT advertising held (AP not ready)")

            if not ws["worker"]:
                ws["worker"] = True
                bt_common.start_reconnect_worker(
                    BT_ADAPTER,
                    lambda: (not ws["active"]) or ctx.session_active()
                            or bt_common.wifi_has_station(WIFI_IFACE),
                    log,
                )
        except Exception as e:
            log("wireless BT setup failed:", repr(e))
        GLib.idle_add(_sync_wireless)
        return False

    def _teardown_wireless():
        try:
            adapter = _adapter_props()
            for prop in ("Discoverable", "Pairable"):
                try:
                    adapter.Set("org.bluez.Adapter1", prop, False)
                except Exception:
                    pass
        except Exception as e:
            log("wireless teardown (bt) error:", repr(e))
        ws["active"] = False
        _apply_protocols()

        def _ap_down():
            try:
                wifi_ap.teardown_ap()
            except Exception as e:
                log("AP teardown error:", repr(e))
            GLib.idle_add(_teardown_done)

        threading.Thread(target=_ap_down, daemon=True,
                         name="wireless-teardown").start()

    def _teardown_done():
        ws["busy"] = False
        log("wireless: down")
        GLib.idle_add(_sync_wireless)
        return False

    def _set_aa_wireless(enabled):
        ws["want_aa"] = bool(enabled)
        GLib.idle_add(_sync_wireless)

    def _set_cp_wireless(enabled):
        ws["want_cp"] = bool(enabled)
        GLib.idle_add(_sync_wireless)

    ctx.set_aa_wireless = _set_aa_wireless
    ctx.set_cp_wireless = _set_cp_wireless

    if _want_wireless():
        GLib.idle_add(_sync_wireless)
    else:
        log("wireless off at start: Bluetooth + Wi-Fi AP idle (runtime toggle armed)")

    mainloop = GLib.MainLoop()

    def _shutdown():
        log("shutting down: stop advertising, unregister, tear down AP")
        stopping.set()
        for prop in ("Discoverable", "Pairable"):
            try:
                _adapter_props().Set("org.bluez.Adapter1", prop, False)
            except Exception:
                pass
        for h in handlers:
            try:
                h.teardown()
            except Exception:
                pass
        try:
            wifi_ap.teardown_ap()
        except Exception as e:
            log("teardown error:", repr(e))
        try:
            aio.call_soon_threadsafe(aio.stop)
        except Exception:
            pass
        mainloop.quit()

    for _sig in (signal.SIGTERM, signal.SIGINT):
        try:
            GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, _sig,
                                 lambda *_: (_shutdown() or True))
        except Exception:
            signal.signal(_sig, lambda *_: _shutdown())

    mainloop.run()

if __name__ == "__main__":
    main()
