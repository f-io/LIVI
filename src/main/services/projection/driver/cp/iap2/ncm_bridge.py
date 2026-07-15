"""Userspace NCM host for iPhone NCM functions the kernel cdc_ncm rejects.
Claims the NCM pair via usbfs, activates the data alt setting and bridges
NTB16 frames to a TAP device that carries the CarPlay AV link."""

import errno
import fcntl
import os
import select
import struct
import subprocess
import threading
import time

from iap2.muxd import find_dev, usb_bulk, usb_claim, usb_ctrl

TUNSETIFF = 0x400454CA
IFF_TAP = 0x0002
IFF_NO_PI = 0x1000

NTH16_SIG = 0x484D434E
NDP16_SIG = 0x304D434E

_registry = {}
_reg_lock = threading.Lock()
_tap_seq = [0]


def av_iface(serial):
    with _reg_lock:
        return _registry.get(serial, "")


def _sys_iface_attr(path, name, default=""):
    try:
        return open(path + "/" + name).read().strip()
    except OSError:
        return default


def _kernel_ncm_bound(usb_path):
    root = os.path.realpath(usb_path)
    try:
        names = os.listdir("/sys/class/net")
    except OSError:
        return False
    for n in names:
        base = "/sys/class/net/%s/device" % n
        try:
            drv = os.path.basename(os.path.realpath(base + "/driver"))
        except OSError:
            continue
        if drv == "cdc_ncm" and os.path.realpath(base).startswith(root + "/"):
            return True
    return False


def _find_ncm_pair(usb_path):
    """First unbound NCM control/data interface pair of the active config."""
    root = os.path.realpath(usb_path)
    dev = os.path.basename(root)
    cfg = _sys_iface_attr(usb_path, "bConfigurationValue")
    for entry in sorted(os.listdir(root)):
        if not entry.startswith(dev + ":" + cfg + "."):
            continue
        ipath = root + "/" + entry
        if _sys_iface_attr(ipath, "bInterfaceClass") != "02":
            continue
        if _sys_iface_attr(ipath, "bInterfaceSubClass") != "0d":
            continue
        if os.path.islink(ipath + "/driver"):
            continue
        ctrl = int(_sys_iface_attr(ipath, "bInterfaceNumber"), 16)
        data_path = "%s/%s:%s.%d" % (root, dev, cfg, ctrl + 1)
        if _sys_iface_attr(data_path, "bInterfaceClass") != "0a":
            continue
        return ctrl, ctrl + 1
    return None


def _parse_host_mac(usb_path, fd, ctrl_if):
    """MAC the host must use, from the CDC Ethernet functional descriptor."""
    try:
        raw = open(usb_path + "/descriptors", "rb").read()
    except OSError:
        return ""
    idx = 0
    cur_if = -1
    mac_str_idx = 0
    while idx + 2 <= len(raw):
        blen, btype = raw[idx], raw[idx + 1]
        if blen < 2:
            break
        if btype == 0x04 and blen >= 3:
            cur_if = raw[idx + 2]
        elif btype == 0x24 and blen >= 4 and cur_if == ctrl_if and raw[idx + 2] == 0x0F:
            mac_str_idx = raw[idx + 3]
            break
        idx += blen
    if not mac_str_idx:
        return ""
    try:
        d = usb_ctrl(fd, 0x80, 6, (3 << 8) | mac_str_idx, 0x0409, 64)
    except OSError:
        return ""
    if len(d) < 2 or d[1] != 3:
        return ""
    s = d[2:d[0]].decode("utf-16-le", "ignore")
    if len(s) != 12:
        return ""
    return ":".join(s[i:i + 2] for i in range(0, 12, 2)).lower()


def _find_bulk_eps(usb_path, data_if):
    root = os.path.realpath(usb_path)
    dev = os.path.basename(root)
    cfg = _sys_iface_attr(usb_path, "bConfigurationValue")
    ipath = "%s/%s:%s.%d" % (root, dev, cfg, data_if)
    ep_in = ep_out = 0
    try:
        entries = os.listdir(ipath)
    except OSError:
        return 0, 0
    for entry in entries:
        if not entry.startswith("ep_"):
            continue
        epath = ipath + "/" + entry
        if _sys_iface_attr(epath, "type") != "Bulk":
            continue
        addr = int(_sys_iface_attr(epath, "bEndpointAddress", "0"), 16)
        if addr & 0x80:
            ep_in = addr
        else:
            ep_out = addr
    return ep_in, ep_out


class NcmBridge:
    def __init__(self, serial, log=lambda *a: None):
        self.serial = serial
        self.log = log
        self.fd = -1
        self.tap = -1
        self.ifname = ""
        self.run = False
        self.seq = 0
        self.out_max = 2048
        self._threads = []
        self._wlock = threading.Lock()

    def start(self):
        dev = find_dev(self.serial)
        if not dev:
            return ""
        bus, num, usb_path = dev
        usb_path = usb_path.rstrip("/")
        if _kernel_ncm_bound(usb_path):
            return ""
        pair = _find_ncm_pair(usb_path)
        if pair is None:
            self.log("[ncm] no unbound NCM pair on %s" % self.serial[:8])
            return ""
        ctrl_if, data_if = pair
        self.fd = os.open("/dev/bus/usb/%03d/%03d" % (bus, num), os.O_RDWR)
        try:
            return self._start_claimed(usb_path, ctrl_if, data_if)
        except Exception:
            self.stop()
            raise

    def _start_claimed(self, usb_path, ctrl_if, data_if):
        usb_claim(self.fd, ctrl_if)
        usb_claim(self.fd, data_if)
        params = usb_ctrl(self.fd, 0xA1, 0x80, 0, ctrl_if, 28)
        if len(params) >= 28:
            self.out_max = min(struct.unpack("<I", params[16:20])[0], 32764)
        mac = _parse_host_mac(usb_path, self.fd, ctrl_if)
        fcntl.ioctl(self.fd, (2 << 30) | (8 << 16) | (ord("U") << 8) | 4,
                    struct.pack("II", data_if, 1))
        ep_in, ep_out = _find_bulk_eps(usb_path, data_if)
        if not ep_in or not ep_out:
            self.stop()
            raise RuntimeError("NCM bulk endpoints not found")
        self.ep_in, self.ep_out = ep_in, ep_out

        with _reg_lock:
            self.ifname = "cpusb%d" % _tap_seq[0]
            _tap_seq[0] += 1
        self.tap = os.open("/dev/net/tun", os.O_RDWR)
        ifr = struct.pack("16sH", self.ifname.encode(), IFF_TAP | IFF_NO_PI)
        fcntl.ioctl(self.tap, TUNSETIFF, ifr.ljust(40, b"\0"))
        if mac:
            subprocess.run(["ip", "link", "set", self.ifname, "address", mac],
                           capture_output=True, timeout=5)
        subprocess.run(["nmcli", "device", "set", self.ifname, "managed", "no"],
                       capture_output=True, timeout=10)
        subprocess.run(["ip", "link", "set", self.ifname, "up"],
                       capture_output=True, timeout=5)

        self.run = True
        for fn in (self._usb_to_tap, self._tap_to_usb):
            t = threading.Thread(target=fn, daemon=True,
                                 name="ncm-%s-%s" % (fn.__name__[1:], self.serial[:8]))
            t.start()
            self._threads.append(t)
        with _reg_lock:
            _registry[self.serial] = self.ifname
        self.log("[ncm] userspace NCM up: if%d/%d ep=0x%02x/0x%02x mac=%s tap=%s"
                 % (ctrl_if, data_if, ep_in, ep_out, mac or "?", self.ifname))
        return self.ifname

    def stop(self):
        self.run = False
        with _reg_lock:
            if _registry.get(self.serial) == self.ifname:
                _registry.pop(self.serial, None)
        for h in ("tap", "fd"):
            v = getattr(self, h)
            if v >= 0:
                try:
                    os.close(v)
                except OSError:
                    pass
                setattr(self, h, -1)
        for t in self._threads:
            t.join(timeout=3.0)
        self._threads = []

    def _usb_to_tap(self):
        while self.run:
            try:
                ntb = usb_bulk(self.fd, self.ep_in, length=32768, timeout=2000)
            except OSError as e:
                if not self.run:
                    return
                if e.errno == errno.ETIMEDOUT:
                    continue
                if e.errno in (errno.ENODEV, errno.EIO, errno.ESHUTDOWN, errno.EPROTO):
                    self.log("[ncm] usb read ended (%s)" % e.errno)
                    return
                time.sleep(0.05)
                continue
            if not ntb:
                continue
            for frame in self._parse_ntb(ntb):
                try:
                    os.write(self.tap, frame)
                except OSError:
                    if not self.run:
                        return

    def _parse_ntb(self, ntb):
        frames = []
        if len(ntb) < 12:
            return frames
        sig, hlen, _seq, blen, ndp_idx = struct.unpack("<IHHHH", ntb[:12])
        if sig != NTH16_SIG:
            return frames
        while ndp_idx and ndp_idx + 12 <= len(ntb):
            nsig, nlen, next_ndp = struct.unpack("<IHH", ntb[ndp_idx:ndp_idx + 8])
            if (nsig & 0x00FFFFFF) != (NDP16_SIG & 0x00FFFFFF):
                break
            off = ndp_idx + 8
            end = min(ndp_idx + nlen, len(ntb))
            while off + 4 <= end:
                d_idx, d_len = struct.unpack("<HH", ntb[off:off + 4])
                if not d_idx or not d_len:
                    break
                if d_idx + d_len <= len(ntb):
                    frames.append(ntb[d_idx:d_idx + d_len])
                off += 4
            ndp_idx = next_ndp
        return frames

    def _tap_to_usb(self):
        while self.run:
            try:
                r, _w, _x = select.select([self.tap], [], [], 1.0)
            except (OSError, ValueError):
                return
            if not r:
                continue
            try:
                frame = os.read(self.tap, 4096)
            except OSError:
                if not self.run:
                    return
                continue
            if not frame:
                continue
            ntb = self._build_ntb(frame)
            try:
                with self._wlock:
                    usb_bulk(self.fd, self.ep_out, data=ntb, timeout=3000)
            except OSError as e:
                if not self.run:
                    return
                if e.errno in (errno.ENODEV, errno.EIO, errno.ESHUTDOWN, errno.EPROTO):
                    self.log("[ncm] usb write ended (%s)" % e.errno)
                    return
                time.sleep(0.05)

    def _build_ntb(self, frame):
        self.seq = (self.seq + 1) & 0xFFFF
        d_idx = 28
        blen = d_idx + len(frame)
        nth = struct.pack("<IHHHH", NTH16_SIG, 12, self.seq, blen, 12)
        ndp = struct.pack("<IHHHHHH", NDP16_SIG, 16, 0, d_idx, len(frame), 0, 0)
        ntb = nth + ndp + frame
        if len(ntb) % 512 == 0:
            ntb += b"\0"
        return ntb
