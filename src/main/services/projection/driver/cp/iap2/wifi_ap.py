"""
wifi_ap.py — WiFi Access Point for wireless CarPlay.

The AP bring-up path is identical to AA (interface-scoped, firewalld trusted
zone, wait_for_ap_ready, teardown). Only the config source, the temp file paths
and get_bt_mac (needed by the iAP2 identification, off the AP critical path)
differ.
"""

import re
import subprocess
import shutil
import time

from shared.config import SSID, PASSPHRASE, CHANNEL, COUNTRY_CODE, WIFI_IFACE, BT_ADAPTER

HOSTAPD_CONFIG_PATH = "/tmp/livi-cp-hostapd.conf"
DNSMASQ_CONFIG_PATH = "/tmp/livi-cp-dnsmasq.conf"
DNSMASQ_LEASE_PATH = "/tmp/livi-cp-dnsmasq.leases"
AP_IP = "10.10.0.1"

# ── firewalld zone management ─────────────────────────────────────────────────
_saved_firewalld_zone = None


def _firewall_cmd(*args, timeout=5.0):
    if not shutil.which("firewall-cmd"):
        return (127, "", "firewall-cmd not installed")
    try:
        r = subprocess.run(
            ["sudo", "firewall-cmd", *args],
            capture_output=True, text=True, timeout=timeout,
        )
        return (r.returncode, r.stdout.strip(), r.stderr.strip())
    except (FileNotFoundError, subprocess.TimeoutExpired) as e:
        return (1, "", str(e))


def _firewalld_open_iface(iface):
    global _saved_firewalld_zone
    rc, prior, _ = _firewall_cmd("--get-zone-of-interface", iface)
    if rc == 127:
        return
    if rc != 0 or not prior:
        prior = ""
    _saved_firewalld_zone = prior or None
    rc, _, err = _firewall_cmd("--zone=trusted", f"--change-interface={iface}")
    if rc == 0:
        print(f"[cp-wifi] firewalld: {iface} -> trusted (runtime)")
    elif rc != 127:
        print(f"[cp-wifi] firewalld: could not move {iface} to trusted: {err}")


def _firewalld_restore_iface(iface):
    global _saved_firewalld_zone
    if _saved_firewalld_zone is None:
        return
    prior = _saved_firewalld_zone
    _saved_firewalld_zone = None
    rc, _, err = _firewall_cmd(f"--zone={prior}", f"--change-interface={iface}")
    if rc == 0:
        print(f"[cp-wifi] firewalld: {iface} trusted -> {prior} (restored)")
    elif rc != 127:
        print(f"[cp-wifi] firewalld: could not restore {iface} to {prior}: {err}")


def get_bt_mac():
    """Bluetooth adapter MAC for the iAP2 identification. /sys is the instant,
    canonical source and is tried first; btmgmt/hciconfig are fallbacks (btmgmt can
    hang for its full ~5s timeout here, and this runs twice during bringup)."""
    try:
        with open(f"/sys/class/bluetooth/{BT_ADAPTER}/address") as f:
            mac = f.read().strip()
            if mac:
                return mac
    except OSError:
        pass
    try:
        out = subprocess.check_output(["hciconfig", BT_ADAPTER], text=True,
                                      timeout=5, stderr=subprocess.DEVNULL)
        m = re.search(r"BD Address: ([0-9A-Fa-f:]{17})", out)
        if m:
            return m.group(1)
    except Exception:
        pass
    # btmgmt LAST: its mgmt socket can hang for the full timeout during bringup
    # (bluez busy with hostapd + profile registration), so keep it short.
    idx = BT_ADAPTER.replace("hci", "") or "0"
    for cmd in (["btmgmt", "--index", idx, "info"], ["btmgmt", "info"]):
        try:
            out = subprocess.check_output(cmd, text=True, timeout=2,
                                          stderr=subprocess.DEVNULL)
            m = re.search(r"addr ([0-9A-Fa-f:]{17})", out)
            if m:
                return m.group(1)
        except Exception:
            continue
    raise RuntimeError("Failed to get Bluetooth MAC address")


def get_security_config():
    return (
        "wpa=2\n"
        "wpa_key_mgmt=WPA-PSK\n"
        "rsn_pairwise=CCMP"
    )


def _channel_to_vht_centre(primary):
    if 36 <= primary <= 48:
        return 42
    if 149 <= primary <= 161:
        return 155
    return None


def _ht40_secondary(primary):
    return "+" if (primary // 4) % 2 == 1 else "-"


def write_hostapd_config():
    security_config = get_security_config()

    is_5ghz = CHANNEL >= 36
    if is_5ghz:
        ht40_dir = _ht40_secondary(CHANNEL)
        vht_centre = _channel_to_vht_centre(CHANNEL)
        radio = (
            "hw_mode=a\n"
            f"channel={CHANNEL}\n"
            "ieee80211n=1\n"
            "ieee80211ac=1\n"
            f"ht_capab=[HT40{ht40_dir}]\n"
        )
        if vht_centre is not None:
            radio += (
                "vht_capab=[SHORT-GI-80]\n"
                "vht_oper_chwidth=1\n"
                f"vht_oper_centr_freq_seg0_idx={vht_centre}\n"
            )
    else:
        radio = (
            "hw_mode=g\n"
            f"channel={CHANNEL}\n"
            "ieee80211n=1\n"
        )

    config = f"""
interface={WIFI_IFACE}
driver=nl80211
ctrl_interface=/var/run/hostapd
ssid={SSID}
country_code={COUNTRY_CODE}
ieee80211d=1
ieee80211h=0
{radio}ignore_broadcast_ssid=0
wmm_enabled=1
{security_config}
wpa_passphrase={PASSPHRASE}
"""
    with open(HOSTAPD_CONFIG_PATH, "w") as f:
        f.write(config)


def setup_network_interface():
    subprocess.run(["sudo", "iw", "reg", "set", COUNTRY_CODE], check=False)
    subprocess.run(["sudo", "ip", "link", "set", WIFI_IFACE, "up"], check=True)
    subprocess.run(["sudo", "ip", "addr", "flush", "dev", WIFI_IFACE], check=True)
    subprocess.run(["sudo", "ip", "addr", "add", f"{AP_IP}/24", "dev", WIFI_IFACE], check=True)


def disable_existing_wifi_network_services():
    subprocess.run(["sudo", "systemctl", "stop", f"wpa_supplicant@{WIFI_IFACE}"], check=False)
    subprocess.run(["sudo", "systemctl", "disable", f"wpa_supplicant@{WIFI_IFACE}"], check=False)
    subprocess.run(["sudo", "ip", "addr", "flush", "dev", WIFI_IFACE], check=False)
    subprocess.run(["sudo", "ip", "link", "set", WIFI_IFACE, "up"], check=False)
    time.sleep(1)


def kill_network_manager_and_supplicant():
    # Only release WIFI_IFACE — leave NetworkManager running for every other
    # interface (including the uplink Wi-Fi card).
    subprocess.run(["sudo", "nmcli", "device", "set", WIFI_IFACE, "managed", "no"], check=False)
    subprocess.run(["sudo", "nmcli", "device", "disconnect", WIFI_IFACE],
                   check=False, stderr=subprocess.DEVNULL)
    subprocess.run(["sudo", "systemctl", "stop", f"wpa_supplicant@{WIFI_IFACE}"], check=False)
    time.sleep(1)


def start_dnsmasq():
    dnsmasq_config = f"""
interface={WIFI_IFACE}
bind-interfaces
dhcp-range=10.10.0.10,10.10.0.50,255.255.255.0,12h
dhcp-leasefile={DNSMASQ_LEASE_PATH}
domain-needed
bogus-priv
""".strip()

    with open(DNSMASQ_CONFIG_PATH, "w") as f:
        f.write(dnsmasq_config)

    # Kill every dnsmasq (including the system resolver on :53) before starting
    # ours, exactly like AA. Otherwise the system dnsmasq holds :53 and ours
    # fails with "Address already in use".
    subprocess.run(["sudo", "pkill", "dnsmasq"], check=False)
    subprocess.Popen(["sudo", "dnsmasq", "--conf-file=" + DNSMASQ_CONFIG_PATH])


def start_hostapd():
    subprocess.Popen(["sudo", "hostapd", HOSTAPD_CONFIG_PATH])


def _is_dhcp_listening():
    try:
        with open("/proc/net/udp") as f:
            for line in f:
                if "local_address" in line:
                    continue
                parts = line.split()
                if len(parts) >= 2 and parts[1].endswith(":0043"):
                    return True
    except OSError:
        pass
    return False


def _hostapd_state():
    try:
        out = subprocess.check_output(
            ["sudo", "hostapd_cli", "-p", "/var/run/hostapd", "-i", WIFI_IFACE, "status"],
            text=True, stderr=subprocess.DEVNULL, timeout=2,
        )
    except (subprocess.CalledProcessError, FileNotFoundError, subprocess.TimeoutExpired):
        return ""
    for line in out.splitlines():
        if line.startswith("state="):
            return line.split("=", 1)[1].strip()
    return ""


def wait_for_ap_ready(timeout_seconds=20.0):
    deadline = time.monotonic() + timeout_seconds
    hostapd_ready = False
    dhcp_ready = False
    while time.monotonic() < deadline:
        if not hostapd_ready and _hostapd_state() == "ENABLED":
            hostapd_ready = True
        if hostapd_ready and not dhcp_ready and _is_dhcp_listening():
            dhcp_ready = True
        if hostapd_ready and dhcp_ready:
            return True
        time.sleep(0.2)
    print(f"[cp-wifi] AP readiness timeout: hostapd={hostapd_ready} dhcp={dhcp_ready}", flush=True)
    return False


def setup_ap():
    subprocess.run(["sudo", "pkill", "-f", f"hostapd.*{HOSTAPD_CONFIG_PATH}"], check=False)
    subprocess.run(["sudo", "pkill", "-f", f"dnsmasq.*{DNSMASQ_CONFIG_PATH}"], check=False)
    time.sleep(0.3)

    kill_network_manager_and_supplicant()
    disable_existing_wifi_network_services()
    setup_network_interface()
    _firewalld_open_iface(WIFI_IFACE)

    write_hostapd_config()
    start_dnsmasq()
    start_hostapd()

    ready = wait_for_ap_ready()
    if ready:
        print(f"[cp-wifi] AP up — SSID={SSID!r}  IP={AP_IP}  channel={CHANNEL}")
    return ready


def teardown_ap():
    subprocess.run(["sudo", "pkill", "-f", f"hostapd.*{HOSTAPD_CONFIG_PATH}"], check=False)
    subprocess.run(["sudo", "pkill", "-f", f"dnsmasq.*{DNSMASQ_CONFIG_PATH}"], check=False)
    subprocess.run(["sudo", "ip", "addr", "flush", "dev", WIFI_IFACE], check=False)
    _firewalld_restore_iface(WIFI_IFACE)
    subprocess.run(["sudo", "nmcli", "device", "set", WIFI_IFACE, "managed", "yes"], check=False)
    print("[cp-wifi] AP down")
