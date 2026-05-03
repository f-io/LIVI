"""
wifi_ap.py — WiFi Access Point for Android Auto
"""

import re
import os
import shutil
import subprocess
import time

from config import SSID, PASSPHRASE, CHANNEL, COUNTRY_CODE, WIFI_IFACE

HOSTAPD_CONFIG_PATH = "/tmp/livi-hostapd.conf"
DNSMASQ_CONFIG_PATH = "/tmp/livi-dnsmasq.conf"
AP_IP = "10.10.0.1"

# ── firewalld zone management ─────────────────────────────────────────────────
#
# Some distros ship firewalld active by default. When that's the case, the
# AP interface lands in a default zone (typically one that drops inbound
# broadcasts), which silently swallows the phone's DHCP DISCOVER even though
# dnsmasq is happily listening. Symptom: the phone associates and completes
# EAPOL but never gets an IP, then deauths after the inactivity timer.
#
# Fix: for the lifetime of the AP, move WIFI_IFACE into the `trusted` zone
# (runtime-only, no `--permanent`) so all inbound traffic on that interface
# is allowed; on teardown restore the zone we displaced.

_saved_firewalld_zone: str | None = None


def _firewall_cmd(*args: str, timeout: float = 5.0) -> tuple[int, str, str]:
    """Run firewall-cmd. Returns (rc, stdout, stderr). rc=127 if not installed."""
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


def _firewalld_open_iface(iface: str) -> None:
    """Move iface into firewalld's `trusted` zone (runtime). Saves prior zone."""
    global _saved_firewalld_zone
    rc, prior, _ = _firewall_cmd("--get-zone-of-interface", iface)
    if rc == 127:
        return  # firewall-cmd not installed, no firewalld to manage
    if rc != 0 or not prior:
        prior = ""
    _saved_firewalld_zone = prior or None

    rc, _, err = _firewall_cmd("--zone=trusted", f"--change-interface={iface}")
    if rc == 0:
        if prior:
            print(f"[wifi_ap] firewalld: {iface} {prior} → trusted (runtime)")
        else:
            print(f"[wifi_ap] firewalld: {iface} → trusted (runtime)")
    elif rc != 127:
        print(f"[wifi_ap] firewalld: could not move {iface} to trusted: {err}")


def _firewalld_restore_iface(iface: str) -> None:
    """Restore the firewalld zone we displaced in _firewalld_open_iface."""
    global _saved_firewalld_zone
    if _saved_firewalld_zone is None:
        return
    prior = _saved_firewalld_zone
    _saved_firewalld_zone = None
    rc, _, err = _firewall_cmd(f"--zone={prior}", f"--change-interface={iface}")
    if rc == 0:
        print(f"[wifi_ap] firewalld: {iface} trusted → {prior} (restored)")
    elif rc != 127:
        print(f"[wifi_ap] firewalld: could not restore {iface} to {prior}: {err}")


def get_wlan_mac(iface: str = WIFI_IFACE) -> str:
    try:
        with open(f"/sys/class/net/{iface}/address") as f:
            return f.read().strip().upper()
    except OSError:
        out = subprocess.check_output(["btmgmt", "info"], text=True)
        m = re.search(r"addr ([0-9A-Fa-f:]{17})", out)
        if m:
            return m.group(1).upper()
    raise RuntimeError(f"Cannot read MAC for {iface}")


def get_security_config() -> str:
    return (
        "wpa=2\n"
        "wpa_key_mgmt=WPA-PSK\n"
        "rsn_pairwise=CCMP"
    )


def _channel_to_vht_centre(primary: int) -> int | None:
    """Centre frequency segment for 80 MHz VHT, given the primary 20 MHz
    channel. Returns None if the primary isn't part of an 80 MHz block we
    can use in AP mode without DFS.

    Non-DFS UNII-1 (36/40/44/48) → centre 42.
    UNII-3 (149/153/157/161)     → centre 155.
    Anything else (DFS UNII-2)   → caller must drop to HT40 / HT20.
    """
    if 36 <= primary <= 48:
        return 42
    if 149 <= primary <= 161:
        return 155
    return None


def _ht40_secondary(primary: int) -> str:
    """HT40 secondary channel position for the given 20 MHz primary.
    5 GHz HT40 pairs adjacent channels (4 apart). The lower of each pair
    uses HT40+ (secondary above), the upper uses HT40−:
        36+ / 40−   44+ / 48−   149+ / 153−   157+ / 161−
    Pattern: (channel // 4) odd → lower of pair → '+', even → upper → '−'."""
    return "+" if (primary // 4) % 2 == 1 else "-"


def write_hostapd_config():
    security_config = get_security_config()

    is_5ghz = CHANNEL >= 36
    if is_5ghz:
        # 11ac VHT80 on 5 GHz UNII-1/UNII-3 (non-DFS). Bare minimum config:
        # channel + bonding width directives
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
                "vht_oper_chwidth=1\n"  # 1 = 80 MHz
                f"vht_oper_centr_freq_seg0_idx={vht_centre}\n"
            )
    else:
        # 2.4 GHz: 11n HT20 — 40 MHz channels overlap on 2.4, not worth it.
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
    subprocess.run(["sudo", "dhcpcd", "-k", WIFI_IFACE], check=False)
    subprocess.run(["sudo", "ip", "link", "set", WIFI_IFACE, "up"], check=False)
    time.sleep(1)


def kill_network_manager_and_supplicant():
    # Only release {WIFI_IFACE} — leave NM running for other interfaces
    subprocess.run(["sudo", "nmcli", "device", "set", WIFI_IFACE, "managed", "no"], check=False)
    subprocess.run(["sudo", "nmcli", "device", "disconnect", WIFI_IFACE], check=False)
    subprocess.run(["sudo", "systemctl", "stop", f"wpa_supplicant@{WIFI_IFACE}"], check=False)
    subprocess.run(["sudo", "rfkill", "unblock", "wifi"], check=False)
    time.sleep(1)


def start_dnsmasq():
    dnsmasq_config = f"""
interface={WIFI_IFACE}
bind-interfaces
dhcp-range=10.10.0.10,10.10.0.50,255.255.255.0,12h
domain-needed
bogus-priv
""".strip()

    with open(DNSMASQ_CONFIG_PATH, "w") as f:
        f.write(dnsmasq_config)

    subprocess.run(["sudo", "pkill", "dnsmasq"], check=False)
    subprocess.Popen(["sudo", "dnsmasq", "--conf-file=" + DNSMASQ_CONFIG_PATH])


def start_hostapd():
    subprocess.Popen(["sudo", "hostapd", HOSTAPD_CONFIG_PATH])


def _is_dhcp_listening() -> bool:
    """True if any process is bound to UDP 67. Reads /proc/net/udp; the
    local_address column is hex IP:PORT (little-endian), 0x0043 == 67."""
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


def _hostapd_state() -> str:
    """Query hostapd's actual state via ctrl-interface. Returns the `state=`
    value (DISABLED / COUNTRY_UPDATE / HT_SCAN / ENABLED / …) or "" on error."""
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


def wait_for_ap_ready(timeout_seconds: float = 20.0) -> bool:
    """Block until hostapd reports state=ENABLED AND dnsmasq is bound to :67."""
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
    print(f"[wifi_ap] AP readiness timeout: hostapd={hostapd_ready} dhcp={dhcp_ready}",
          flush=True)
    return False


def setup_ap() -> bool:
    """Bring up the AP. Returns True iff the AP is fully serving (hostapd
    beaconing AND dnsmasq bound). Caller gates BT advertising on this."""
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
        print(f"[wifi_ap] AP up — SSID={SSID!r}  IP={AP_IP}  channel={CHANNEL}")
    return ready


def teardown_ap():
    subprocess.run(["sudo", "pkill", "-f", f"hostapd.*{HOSTAPD_CONFIG_PATH}"], check=False)
    subprocess.run(["sudo", "pkill", "-f", f"dnsmasq.*{DNSMASQ_CONFIG_PATH}"], check=False)
    subprocess.run(["sudo", "ip", "addr", "flush", "dev", WIFI_IFACE], check=False)

    # Restore the original firewalld zone
    _firewalld_restore_iface(WIFI_IFACE)

    # Hand WIFI_IFACE back to NetworkManager
    subprocess.run(["sudo", "nmcli", "device", "set", WIFI_IFACE, "managed", "yes"], check=False)
    print("[wifi_ap] AP down")
