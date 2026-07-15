import ctypes
import errno
import fcntl
import glob
import os
import plistlib
import queue
import socket
import struct
import subprocess
import threading
import time
import uuid

VID = "05ac"
MUX_MAGIC = 0xFEEDFACE
P_VERSION, P_SETUP, P_TCP = 0, 2, 6
TH_FIN, TH_SYN, TH_RST, TH_PSH, TH_ACK = 0x01, 0x02, 0x04, 0x08, 0x10
EP_OUT, EP_IN = 0x04, 0x85
TX_WIN = 131072
MAX_PAYLOAD = 16384
LOCKDOWN_STORE = "/var/lib/lockdown"
CP_CONFIG = "6"


def find_dev(serial=""):
    for d in sorted(glob.glob("/sys/bus/usb/devices/*/")):
        try:
            if open(d + "idVendor").read().strip() != VID:
                continue
            if serial and open(d + "serial").read().strip() != serial:
                continue
            return (int(open(d + "busnum").read()), int(open(d + "devnum").read()), d)
        except OSError:
            continue
    return None


def dev_serial(path):
    try:
        return open(path + "serial").read().strip()
    except OSError:
        return ""


def num_configs(path):
    try:
        return int(open(path + "bNumConfigurations").read().strip())
    except OSError:
        return 0


def list_iphones():
    out = []
    for d in sorted(glob.glob("/sys/bus/usb/devices/*/")):
        try:
            if open(d + "idVendor").read().strip() != VID:
                continue
            s = open(d + "serial").read().strip()
            if s:
                out.append(s)
        except OSError:
            continue
    return out


class _CT(ctypes.Structure):
    _fields_ = [("brt", ctypes.c_uint8), ("br", ctypes.c_uint8), ("wv", ctypes.c_uint16),
                ("wi", ctypes.c_uint16), ("wl", ctypes.c_uint16), ("to", ctypes.c_uint32),
                ("data", ctypes.c_void_p)]


class _BULK(ctypes.Structure):
    _fields_ = [("ep", ctypes.c_uint), ("len", ctypes.c_uint), ("to", ctypes.c_uint),
                ("data", ctypes.c_void_p)]


def _op(size, nr, dirw=3):
    return (dirw << 30) | (size << 16) | (ord("U") << 8) | nr


def usb_ctrl(fd, brt, br, wv, wi, wl):
    buf = (ctypes.c_uint8 * max(wl, 1))()
    ct = _CT(brt, br, wv, wi, wl, 3000, ctypes.cast(buf, ctypes.c_void_p))
    fcntl.ioctl(fd, _op(ctypes.sizeof(_CT), 0), ct)
    return bytes(buf)[:wl]


def usb_claim(fd, iface):
    fcntl.ioctl(fd, (2 << 30) | (4 << 16) | (ord("U") << 8) | 15, ctypes.c_uint(iface))


def usb_bulk(fd, ep, data=b"", length=0, timeout=2000):
    if ep & 0x80:
        buf = (ctypes.c_uint8 * length)()
        b = _BULK(ep, length, timeout, ctypes.cast(buf, ctypes.c_void_p))
        n = fcntl.ioctl(fd, _op(ctypes.sizeof(_BULK), 2), b)
        return bytes(buf)[:n]
    buf = (ctypes.c_uint8 * len(data)).from_buffer_copy(data)
    b = _BULK(ep, len(data), timeout, ctypes.cast(buf, ctypes.c_void_p))
    fcntl.ioctl(fd, _op(ctypes.sizeof(_BULK), 2), b)


class MuxTcpConn:
    def __init__(self, host, sport, dport):
        self.host = host
        self.sport = sport
        self.dport = dport
        self.tx_seq = 0
        self.tx_ack = 0
        self.connected = threading.Event()
        self.rq = queue.Queue()
        self.closed = False

    def _tcp(self, flags, payload=b""):
        th = struct.pack(">HHIIBBHHH", self.sport, self.dport,
                         self.tx_seq & 0xffffffff, self.tx_ack & 0xffffffff,
                         0x50, flags, TX_WIN >> 8, 0, 0)
        self.host.mux_send(P_TCP, th + payload)

    def on_packet(self, flags, seq, ack, win, payload):
        if flags & TH_SYN and flags & TH_ACK:
            self.tx_seq += 1
            self.tx_ack = seq + 1
            self._tcp(TH_ACK)
            self.connected.set()
            return
        if flags & TH_RST:
            print("[muxd] sport=%d dport=%d RST from device (win=%d)" % (self.sport, self.dport, win), flush=True)
            self.closed = True
            self.rq.put(b"")
            self.connected.set()
            return
        if payload:
            self.tx_ack += len(payload)
            self.rq.put(payload)
            self._tcp(TH_ACK)
        if flags & TH_FIN:
            print("[muxd] sport=%d dport=%d FIN from device" % (self.sport, self.dport), flush=True)
            self.tx_ack += 1
            self._tcp(TH_ACK)
            self.closed = True
            self.rq.put(b"")

    def send(self, data):
        i = 0
        while i < len(data):
            chunk = data[i:i + MAX_PAYLOAD]
            self._tcp(TH_ACK, chunk)
            self.tx_seq += len(chunk)
            i += len(chunk)

    def recv(self, timeout=None):
        return self.rq.get(timeout=timeout)

    def close(self):
        if not self.closed:
            try:
                self._tcp(TH_FIN | TH_ACK)
            except Exception:
                pass
        self.closed = True
        self.rq.put(b"")


class MuxHost:
    def __init__(self, serial=""):
        self.serial = serial
        self.fd = -1
        self.wlock = threading.Lock()
        self.mux_tx = 0
        self.mux_rx = 0
        self.conns = {}
        self.next_sport = 1
        self.sportlock = threading.Lock()
        self.rxbuf = b""
        self.run = False
        self._rt = None

    def open(self):
        dev = find_dev(self.serial)
        node = "/dev/bus/usb/%03d/%03d" % (dev[0], dev[1])
        self.fd = os.open(node, os.O_RDWR)
        last = None
        for _ in range(12):
            try:
                usb_claim(self.fd, 1)
                last = None
                break
            except OSError as e:
                last = e
                time.sleep(0.3)
        if last is not None:
            raise last
        with self.wlock:
            usb_bulk(self.fd, EP_OUT, struct.pack(">II", P_VERSION, 8 + 12) + struct.pack(">III", 2, 0, 0))
        self._in(2000)
        self.mux_send(P_SETUP, b"\x07")
        self.run = True
        self._rt = threading.Thread(target=self._reader, daemon=True)
        self._rt.start()

    def close(self):
        self.run = False
        if self._rt:
            self._rt.join(timeout=2.0)
        with self.wlock:
            try:
                if self.fd >= 0:
                    os.close(self.fd)
            except OSError:
                pass
            self.fd = -1

    def mux_send(self, proto, payload):
        with self.wlock:
            if self.fd < 0:
                return
            hdr = struct.pack(">IIIHH", proto, 16 + len(payload), MUX_MAGIC,
                              self.mux_tx & 0xffff, self.mux_rx & 0xffff)
            self.mux_tx += 1
            usb_bulk(self.fd, EP_OUT, hdr + payload)

    def _in(self, timeout):
        return usb_bulk(self.fd, EP_IN, length=65536, timeout=timeout)

    def _reader(self):
        while self.run:
            try:
                data = self._in(1000)
            except OSError as e:
                if e.errno == errno.ETIMEDOUT:
                    continue
                if self.run:
                    print("[muxd] usb reader ended: %r" % e, flush=True)
                break
            if not data:
                continue
            self.rxbuf += data
            while len(self.rxbuf) >= 8:
                proto, length = struct.unpack(">II", self.rxbuf[:8])
                if length < 8 or len(self.rxbuf) < length:
                    break
                pkt = self.rxbuf[:length]
                self.rxbuf = self.rxbuf[length:]
                if length >= 16:
                    self.mux_rx = struct.unpack(">H", pkt[12:14])[0]
                if proto == P_TCP and length >= 36:
                    _, dp, seq, ack, off, flags, win, _, _ = struct.unpack(">HHIIBBHHH", pkt[16:36])
                    conn = self.conns.get(dp)
                    if conn:
                        conn.on_packet(flags, seq, ack, win, pkt[36:length])

    def connect(self, dport):
        with self.sportlock:
            sport = self.next_sport
            self.next_sport += 1
        conn = MuxTcpConn(self, sport, dport)
        self.conns[sport] = conn
        conn._tcp(TH_SYN)
        if not conn.connected.wait(5.0) or conn.closed:
            self.conns.pop(sport, None)
            raise ConnectionError("mux connect to port %d failed" % dport)
        return conn


def read_buid():
    p = os.path.join(LOCKDOWN_STORE, "SystemConfiguration.plist")
    try:
        with open(p, "rb") as f:
            return plistlib.load(f).get("SystemBUID") or str(uuid.uuid4()).upper()
    except Exception:
        return str(uuid.uuid4()).upper()


def read_pair_record(udid):
    for name in (udid, udid.upper(), udid.lower()):
        try:
            with open(os.path.join(LOCKDOWN_STORE, name + ".plist"), "rb") as f:
                return f.read()
        except OSError:
            continue
    return None


def save_pair_record(udid, data):
    try:
        os.makedirs(LOCKDOWN_STORE, exist_ok=True)
        with open(os.path.join(LOCKDOWN_STORE, udid + ".plist"), "wb") as f:
            f.write(data)
    except OSError:
        pass


class UsbmuxdServer:
    def __init__(self, host, sock_path):
        self.host = host
        self.sock_path = sock_path
        self.srv = None

    def start(self):
        try:
            os.unlink(self.sock_path)
        except OSError:
            pass
        self.srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.srv.bind(self.sock_path)
        os.chmod(self.sock_path, 0o777)
        self.srv.listen(16)
        threading.Thread(target=self._accept, daemon=True).start()

    def stop(self):
        try:
            if self.srv:
                self.srv.close()
        except OSError:
            pass
        try:
            os.unlink(self.sock_path)
        except OSError:
            pass

    def _accept(self):
        while True:
            try:
                c, _ = self.srv.accept()
            except OSError:
                return
            threading.Thread(target=self._client, args=(c,), daemon=True).start()

    def _recvn(self, c, n):
        buf = b""
        while len(buf) < n:
            chunk = c.recv(n - len(buf))
            if not chunk:
                return None
            buf += chunk
        return buf

    def _recv_packet(self, c):
        hdr = self._recvn(c, 4)
        if not hdr:
            return None
        size = struct.unpack("<I", hdr)[0]
        body = self._recvn(c, size - 4)
        if body is None:
            return None
        version, message, tag = struct.unpack("<III", body[:12])
        return version, message, tag, body[12:]

    def _reply(self, c, tag, obj):
        body = plistlib.dumps(obj)
        c.sendall(struct.pack("<IIII", 16 + len(body), 1, 8, tag) + body)

    def _device_entry(self):
        return {
            "DeviceID": 1,
            "MessageType": "Attached",
            "Properties": {
                "ConnectionType": "USB",
                "SerialNumber": self.host.serial,
                "DeviceID": 1,
                "LocationID": 0,
                "ProductID": 0x12a8,
            },
        }

    def _client(self, c):
        try:
            while True:
                pkt = self._recv_packet(c)
                if pkt is None:
                    return
                _, _, tag, data = pkt
                req = plistlib.loads(data)
                mt = req.get("MessageType")
                if mt == "ReadBUID":
                    self._reply(c, tag, {"BUID": read_buid()})
                elif mt == "ListDevices":
                    self._reply(c, tag, {"DeviceList": [self._device_entry()]})
                elif mt == "Listen":
                    self._reply(c, tag, {"MessageType": "Result", "Number": 0})
                elif mt == "ReadPairRecord":
                    rec = read_pair_record(req.get("PairRecordID", self.host.serial))
                    if rec is None:
                        self._reply(c, tag, {"MessageType": "Result", "Number": 2})
                    else:
                        self._reply(c, tag, {"PairRecordData": rec})
                elif mt == "SavePairRecord":
                    save_pair_record(req.get("PairRecordID", self.host.serial), req.get("PairRecordData"))
                    self._reply(c, tag, {"MessageType": "Result", "Number": 0})
                elif mt == "Connect":
                    self._do_connect(c, tag, req)
                    return
                else:
                    self._reply(c, tag, {"MessageType": "Result", "Number": 0})
        except (OSError, ConnectionError, EOFError):
            pass
        finally:
            try:
                c.close()
            except OSError:
                pass

    def _do_connect(self, c, tag, req):
        port = socket.ntohs(req.get("PortNumber", 0))
        try:
            conn = self.host.connect(port)
        except Exception:
            self._reply(c, tag, {"MessageType": "Result", "Number": 3})
            return
        self._reply(c, tag, {"MessageType": "Result", "Number": 0})
        self._relay(c, conn)

    def _relay(self, c, conn):
        def up():
            while not conn.closed:
                data = conn.recv()
                if not data:
                    print("[muxd] sport=%d relay: device side closed" % conn.sport, flush=True)
                    break
                try:
                    c.sendall(data)
                except OSError:
                    break
            try:
                c.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass

        threading.Thread(target=up, daemon=True).start()
        try:
            while True:
                data = c.recv(MAX_PAYLOAD)
                if not data:
                    print("[muxd] sport=%d relay: client(pymobiledevice3) side closed" % conn.sport, flush=True)
                    break
                conn.send(data)
        except OSError as e:
            print("[muxd] sport=%d relay: socket error %r" % (conn.sport, e), flush=True)
        conn.close()


class Muxd:
    def __init__(self, serial, log=lambda *a: None):
        self.serial = serial
        self.log = log
        self.host = None
        self.server = None
        self.ncm = None
        self.socket_path = "/tmp/livi-usbmux-%s.sock" % serial[:8]

    def start(self):
        self._stop_system_usbmuxd()
        self._config_carplay()
        self.host = MuxHost(self.serial)
        self.host.open()
        self.server = UsbmuxdServer(self.host, self.socket_path)
        self.server.start()
        self._start_ncm_bridge()
        self.log("[muxd] usbmux up for %s on config %s, socket %s" % (self.serial[:8], CP_CONFIG, self.socket_path))
        return self.socket_path

    def _start_ncm_bridge(self):
        try:
            from iap2 import ncm_bridge
            b = ncm_bridge.NcmBridge(self.serial, self.log)
            if b.start():
                self.ncm = b
        except Exception as e:
            self.log("[muxd] ncm bridge failed:", repr(e))

    def stop(self):
        try:
            if self.ncm:
                self.ncm.stop()
        except Exception:
            pass
        self.ncm = None
        try:
            if self.server:
                self.server.stop()
        except Exception:
            pass
        try:
            if self.host:
                self.host.close()
        except Exception:
            pass
        self._restore_config4()
        self._start_system_usbmuxd()
        self.log("[muxd] usbmux down for %s" % self.serial[:8])

    def _config_carplay(self):
        dev = find_dev(self.serial)
        if not dev:
            raise RuntimeError("iphone %s gone before config %s" % (self.serial[:8], CP_CONFIG))
        bus, num, path = dev
        if num_configs(path) < 6:
            fd = os.open("/dev/bus/usb/%03d/%03d" % (bus, num), os.O_RDWR)
            try:
                usb_ctrl(fd, 0xC0, 0x52, 0x0000, 0x0004, 1)
            finally:
                os.close(fd)
            for _ in range(25):
                time.sleep(0.2)
                d = find_dev(self.serial)
                if d and num_configs(d[2]) >= 6:
                    path = d[2]
                    break
            else:
                raise RuntimeError("iphone %s did not expose CarPlay configs" % self.serial[:8])
        if open(path + "bConfigurationValue").read().strip() != CP_CONFIG:
            open(path + "bConfigurationValue", "w").write(CP_CONFIG)
            time.sleep(2)

    def _restore_config4(self):
        dev = find_dev(self.serial)
        if dev:
            try:
                open(dev[2] + "bConfigurationValue", "w").write("4")
            except OSError:
                pass

    def _stop_system_usbmuxd(self):
        subprocess.run(["systemctl", "stop", "usbmuxd"], capture_output=True)
        subprocess.run(["pkill", "-x", "usbmuxd"], capture_output=True)
        time.sleep(1)

    def _start_system_usbmuxd(self):
        subprocess.run(["systemctl", "start", "usbmuxd"], capture_output=True)
