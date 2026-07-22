import os
import re
import shutil
import subprocess
import time

from .config import BT_ADAPTER, CHANNEL, COUNTRY_CODE, PASSPHRASE, SSID, WIFI_IFACE

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
            print(f"[wifi_ap] firewalld: {iface} {prior} -> trusted (runtime)")
        else:
            print(f"[wifi_ap] firewalld: {iface} -> trusted (runtime)")
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
        print(f"[wifi_ap] firewalld: {iface} trusted -> {prior} (restored)")
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


def get_bt_mac() -> str:
    """Bluetooth adapter MAC (needed by the CarPlay iAP2 identification). /sys is
    the instant, canonical source and is tried first; hciconfig then btmgmt are
    fallbacks (btmgmt's mgmt socket can hang for its full timeout during bringup)."""
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

    Non-DFS UNII-1 (36/40/44/48) -> centre 42.
    UNII-3 (149/153/157/161)     -> centre 155.
    Anything else (DFS UNII-2)   -> caller must drop to HT40 / HT20.
    """
    if 36 <= primary <= 48:
        return 42
    if 149 <= primary <= 161:
        return 155
    return None


def _ht40_secondary(primary: int) -> str:
    """HT40 secondary channel position for the given 20 MHz primary.
    5 GHz HT40 pairs adjacent channels (4 apart). The lower of each pair
    uses HT40+ (secondary above), the upper uses HT40-:
        36+ / 40-   44+ / 48-   149+ / 153-   157+ / 161-
    Pattern: (channel // 4) odd -> lower of pair -> '+', even -> upper -> '-'."""
    return "+" if (primary // 4) % 2 == 1 else "-"


def write_hostapd_config() -> None:
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
        # 2.4 GHz: 11n HT20
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


def setup_network_interface() -> None:
    subprocess.run(["sudo", "iw", "reg", "set", COUNTRY_CODE], check=False)
    subprocess.run(["sudo", "ip", "link", "set", WIFI_IFACE, "up"], check=True)
    subprocess.run(["sudo", "ip", "addr", "flush", "dev", WIFI_IFACE], check=True)
    subprocess.run(["sudo", "ip", "addr", "add", f"{AP_IP}/24", "dev", WIFI_IFACE], check=True)


def disable_existing_wifi_network_services() -> None:
    subprocess.run(["sudo", "systemctl", "stop", f"wpa_supplicant@{WIFI_IFACE}"], check=False)
    subprocess.run(["sudo", "ip", "addr", "flush", "dev", WIFI_IFACE], check=False)
    subprocess.run(["sudo", "ip", "link", "set", WIFI_IFACE, "up"], check=False)
    time.sleep(1)


RUN_NM_DIR = "/run/NetworkManager/system-connections"
ETC_NM_DIR = "/etc/NetworkManager/system-connections"


def persist_generated_wifi_profiles() -> None:
    """Write runtime-only Wi-Fi profiles to disk before the interface is taken.

    A profile rendered from netplan or cloud-init lives only under /run and is lost
    once the interface leaves NetworkManager. Whether anything ever recreates it is
    out of our hands. A copy in /etc belongs to NetworkManager and stays.
    """
    try:
        listing = subprocess.run(
            ["nmcli", "-t", "-f", "TYPE,FILENAME", "connection", "show"],
            capture_output=True, text=True, timeout=10).stdout
    except Exception as e:
        print(f"[wifi_ap] cannot list connections: {e!r}")
        return

    copied = []
    for line in listing.splitlines():
        conn_type, _, path = line.partition(":")
        if conn_type != "802-11-wireless" or not path.startswith(RUN_NM_DIR + "/"):
            continue
        dest = os.path.join(ETC_NM_DIR, os.path.basename(path))
        if os.path.exists(dest):
            continue
        r = subprocess.run(
            ["sudo", "install", "-m", "600", "-o", "root", "-g", "root", path, dest],
            capture_output=True, text=True, timeout=10)
        if r.returncode == 0:
            copied.append(os.path.basename(path))
        else:
            print(f"[wifi_ap] cannot persist {path}: {r.stderr.strip()}")

    if copied:
        print(f"[wifi_ap] persisted runtime Wi-Fi profiles: {', '.join(copied)}")
        subprocess.run(["sudo", "nmcli", "connection", "reload"], check=False)


def kill_network_manager_and_supplicant() -> None:
    # Only release WIFI_IFACE — leave NM running for other interfaces
    subprocess.run(["sudo", "nmcli", "device", "set", WIFI_IFACE, "managed", "no"], check=False)
    # Already-disconnected wlan0 returns "device not active"; suppress stderr noise
    subprocess.run(["sudo", "nmcli", "device", "disconnect", WIFI_IFACE],
                   check=False, stderr=subprocess.DEVNULL)
    subprocess.run(["sudo", "systemctl", "stop", f"wpa_supplicant@{WIFI_IFACE}"], check=False)
    subprocess.run(["sudo", "rfkill", "unblock", "wifi"], check=False)
    time.sleep(1)


def start_dnsmasq() -> None:
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


def start_hostapd() -> None:
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


def deauth_all_clients() -> int:
    """Force every associated station off the AP. Returns the number of
    stations deauthenticated (0 if hostapd isn't running)."""
    try:
        out = subprocess.run(
            ["sudo", "hostapd_cli", "-p", "/var/run/hostapd", "-i", WIFI_IFACE, "list_sta"],
            capture_output=True, text=True, timeout=3,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired):
        return 0
    macs = [m.strip() for m in (out.stdout or "").splitlines() if m.strip()]
    for mac in macs:
        try:
            subprocess.run(
                ["sudo", "hostapd_cli", "-p", "/var/run/hostapd", "-i", WIFI_IFACE,
                 "deauthenticate", mac],
                capture_output=True, text=True, timeout=3,
            )
        except Exception:
            pass
    return len(macs)


def _hostapd_state() -> str:
    """Query hostapd's actual state via ctrl-interface. Returns the `state=`
    value (DISABLED / COUNTRY_UPDATE / HT_SCAN / ENABLED / ...) or "" on error."""
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

    persist_generated_wifi_profiles()
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


def teardown_ap() -> None:
    subprocess.run(["sudo", "pkill", "-f", f"hostapd.*{HOSTAPD_CONFIG_PATH}"], check=False)
    subprocess.run(["sudo", "pkill", "-f", f"dnsmasq.*{DNSMASQ_CONFIG_PATH}"], check=False)
    subprocess.run(["sudo", "ip", "addr", "flush", "dev", WIFI_IFACE], check=False)

    # Restore the original firewalld zone
    _firewalld_restore_iface(WIFI_IFACE)

    # Hand WIFI_IFACE back to NetworkManager
    subprocess.run(["sudo", "nmcli", "device", "set", WIFI_IFACE, "managed", "yes"], check=False)
    restore_wifi_client()
    print("[wifi_ap] AP down")


def saved_wifi_profiles() -> list:
    """Names of the stored Wi-Fi profiles NetworkManager can reconnect with."""
    try:
        out = subprocess.run(
            ["nmcli", "-t", "-f", "NAME,TYPE", "connection", "show"],
            capture_output=True, text=True, timeout=10).stdout
    except Exception:
        return []
    return [ln.rsplit(":", 1)[0] for ln in out.splitlines() if ln.endswith(":802-11-wireless")]


IMAGER_NETWORK_CONFIG = "/boot/firmware/network-config"


def imager_wifi_credentials() -> tuple:
    """SSID and key from the boot partition config the imager wrote, or (None, None).

    On a Lite image the imager stores the network as netplan YAML that cloud-init
    renders, so there is no NetworkManager profile of its own to fall back on.
    """
    try:
        text = subprocess.run(["sudo", "cat", IMAGER_NETWORK_CONFIG],
                              capture_output=True, text=True, timeout=10).stdout
    except Exception:
        return (None, None)
    if not text.strip():
        return (None, None)

    try:
        import yaml
        aps = ((yaml.safe_load(text) or {}).get("network", {})
               .get("wifis", {}).get(WIFI_IFACE, {}).get("access-points", {}))
        for ssid, opts in (aps or {}).items():
            key = (opts or {}).get("password")
            if ssid and key:
                return (str(ssid), str(key))
        return (None, None)
    except ImportError:
        pass

    ssid = None
    for line in text.splitlines():
        stripped = line.strip()
        if ssid is None:
            m = re.match(r'^"?([^"#:]+)"?:$', stripped)
            if m and not stripped.startswith(("network", "wifis", "ethernets", "access-points")):
                ssid = m.group(1)
            continue
        m = re.match(r'^password:\s*"?([^"\s]+)"?$', stripped)
        if m:
            return (ssid, m.group(1))
    return (None, None)


def restore_imager_wifi_profile() -> bool:
    """Recreate the imager's network as a NetworkManager profile of its own."""
    ssid, key = imager_wifi_credentials()
    if not ssid or not key:
        return False
    r = subprocess.run(
        ["sudo", "nmcli", "connection", "add", "type", "wifi",
         "ifname", WIFI_IFACE, "con-name", ssid, "ssid", ssid,
         "wifi-sec.key-mgmt", "wpa-psk", "wifi-sec.psk", key,
         "connection.autoconnect", "yes"],
        capture_output=True, text=True, timeout=20)
    if r.returncode != 0:
        print(f"[wifi_ap] restoring {ssid!r} from the imager config failed: {r.stderr.strip()}")
        return False
    print(f"[wifi_ap] restored Wi-Fi profile {ssid!r} from {IMAGER_NETWORK_CONFIG}")
    return True


def restore_wifi_client() -> None:
    """Re-read the stored profiles and let NetworkManager reconnect the interface."""
    subprocess.run(["sudo", "nmcli", "connection", "reload"], check=False)
    saved = saved_wifi_profiles()
    if not saved and restore_imager_wifi_profile():
        saved = saved_wifi_profiles()
    if not saved:
        print(f"[wifi_ap] WARNING: no saved Wi-Fi profile left, {WIFI_IFACE} stays offline")
        return
    print(f"[wifi_ap] saved Wi-Fi profiles: {', '.join(saved)}")
    subprocess.run(["sudo", "nmcli", "device", "connect", WIFI_IFACE],
                   check=False, capture_output=True, timeout=30)
