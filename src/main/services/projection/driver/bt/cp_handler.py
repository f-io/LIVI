import asyncio
import base64
import binascii
import json
import os
import subprocess
import sys
import time

_HERE = os.path.dirname(os.path.abspath(__file__))
for _cand in (
    os.path.normpath(os.path.join(_HERE, "..", "cp")),
    os.path.normpath(os.path.join(_HERE, "cp")),
):
    if os.path.isdir(os.path.join(_cand, "iap2")) and _cand not in sys.path:
        sys.path.insert(0, _cand)
        break

import dbus
from gi.repository import GLib

import iap2.carplay_bonjour as carplay_bonjour
from iap2 import livi_sock
from shared.config import (
    AIRPLAY_PORT, AVAILABLE_CURRENT_MA, BT_ADAPTER, CARPLAY_SOURCE_VERSION,
    CARPLAY_WIRED_START_SESSION, CHANNEL, DEBUG, MANUFACTURER, MODEL, NAME, PASSPHRASE, PI,
    SECURITY_TYPE, SSID,
)
from iap2.control_session_message import Int16, Uint8, Uint16, Uint32, read_csm, register_csm, write_csm
from iap2.control_session_message.authentication import (
    AuthenticationCertificate, AuthenticationFailed, AuthenticationResponse,
    AuthenticationSucceeded, RequestAuthenticationCertificate,
    RequestAuthenticationChallengeResponse,
)
from iap2.control_session_message.car_play import (
    CarPlayAvailability, CarPlayStartSession, CarPlayStartSessionWiredAttributes,
    DeviceTransportIdentifierNotification, WirelessCarPlayUpdate,
)
from iap2.control_session_message.eap import (
    StartExternalAccessoryProtocolSession, StopExternalAccessoryProtocolSession,
)
from iap2.control_session_message.identification import (
    BluetoothTransportComponent, EngineType, ExternalAccessoryProtocol,
    IdentificationAccepted, IdentificationInformation, IdentificationRejected,
    LocationInformationComponent, MatchAction, PowerProvidingCapability,
    RouteGuidanceDisplayComponent, StartIdentification, USBHostTransportComponent,
    VehicleInformationComponent, VehicleStatusComponent, WirelessCarPlayTransportComponent,
)
from iap2.control_session_message.location import (
    LocationInformation, StartLocationInformation, StopLocationInformation,
)
from iap2.control_session_message.now_playing import (
    NowPlayingUpdate, PlaybackStatus, StartMediaItemAttributes, StartNowPlayingUpdates,
    StartPlaybackAttributes, StopNowPlayingUpdates,
)
from iap2.control_session_message.power import (
    PowerUpdate, PowerSourceUpdate, StartPowerUpdates, StopPowerUpdates,
)
from iap2.control_session_message.communications import (
    CallStateUpdate, CommunicationsUpdate, StartCallStateUpdates, StartCommunicationsUpdates,
    StopCallStateUpdates, StopCommunicationsUpdates,
)
from iap2.control_session_message.route_guidance import (
    RouteGuidanceManeuverUpdate, RouteGuidanceUpdate, StartRouteGuidanceUpdates,
    StopRouteGuidanceUpdates,
)
from iap2.control_session_message.vehicle_status import (
    StartVehicleStatusUpdates, StopVehicleStatusUpdates, VehicleStatusUpdate,
)
from iap2.control_session_message.wifi import (
    AccessoryWiFiConfigurationInformation, RequestAccessoryWiFiConfigurationInformation,
    SecurityType,
)
from iap2.file_transfer import FileTransferReceiver
from iap2.link_layer import IAP2Connection
from iap2.mfi_auth_coprocessor import (
    generate_challenge_response,
    init as mfi_init,
    power_off as mfi_power_off,
    read_certificate,
)
from iap2.transport.bluetooth import (
    CHANNEL as IAP_CHANNEL, IAP_CLIENT_UUID, IAP_RECORD, IAP_SERVER_UUID, IAPProfile,
)
from iap2.wifi_ap import get_bt_mac
from iap2 import muxd, ncm_bridge

AV_IFACE_DRIVERS = ("cdc_ncm", "ipheth")

SECURITY_TYPE_MAP = {
    "NONE": SecurityType.NONE,
    "WEP": SecurityType.WEP,
    "WPA_WPA2": SecurityType.WPA_WPA2,
    "WPA3_TRANSITION": SecurityType.WPA3_TRANSITION,
    "WPA3_ONLY": SecurityType.WPA3_ONLY,
}

for _msg in (
    RequestAuthenticationCertificate, RequestAuthenticationChallengeResponse,
    AuthenticationSucceeded, AuthenticationFailed, StartIdentification,
    IdentificationAccepted, IdentificationRejected, StartVehicleStatusUpdates,
    StopVehicleStatusUpdates, DeviceTransportIdentifierNotification, WirelessCarPlayUpdate,
    RequestAccessoryWiFiConfigurationInformation, NowPlayingUpdate,
    RouteGuidanceUpdate, RouteGuidanceManeuverUpdate,
    StartLocationInformation, StopLocationInformation, PowerUpdate,
    CommunicationsUpdate, CallStateUpdate, CarPlayAvailability,
):
    register_csm(_msg)


class _IapPeer:
    def __init__(self, cid):
        self.cid = cid
        self.calls = {}
        self.last_call_phase = "ended"
        self.navi = {}
        self.navi_maneuvers = {}
        self.navi_current_index = None
        self.stream = None
        self.location_types = set()
        self.vehicle_status_active = False
        self.phone_id = ""
        self.usb_transport_id = ""
        self.np_sig = None


class CpHandler:
    def __init__(self, ctx):
        self.ctx = ctx
        self.loop = ctx.loop
        self._server = None
        self._client = None
        self._bt_writer = None
        self._tasks = set()
        self._peers = set()
        self._vehicle_status = {}
        self._carkit_task = None
        self._carkit_iap_count = 0
        self._carkit_ll_ifaces = set()
        try:
            mfi_init()
        except Exception as e:
            self._log("mfi init failed (auth will retry on demand):", repr(e))

    def _log(self, *args):
        self.ctx.log("[cp] " + " ".join(str(a) for a in args))

    def _ensure_carkit_link_local(self):
        for _ in range(15):
            if self._carkit_link_local_once():
                return
            time.sleep(1)
        self._log("[carkit-net] no USB-ethernet iface with a phone address appeared (15s)")

    def _carkit_link_local_once(self):
        try:
            info = json.loads(subprocess.run(
                ["ip", "-json", "addr"], capture_output=True, text=True, timeout=5).stdout)
        except Exception as e:
            self._log("[carkit-net] ip addr failed:", repr(e))
            return False
        iface = None
        has_ll_v4 = False
        for link in info:
            name = link.get("ifname", "")
            if name == "lo" or name.startswith(("wlan", "ap")):
                continue
            addrs = link.get("addr_info", [])
            v4 = [a.get("local", "") for a in addrs if a.get("family") == "inet"]
            if any(a.startswith("172.20.10.") or a.startswith("169.254.") for a in v4):
                iface = name
                has_ll_v4 = any(a.startswith("169.254.") for a in v4)
                break
        if not iface:
            return False
        if not has_ll_v4:
            subprocess.run(["ip", "addr", "add", "169.254.1.1/16", "dev", iface],
                           capture_output=True, text=True, timeout=5)
            self._log("[carkit-net] added link-local IPv4 169.254.1.1 to %s" % iface)
        self._carkit_ll_ifaces.add(iface)
        try:
            subprocess.run(["sysctl", "-w", "net.ipv6.conf.%s.accept_ra=0" % iface],
                           capture_output=True, text=True, timeout=5)
            subprocess.run(["ip", "-6", "addr", "flush", "dev", iface, "scope", "global"],
                           capture_output=True, text=True, timeout=5)
            subprocess.run(
                ["nmcli", "device", "modify", iface,
                 "ipv6.method", "link-local",
                 "ipv4.never-default", "yes", "ipv6.never-default", "yes"],
                capture_output=True, text=True, timeout=10)
            self._log("[carkit-net] applied link-local to %s (runtime, not persisted)" % iface)
        except Exception as e:
            self._log("[carkit-net] apply failed:", repr(e))
        return True

    def _carkit_link_local_cleanup(self):
        for iface in list(self._carkit_ll_ifaces):
            try:
                subprocess.run(["ip", "addr", "del", "169.254.1.1/16", "dev", iface],
                               capture_output=True, text=True, timeout=5)
                subprocess.run(["nmcli", "device", "reapply", iface],
                               capture_output=True, text=True, timeout=5)
            except Exception:
                pass
        self._carkit_ll_ifaces.clear()

    def _av_watch(self):
        try:
            air = subprocess.run(["avahi-browse", "-rtp", "_airplay._tcp"],
                                 capture_output=True, text=True, timeout=4).stdout
            for ln in air.splitlines():
                if ln.startswith("=") and "LIVI" in ln and ";lo;" not in ln:
                    self._log("[av-watch] our _airplay:", ln)
        except Exception as e:
            self._log("[av-watch] airplay browse failed:", repr(e))
        for _ in range(15):
            try:
                out = subprocess.run(["avahi-browse", "-rtp", "_carplay-ctrl._tcp"],
                                     capture_output=True, text=True, timeout=4).stdout
                hits = [ln for ln in out.splitlines() if ln.startswith("=")]
                if hits:
                    for ln in hits:
                        self._log("[av-watch] ctrl:", ln)
                else:
                    self._log("[av-watch] no _carplay-ctrl advertised yet")
            except Exception as e:
                self._log("[av-watch] browse failed:", repr(e))
            time.sleep(4)

    def _carkit_av_iface(self, serial):
        tap = ncm_bridge.av_iface(serial)
        if tap and os.path.isdir("/sys/class/net/%s" % tap):
            return tap
        usb_root = ""
        if serial:
            dev = muxd.find_dev(serial)
            if dev:
                usb_root = os.path.realpath(dev[2])
        best = ""
        best_rank = len(AV_IFACE_DRIVERS)
        try:
            names = sorted(os.listdir("/sys/class/net"))
        except OSError:
            return ""
        for name in names:
            base = "/sys/class/net/%s/device" % name
            try:
                drv = os.path.basename(os.path.realpath(base + "/driver"))
            except OSError:
                continue
            if drv not in AV_IFACE_DRIVERS:
                continue
            if usb_root and not os.path.realpath(base).startswith(usb_root + "/"):
                continue
            rank = AV_IFACE_DRIVERS.index(drv)
            if rank < best_rank:
                best, best_rank = name, rank
        return best

    def _iface_fe80(self, iface):
        if not iface:
            return ""
        try:
            info = json.loads(subprocess.run(
                ["ip", "-json", "addr", "show", "dev", iface],
                capture_output=True, text=True, timeout=5).stdout)
        except Exception:
            return ""
        for link in info:
            for a in link.get("addr_info", []):
                if a.get("family") == "inet6" and a.get("scope") == "link":
                    return a.get("local", "")
        return ""

    def _iface_eui64_fe80(self, iface):
        try:
            mac = open("/sys/class/net/%s/address" % iface).read().strip()
        except OSError:
            return ""
        b = [int(x, 16) for x in mac.split(":")]
        if len(b) != 6:
            return ""
        return "fe80::%x:%x:%x:%x" % (
            (b[0] ^ 0x02) << 8 | b[1], b[2] << 8 | 0xFF,
            0xFE << 8 | b[3], b[4] << 8 | b[5])

    def _iface_carrier(self, iface):
        try:
            return open("/sys/class/net/%s/carrier" % iface).read().strip() == "1"
        except OSError:
            return False

    def _ensure_carkit_av_link(self, serial):
        iface = ""
        for _ in range(15):
            iface = self._carkit_av_iface(serial)
            if iface:
                break
            time.sleep(1)
        if not iface:
            self._log("[carkit-net] no AV iface (cdc_ncm/ipheth) for udid=%s (15s)"
                      % (serial[:8] if serial else "?"))
            return
        try:
            if not os.path.isdir("/sys/class/net/%s/device" % iface):
                if not self._iface_fe80(iface):
                    fe80 = self._iface_eui64_fe80(iface)
                    if fe80:
                        subprocess.run(["ip", "addr", "add", fe80 + "/64", "dev", iface],
                                       capture_output=True, text=True, timeout=5)
                        self._log("[carkit-net] added %s on %s (userspace NCM)"
                                  % (fe80, iface))
                return
            subprocess.run(["ip", "link", "set", iface, "up"],
                           capture_output=True, text=True, timeout=5)
            carrier = self._iface_carrier(iface)
            active = subprocess.run(
                ["nmcli", "-g", "GENERAL.CONNECTION", "device", "show", iface],
                capture_output=True, text=True, timeout=10).stdout.strip()
            name = "livi-carkit-%s" % iface
            if not active.startswith("livi-carkit"):
                have = subprocess.run(
                    ["nmcli", "-g", "connection.id", "connection", "show", name],
                    capture_output=True, text=True, timeout=10).returncode == 0
                if not have:
                    r = subprocess.run(
                        ["nmcli", "connection", "add", "type", "ethernet",
                         "ifname", iface, "con-name", name,
                         "ipv4.method", "disabled", "ipv6.method", "link-local",
                         "ipv6.addr-gen-mode", "eui64",
                         "connection.autoconnect", "yes",
                         "connection.autoconnect-priority", "999"],
                        capture_output=True, text=True, timeout=10)
                    if r.returncode == 0:
                        self._log("[carkit-net] added NM profile %s (link-local only)" % name)
                    else:
                        self._log("[carkit-net] NM profile add failed:", r.stderr.strip())
                elif carrier:
                    subprocess.run(["nmcli", "connection", "up", name],
                                   capture_output=True, text=True, timeout=10)
                    self._log("[carkit-net] activated NM profile %s" % name)
            if not carrier and not self._iface_fe80(iface):
                fe80 = self._iface_eui64_fe80(iface)
                if fe80:
                    subprocess.run(["ip", "addr", "add", fe80 + "/64", "dev", iface],
                                   capture_output=True, text=True, timeout=5)
                    self._log("[carkit-net] pre-added %s on %s (no carrier yet)"
                              % (fe80, iface))
        except Exception as e:
            self._log("[carkit-net] AV link setup failed:", repr(e))

    async def _wait_carkit_fe80(self, serial, timeout=20.0):
        fe80 = self._iface_fe80(self._carkit_av_iface(serial))
        if fe80:
            return fe80
        fut = self.loop.create_future()
        handle = {}

        def _resolve(addr):
            if not fut.done():
                fut.set_result(addr)

        def _on_nm_state(*_args):
            addr = self._iface_fe80(self._carkit_av_iface(serial))
            if addr:
                self.loop.call_soon_threadsafe(_resolve, addr)

        def _subscribe():
            try:
                handle["match"] = self.ctx.bus.add_signal_receiver(
                    _on_nm_state,
                    dbus_interface="org.freedesktop.NetworkManager.Device",
                    signal_name="StateChanged",
                    bus_name="org.freedesktop.NetworkManager")
            except Exception as e:
                self._log("[carkit-net] NM signal subscribe failed:", repr(e))
            _on_nm_state()
            return False

        def _unsubscribe():
            match = handle.pop("match", None)
            if match is not None:
                try:
                    match.remove()
                except Exception:
                    pass
            return False

        self._log("[carkit-net] waiting for AV link-local (NM StateChanged, udid=%s)"
                  % (serial[:8] if serial else "?"))
        GLib.idle_add(_subscribe)
        try:
            return await asyncio.wait_for(fut, timeout)
        except asyncio.TimeoutError:
            return ""
        finally:
            GLib.idle_add(_unsubscribe)

    async def _send_carplay_start_session(self, stream, avail, serial=""):
        wired = getattr(avail, "wired_attributes", None)
        if wired is None or not getattr(wired, "available", False):
            self._log("CarPlayAvailability: wired not available, not starting")
            return
        fe80 = await self._wait_carkit_fe80(serial)
        if not fe80:
            self._log("CarPlayStartSession: no AV link-local (20s, udid=%s), not starting"
                      % (serial[:8] if serial else "?"))
            return
        await write_csm(stream, CarPlayStartSession(
            wired_attributes=CarPlayStartSessionWiredAttributes(ip_address=[fe80]),
            port=Uint32(AIRPLAY_PORT),
            device_identifier=get_bt_mac(),
            public_key=PI,
            source_version=CARPLAY_SOURCE_VERSION))
        self._log("CarPlayStartSession sent: fe80=%s port=%d" % (fe80, AIRPLAY_PORT))

    def drop_bt_iap2(self):
        w = self._bt_writer
        self._bt_writer = None
        if w is None:
            return
        self._log("disableBluetooth -> closing BT iAP2 link")
        try:
            w.close()
        except Exception as e:
            self._log("drop bt iap2 error:", repr(e))

    def _tag(self, peer, ev):
        ev["cid"], ev["phoneId"] = peer.cid, peer.phone_id
        if peer.usb_transport_id:
            ev["usbTransportId"] = peer.usb_transport_id
        return ev

    def _push_now_playing(self, peer, upd):
        ev = {"type": "nowplaying"}
        mi = upd.media_item_attributes
        if mi is not None:
            if mi.title:
                ev["title"] = mi.title
            if mi.artist:
                ev["artist"] = mi.artist
            if mi.album:
                ev["album"] = mi.album
            if mi.duration_ms:
                ev["durationMs"] = mi.duration_ms
            if mi.artwork_ftid is not None:
                self._log("nowplaying: artwork offered (ftid=%d)" % mi.artwork_ftid)
        pb = upd.playback_attributes
        if pb is not None:
            if pb.status is not None:
                ev["playing"] = 1 if pb.status == PlaybackStatus.PLAYING else 0
            if pb.elapsed_ms is not None:
                ev["elapsedMs"] = pb.elapsed_ms
            if pb.app_name:
                ev["appName"] = pb.app_name
        if len(ev) > 1:
            self._tag(peer, ev)
            livi_sock.push(ev)
            if DEBUG:
                sig = tuple(ev.get(k) for k in ("title", "artist", "album", "playing", "appName"))
                if sig != peer.np_sig:
                    peer.np_sig = sig
                    self._log("nowplaying " + " ".join(
                        "%s=%s" % (k, ev[k]) for k in ev if k != "type"))

    def _push_power(self, peer, upd):
        ev = {"type": "power"}
        if upd.battery_charge_level is not None:
            ev["level"] = int(upd.battery_charge_level)
        if upd.is_external_charger_connected is not None:
            ev["charging"] = bool(upd.is_external_charger_connected)
        if len(ev) > 1:
            self._tag(peer, ev)
            livi_sock.push(ev)
            self._log("power " + " ".join(
                "%s=%s" % (k, ev[k]) for k in ev if k != "type"))

    def _push_comms(self, peer, upd):
        ev = {"type": "cellular"}
        if upd.signal_strength is not None and len(upd.signal_strength) > 0:
            ev["signal"] = upd.signal_strength[0]
        if upd.carrier_name is not None:
            ev["carrier"] = upd.carrier_name
        if upd.cellular_supported is not None:
            ev["cellularSupported"] = bool(upd.cellular_supported)
        if len(ev) > 1:
            self._tag(peer, ev)
            livi_sock.push(ev)
            self._log("cellular " + " ".join(
                "%s=%s" % (k, ev[k]) for k in ev if k != "type"))

    def _push_call(self, peer, upd):
        status = upd.status
        if status is None:
            return
        uuid = upd.call_uuid or "_"
        if status == 0:
            peer.calls.pop(uuid, None)
        else:
            peer.calls[uuid] = {
                "status": status,
                "number": upd.remote_id,
                "name": upd.display_name,
            }

        active = any(c["status"] in (1, 3, 4, 5, 6) for c in peer.calls.values())
        ringing = any(c["status"] == 2 for c in peer.calls.values())
        phase = "active" if active else "ringing" if ringing else "ended"

        if phase == peer.last_call_phase:
            return
        peer.last_call_phase = phase

        ev = {"type": "call", "phase": phase}
        if phase != "ended":
            for c in peer.calls.values():
                match = c["status"] == 2 if phase == "ringing" else c["status"] in (1, 3, 4, 5, 6)
                if match:
                    if c.get("number"):
                        ev["number"] = c["number"]
                    if c.get("name"):
                        ev["name"] = c["name"]
                    break

        self._tag(peer, ev)
        livi_sock.push(ev)
        suffix = (" " + str(ev["name"])) if ev.get("name") else ""
        self._log("call phase=%s%s" % (phase, suffix))

    def _push_album_art(self, peer, data):
        if not data:
            return
        livi_sock.push({"type": "albumart",
                        "cid": peer.cid,
                        "phoneId": peer.phone_id,
                        "dataB64": base64.b64encode(data).decode()})
        self._log("nowplaying: album art %d bytes" % len(data))

    def _handle_route_guidance(self, peer, upd):
        if upd.state is not None:
            peer.navi["status"] = upd.state
        if upd.maneuver_state is not None:
            peer.navi["orderType"] = upd.maneuver_state
        if upd.current_road_name is not None:
            peer.navi["roadName"] = upd.current_road_name
        if upd.destination_name is not None:
            peer.navi["destinationName"] = upd.destination_name
        if upd.eta is not None:
            peer.navi["etaEpoch"] = upd.eta
        if upd.time_remaining is not None:
            peer.navi["timeToDestination"] = upd.time_remaining
        if upd.distance_remaining is not None:
            peer.navi["distanceToDestination"] = upd.distance_remaining
        if upd.distance_to_maneuver is not None:
            peer.navi["remainDistance"] = upd.distance_to_maneuver
        if upd.current_maneuver_list:
            peer.navi_current_index = int.from_bytes(upd.current_maneuver_list[:2], "big")
        self._push_route_guidance(peer)

    def _handle_route_maneuver(self, peer, upd):
        if upd.index is None:
            return
        m = peer.navi_maneuvers.setdefault(upd.index, {})
        if upd.maneuver_type is not None:
            m["maneuverType"] = upd.maneuver_type
        if upd.driving_side is not None:
            m["turnSide"] = upd.driving_side
        if upd.junction_type is not None:
            m["junctionType"] = upd.junction_type
        if upd.exit_angle is not None:
            m["turnAngle"] = upd.exit_angle
        if upd.after_maneuver_road_name is not None:
            m["afterRoadName"] = upd.after_maneuver_road_name
        self._push_route_guidance(peer)

    def _push_route_guidance(self, peer):
        ev = {"type": "navigation"}
        ev.update(peer.navi)
        cur = peer.navi_maneuvers.get(peer.navi_current_index)
        if cur:
            ev.update(cur)
        if len(ev) > 1:
            self._tag(peer, ev)
            livi_sock.push(ev)
            self._log("navigation " + " ".join(
                "%s=%s" % (k, ev[k]) for k in ev if k != "type"))

    async def _handle_auth(self, stream, cert):
        while True:
            incoming_message = await read_csm(stream)
            if isinstance(incoming_message, RequestAuthenticationCertificate):
                self._log("auth: sending MFi certificate")
                await write_csm(stream, AuthenticationCertificate(certificate=cert))
            elif isinstance(incoming_message, RequestAuthenticationChallengeResponse):
                self._log("auth: signing challenge")
                response = await self.loop.run_in_executor(
                    None, lambda: generate_challenge_response(incoming_message.challenge))
                await write_csm(stream, AuthenticationResponse(response=response))
            elif isinstance(incoming_message, AuthenticationSucceeded):
                self._log("auth: succeeded")
                return
            else:
                self._log("auth: unexpected message %s" % (
                    type(incoming_message).__name__ if incoming_message is not None
                    else "None (unhandled msg_id, see [csm] log)"))
                raise Exception("auth failed")

    async def _handle_identification(self, stream, carkit=False):
        def messages_ids(*messages):
            from struct import Struct
            word = Struct(">H")
            return b"".join(word.pack(m.CSM_MSG_ID) for m in messages)

        def build_identification(exclude):
            mac_str = get_bt_mac()
            mac_bytes = binascii.unhexlify(mac_str.replace(":", ""))
            bt_comp = [BluetoothTransportComponent(
                id=Uint16(0), name="blue", supports_iap2_connection=True,
                bluetooth_transport_mac=mac_bytes)]
            wl_comp = WirelessCarPlayTransportComponent(
                id=Uint16(1), name=SSID, supports_iap2_connection=True,
                supports_car_play=True)
            usb_list = []
            if carkit:
                bt_comp = []
                wl_comp = None
                usb_list = [USBHostTransportComponent(
                    id=Uint16(0), name="USBHostTransport",
                    supports_iap2_connection=True,
                    car_play_interface_number=Uint8(3),
                    supports_car_play=True)]
            cp_start = carkit and CARPLAY_WIRED_START_SESSION
            cp_sent = (CarPlayStartSession,) if cp_start else ()
            cp_recv = (CarPlayAvailability,) if cp_start else ()
            power_sent = (PowerSourceUpdate,) if carkit else ()
            fields = dict(
                name=NAME,
                model_identifier=MODEL,
                manufacturer=MANUFACTURER,
                serial_number="0123456",
                firmware_version="1.0.0",
                hardware_version="1.0",
                messages_sent_by_accessory=messages_ids(
                    VehicleStatusUpdate, AccessoryWiFiConfigurationInformation,
                    StartNowPlayingUpdates, StopNowPlayingUpdates,
                    StartRouteGuidanceUpdates, StopRouteGuidanceUpdates,
                    LocationInformation, StartPowerUpdates, StopPowerUpdates,
                    StartCommunicationsUpdates, StopCommunicationsUpdates,
                    StartCallStateUpdates, StopCallStateUpdates, *power_sent, *cp_sent),
                messages_received_from_accessory=messages_ids(
                    StartExternalAccessoryProtocolSession, StopExternalAccessoryProtocolSession,
                    StartVehicleStatusUpdates, StopVehicleStatusUpdates,
                    WirelessCarPlayUpdate, DeviceTransportIdentifierNotification,
                    RequestAccessoryWiFiConfigurationInformation, NowPlayingUpdate,
                    RouteGuidanceUpdate, RouteGuidanceManeuverUpdate,
                    StartLocationInformation, StopLocationInformation, PowerUpdate,
                    CommunicationsUpdate, CallStateUpdate, *cp_recv,
                ),
                power_providing_capability=(
                    PowerProvidingCapability.ADVANCED if carkit
                    else PowerProvidingCapability.NONE),
                maximum_current_drawn_from_device=Uint16(20),
                supported_external_accessory_protocol=[
                    ExternalAccessoryProtocol(
                        id=Uint8(1),
                        name="en.opencarplay.test",
                        match_action=MatchAction.NONE,
                    )
                ],
                current_language="en",
                supported_language=["en", "de"],
                app_match_team_id=None,
                bluetooth_transport_component=bt_comp,
                usb_host_transport_component=usb_list,
                vehicle_information_component=VehicleInformationComponent(
                    id=Uint16(0),
                    name=NAME,
                    engine_type=EngineType.DIESEL,
                ),
                vehicle_status_component=VehicleStatusComponent(
                    id=Uint16(0),
                    name=NAME,
                    range=True,
                    outside_temperature=True,
                ),
                location_information_component=LocationInformationComponent(
                    id=Uint16(0),
                    name=NAME,
                    global_positioning_system_fix_data=True,
                    recommended_minimum_specific_gps_transit_data=True,
                ),
                wireless_car_play_transport_component=wl_comp,
                route_guidance_display_component=[
                    RouteGuidanceDisplayComponent(id=Uint16(0), name=NAME),
                ],
            )
            for key in exclude:
                if key in fields:
                    fields[key] = [] if isinstance(fields[key], list) else None
            return IdentificationInformation(**fields)

        droppable = {
            "location_information_component",
            "vehicle_information_component",
            "vehicle_status_component",
            "route_guidance_display_component",
        }
        excluded = set()
        while True:
            incoming_message = await read_csm(stream)
            if isinstance(incoming_message, StartIdentification):
                self._log("identification: sending accessory info")
                await write_csm(stream, build_identification(excluded))
            elif isinstance(incoming_message, IdentificationAccepted):
                self._log("identification: accepted")
                return
            elif isinstance(incoming_message, IdentificationRejected):
                import dataclasses
                flagged = [f.name for f in dataclasses.fields(incoming_message)
                           if getattr(incoming_message, f.name, None)]
                self._log("identification REJECTED, flagged fields:", flagged)
                drop = [f for f in flagged if f in droppable and f not in excluded]
                if not drop:
                    raise Exception("identification failed")
                excluded.update(drop)
                self._log("identification: retrying without", drop)
                await write_csm(stream, build_identification(excluded))
            else:
                raise Exception("identification failed")

    def _on_connection(self, reader, writer, over_wifi=False, carkit=False, cid="", usb_serial="", bt_mac=""):
        if over_wifi and self._carkit_iap_count > 0:
            self._log("wired: keeping iAP2 on USB carkit, ignoring redundant CarPlay tunnel"
                      + (" (cid=%s)" % cid if cid else ""))
            return None
        label = "USB (carkit)" if carkit else ("Wi-Fi (CarPlay tunnel)" if over_wifi else "RFCOMM")
        self._log("phone connected over " + label + (" mac=%s" % bt_mac if bt_mac else ""))
        if carkit:
            self._carkit_iap_count += 1
            carplay_bonjour.reset()
            if CARPLAY_WIRED_START_SESSION:
                self.loop.run_in_executor(None, self._ensure_carkit_av_link, usb_serial)
            else:
                self.loop.run_in_executor(None, self._ensure_carkit_link_local)
        elif not over_wifi:
            carplay_bonjour.reset()
            self._bt_writer = writer
            if bt_mac:
                self._remember_wireless_phone(bt_mac)

        direct = over_wifi or carkit
        transport = "usb" if carkit else "wifi" if over_wifi else "bt"

        async def iap_handler():
            peer = _IapPeer(cid)
            self._peers.add(peer)
            conn = None
            try:
                cert = await self.loop.run_in_executor(None, read_certificate)
                conn = IAP2Connection(writer, reader, self.loop, max_outgoing=4, zero_ack=direct,
                                      control_version=2, tag=transport)
                conn.start(initiate_negotiate=direct)
                if direct:
                    self._log("iAP2 link started (negotiate initiated, %s)"
                              % transport)
                stream = conn.control_session

                ft = FileTransferReceiver(
                    send=conn.send_file_transfer,
                    on_file=lambda ftid, data: self._push_album_art(peer, data),
                    log=self._log)
                conn.on_file_transfer = ft.feed

                if carkit:
                    await self._handle_identification(stream, carkit=carkit)
                    await self._handle_auth(stream, cert)
                    await write_csm(stream, PowerSourceUpdate(
                        available_current_for_device=Uint16(AVAILABLE_CURRENT_MA),
                        device_battery_should_charge_if_power_is_present=True))
                    self._log("power source: advertising %dmA to device (usb)" % AVAILABLE_CURRENT_MA)
                    if CARPLAY_WIRED_START_SESSION:
                        self._log("AV: identification done, will start via CarPlayStartSession on availability")
                    else:
                        self._log("AV: identification done, watching for phone _carplay-ctrl (av-watch)")
                        self.loop.run_in_executor(None, self._av_watch)
                else:
                    await self._handle_identification(stream, carkit=carkit)
                    await self._handle_auth(stream, cert)

                await write_csm(stream, StartNowPlayingUpdates(
                    media_item_attributes=StartMediaItemAttributes(
                        title=True, artist=True, album=True, duration_ms=True,
                        artwork=True),
                    playback_attributes=StartPlaybackAttributes(
                        status=True, elapsed_ms=True, app_name=True),
                ))
                self._log("nowplaying: subscribed (%s)" % transport)

                await write_csm(stream, StartRouteGuidanceUpdates())
                self._log("navigation: subscribed (%s)" % transport)

                await write_csm(stream, StartPowerUpdates(
                    battery_charge_level=True,
                    battery_charging_state=True,
                    is_external_charger_connected=True,
                ))
                self._log("power: subscribed (%s)" % transport)

                await write_csm(stream, StartCommunicationsUpdates(
                    signal_strength=True,
                    carrier_name=True,
                    cellular_supported=True,
                ))
                self._log("cellular: subscribed (%s)" % transport)

                await write_csm(stream, StartCallStateUpdates(
                    remote_id=True,
                    display_name=True,
                    status=True,
                    direction=True,
                    call_uuid=True,
                    disconnect_reason=True,
                ))
                self._log("call state: subscribed (%s)" % transport)

                peer.stream = stream

                while True:
                    incoming = await read_csm(stream)
                    if isinstance(incoming, RequestAccessoryWiFiConfigurationInformation):
                        self._log("phone requested Wi-Fi config, replying with AP credentials")
                        info = AccessoryWiFiConfigurationInformation(
                            ssid=SSID,
                            passphrase=PASSPHRASE,
                            security_type=SECURITY_TYPE_MAP.get(
                                SECURITY_TYPE.upper(), SecurityType.WPA_WPA2),
                            channel=Uint8(CHANNEL),
                        )
                        await write_csm(stream, info)
                    elif isinstance(incoming, WirelessCarPlayUpdate):
                        self._log("wireless CarPlay status:", getattr(incoming, "status", None))
                    elif isinstance(incoming, CarPlayAvailability):
                        t = self.loop.create_task(
                            self._send_carplay_start_session(stream, incoming, usb_serial))
                        self._tasks.add(t)
                        t.add_done_callback(self._tasks.discard)
                    elif isinstance(incoming, DeviceTransportIdentifierNotification):
                        peer.phone_id = incoming.bluetooth_transport_id or ""
                        peer.usb_transport_id = incoming.usb_transport_id or ""
                        self._log("device transport identifier: bt=%r usb=%r"
                                  % (incoming.bluetooth_transport_id, incoming.usb_transport_id))
                        if carkit and usb_serial and peer.phone_id:
                            livi_sock.push({"type": "device", "src": "carkit",
                                            "btMac": peer.phone_id, "usbUdid": usb_serial})
                    elif isinstance(incoming, NowPlayingUpdate):
                        self._push_now_playing(peer, incoming)
                    elif isinstance(incoming, PowerUpdate):
                        self._push_power(peer, incoming)
                    elif isinstance(incoming, CommunicationsUpdate):
                        self._push_comms(peer, incoming)
                    elif isinstance(incoming, CallStateUpdate):
                        self._push_call(peer, incoming)
                    elif isinstance(incoming, RouteGuidanceUpdate):
                        self._handle_route_guidance(peer, incoming)
                    elif isinstance(incoming, RouteGuidanceManeuverUpdate):
                        self._handle_route_maneuver(peer, incoming)
                    elif isinstance(incoming, StartLocationInformation):
                        self._handle_start_location(peer, incoming, transport)
                    elif isinstance(incoming, StopLocationInformation):
                        peer.location_types = set()
                        self._log("location: stopped (%s)" % transport)
                    elif isinstance(incoming, StartVehicleStatusUpdates):
                        peer.vehicle_status_active = True
                        self._push_vehicle_status_to(peer)
                        self._log("vehicle status: subscribed (%s)" % transport)
                    elif isinstance(incoming, StopVehicleStatusUpdates):
                        peer.vehicle_status_active = False
                        self._log("vehicle status: stopped (%s)" % transport)
            except Exception as e:
                self._log("session ended:", repr(e))
            finally:
                self._peers.discard(peer)
                if conn is not None:
                    conn.close()
                try:
                    writer.close()
                except Exception:
                    pass
                if carkit:
                    self._carkit_iap_count -= 1
                if not over_wifi and self._bt_writer is writer:
                    self._bt_writer = None

        task = self.loop.create_task(iap_handler())
        self._tasks.add(task)
        task.add_done_callback(self._tasks.discard)
        return task

    def register(self):
        self._start_bonjour()
        self._start_control_socket()
        self._start_wifi_presence()
        self._start_carkit_transport()

    def _start_wifi_presence(self):
        try:
            from iap2 import wifi_presence
            from shared.config import WIFI_IFACE
            from iap2.wifi_ap import DNSMASQ_LEASE_PATH
            wifi_presence.start(WIFI_IFACE, DNSMASQ_LEASE_PATH, self._on_wifi_presence, self._log)
        except Exception as e:
            self._log("wifi presence start failed:", repr(e))

    def _on_wifi_presence(self, kind, mac, ip):
        self._log("wifi %s mac=%s ip=%s" % (kind, mac, ip or "?"))
        livi_sock.push({"type": "wifi", "event": kind, "mac": mac, "ip": ip})
        # The phone rejoined the AP with a fresh _carplay-ctrl port; re-browse for it.
        if kind == "joined":
            carplay_bonjour.kick()

    def _register_bt_transport(self):
        bus = self.ctx.bus
        bluez = bus.get_object("org.bluez", "/org/bluez")
        profile_manager = dbus.Interface(bluez, "org.bluez.ProfileManager1")

        self._server = IAPProfile(bus, "/org/bluez/iap_server", self._on_connection, self.loop)
        self._client = IAPProfile(bus, "/org/bluez/iap_client", self._on_connection, self.loop)
        profile_manager.RegisterProfile(self._server, IAP_SERVER_UUID, {
            "Role": "server",
            "Channel": dbus.types.UInt16(IAP_CHANNEL),
            "ServiceRecord": IAP_RECORD,
            "RequireAuthentication": False,
            "RequireAuthorization": False,
        })
        profile_manager.RegisterProfile(self._client, IAP_CLIENT_UUID, {
            "Role": "client",
            "AutoConnect": True,
        })
        self._log("iAP2 Bluetooth profiles registered")

    def _remember_wireless_phone(self, mac):
        """Mark a wireless CarPlay iPhone trusted and record it so the reconnect worker
        can page it back after a restart."""
        try:
            path = "%s/dev_%s" % (self.ctx.adapter_path, mac.replace(":", "_").upper())
            dev = dbus.Interface(
                self.ctx.bus.get_object("org.bluez", path), "org.freedesktop.DBus.Properties")
            dev.Set("org.bluez.Device1", "Trusted", dbus.Boolean(True))
        except Exception as e:
            self._log("could not set iPhone trusted:", repr(e))
        livi_sock.push({"type": "device", "src": "bt", "btMac": mac})

    def _unregister_bt_transport(self):
        bus = self.ctx.bus
        profile_manager = dbus.Interface(
            bus.get_object("org.bluez", "/org/bluez"), "org.bluez.ProfileManager1")
        if self._server is not None:
            profile_manager.UnregisterProfile(self._server)
            self._server = None
        if self._client is not None:
            profile_manager.UnregisterProfile(self._client)
            self._client = None
        self._log("iAP2 Bluetooth profiles unregistered")

    # BlueZ calls run on the GLib main thread; the control socket runs on asyncio,
    # so marshal the runtime toggle onto the main loop.
    def set_cp_wireless(self, enabled):
        GLib.idle_add(self._toggle_bt_transport, bool(enabled))

    def _toggle_bt_transport(self, enabled):
        try:
            if enabled and self._server is None:
                self._register_bt_transport()
            elif not enabled and self._server is not None:
                self._unregister_bt_transport()
        except Exception as e:
            self._log("CP BT toggle failed:", repr(e))
        return False

    def _start_bonjour(self):
        try:
            carplay_bonjour.start_service(get_bt_mac())
        except Exception as e:
            self._log("bonjour start failed:", repr(e))

    def _start_control_socket(self):
        try:
            self._livi_sock_task = livi_sock.start(
                self.loop, lambda r, w, cid="": self._on_connection(r, w, over_wifi=True, cid=cid),
                BT_ADAPTER, self._log, on_drop_iap2=self.drop_bt_iap2,
                on_command=self.handle_command)
        except Exception as e:
            self._log("control socket failed to start:", repr(e))

    def _start_carkit_transport(self):
        try:
            import iap2.carkit as carkit
            self._carkit_task = carkit.start(
                self.loop,
                lambda r, w, serial="": self._on_connection(r, w, carkit=True, usb_serial=serial),
                self._log)
            self._log("carkit (wired CarPlay) watcher started")
        except Exception as e:
            self._log("carkit start failed:", repr(e))

    def _handle_start_location(self, peer, incoming, transport):
        types = set()
        if getattr(incoming, "gps_fix_data", None):
            types.add("GGA")
        if getattr(incoming, "recommended_minimum", None):
            types.add("RMC")
        if getattr(incoming, "satellites_in_view", None):
            types.add("GSV")
        peer.location_types = types
        self._log("location: subscribed %s (%s)" % (sorted(types), transport))

    def handle_command(self, cmd, arg):
        if cmd == "location":
            try:
                nmea = base64.b64decode(arg).decode("utf-8", "replace")
            except Exception:
                return True
            self._queue_location(nmea)
            return True
        if cmd == "vehicle-status":
            try:
                data = json.loads(arg)
            except Exception:
                return True
            self._vehicle_status.update(data)
            self._push_vehicle_status()
            return True
        if cmd == "set-aa":
            fn = getattr(self.ctx, "set_aa_wireless", None)
            if callable(fn):
                fn(arg.strip() in ("1", "true", "on"))
            return True
        if cmd == "set-cp":
            enabled = arg.strip() in ("1", "true", "on")
            fn = getattr(self.ctx, "set_cp_wireless", None)
            if callable(fn):
                fn(enabled)
            else:
                self.set_cp_wireless(enabled)
            return True
        if cmd == "reconnect-targets":
            try:
                targets = json.loads(arg) if arg.strip() else {}
            except Exception:
                targets = {}
            if not isinstance(targets, dict):
                targets = {}
            self._log("reconnect-targets:", targets)
            fn = getattr(self.ctx, "set_reconnect_targets", None)
            if callable(fn):
                fn(targets)
            return True
        return False

    def _queue_location(self, nmea):
        if not nmea:
            return
        for peer in self._peers:
            if peer.stream is None or not peer.location_types:
                continue
            task = self.loop.create_task(
                self._send_location(peer.stream, set(peer.location_types), nmea))
            self._tasks.add(task)
            task.add_done_callback(self._tasks.discard)

    def _push_vehicle_status(self):
        for peer in self._peers:
            self._push_vehicle_status_to(peer)

    def _push_vehicle_status_to(self, peer):
        if peer.stream is None or not peer.vehicle_status_active or not self._vehicle_status:
            return
        task = self.loop.create_task(
            self._send_vehicle_status(peer.stream, dict(self._vehicle_status)))
        self._tasks.add(task)
        task.add_done_callback(self._tasks.discard)

    async def _send_vehicle_status(self, stream, vs):
        try:
            self._log("vehicle status → range=%s temp=%s warn=%s" % (
                vs.get("range"), vs.get("outsideTemperature"), vs.get("rangeWarning")))
            await write_csm(stream, VehicleStatusUpdate(
                range=Uint16(vs["range"]) if "range" in vs else None,
                outside_temperature=Int16(vs["outsideTemperature"]) if "outsideTemperature" in vs else None,
                range_warning=vs.get("rangeWarning"),
            ))
        except Exception as e:
            self._log("vehicle status send failed:", repr(e))

    async def _send_location(self, stream, types, nmea):
        try:
            for line in nmea.replace("\r", "").split("\n"):
                line = line.strip()
                if not line.startswith("$") or len(line) < 6:
                    continue
                if line[3:6] not in types:
                    continue
                await write_csm(stream, LocationInformation(nmea_sentence=line))
        except Exception as e:
            self._log("location send failed:", repr(e))

    def teardown(self):
        try:
            self._unregister_bt_transport()
        except Exception as e:
            self._log("teardown error:", repr(e))
        self._carkit_link_local_cleanup()
        try:
            mfi_power_off()
        except Exception as e:
            self._log("mfi power-off error:", repr(e))

def create(ctx):
    return CpHandler(ctx)
