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
        return  # firewall-cmd not installed → no firewalld to manage
    if rc != 0 or not prior:
        # firewalld likely inactive; trying to set anyway will fail harmlessly.
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
        # Non-fatal: on next firewalld reload the runtime change is gone anyway.
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


def write_hostapd_config():
    security_config = get_security_config()
    config = f"""
interface={WIFI_IFACE}
driver=nl80211
ssid={SSID}
hw_mode=a
channel={CHANNEL}
country_code={COUNTRY_CODE}
ieee80211n=1
ieee80211ac=1
ignore_broadcast_ssid=0
wmm_enabled=1
{security_config}
wpa_passphrase={PASSPHRASE}
"""
    with open(HOSTAPD_CONFIG_PATH, "w") as f:
        f.write(config)


def setup_network_interface():
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
    # Only release {WIFI_IFACE} — leave NM running for other interfaces (e.g. wlan1/MT7925)
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


def wait_for_ap_ready(timeout_seconds: float = 8.0) -> bool:
    """
    Block until hostapd has actually brought WIFI_IFACE up as an AP, not just
    until the Popen returned. Polls `iw dev WIFI_IFACE info` for `type AP` and
    `channel <n>` (a populated channel line means hostapd finished its
    nl80211 init and is broadcasting beacons).

    Returns True on success, False on timeout. We don't fail hard on a
    timeout — the rest of the bring-up (BT advertising, profile register)
    will still succeed; the phone just may see BT before WiFi and retry
    its AP-join. Logging the timeout is enough to diagnose.

    Why this matters: BlueZ's adapter.Set('Discoverable', True) below in
    main() flips BT advertising as soon as it returns. Pairing phones
    that auto-trigger AA hit the BT advert and immediately try to join
    the announced AP via the WPP (Wi-Fi Pairing Protocol) credential
    exchange — if hostapd hasn't beaconed yet the phone's join times out
    and we have to retry the whole sequence.
    """
    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        try:
            out = subprocess.check_output(
                ["iw", "dev", WIFI_IFACE, "info"],
                text=True, stderr=subprocess.DEVNULL,
            )
            # Two indicators: type AP (nl80211 mode flipped) and channel set
            # (hostapd's hw_mode/channel block applied). Both required.
            if "type AP" in out and "channel " in out:
                return True
        except (subprocess.CalledProcessError, FileNotFoundError):
            pass
        time.sleep(0.2)
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

    # Don't return until hostapd is actually beaconing
    if wait_for_ap_ready():
        print(f"[wifi_ap] AP up — SSID={SSID!r}  IP={AP_IP}  channel={CHANNEL}")
    else:
        print(f"[wifi_ap] AP wait timed out — proceeding anyway (BT may race the phone's AP-join)")


def teardown_ap():
    subprocess.run(["sudo", "pkill", "-f", f"hostapd.*{HOSTAPD_CONFIG_PATH}"], check=False)
    subprocess.run(["sudo", "pkill", "-f", f"dnsmasq.*{DNSMASQ_CONFIG_PATH}"], check=False)
    subprocess.run(["sudo", "ip", "addr", "flush", "dev", WIFI_IFACE], check=False)

    # Restore the original firewalld zone — silent no-op with no firewalld
    _firewalld_restore_iface(WIFI_IFACE)

    # Hand WIFI_IFACE back to NetworkManager — symmetric with kill_network_manager_and_supplicant
    # at start. If NM happens to grab it for Wi-Fi-client mode before the next
    # AA session starts, setup_ap()'s `nmcli device set WIFI_IFACE managed no` plus
    # the new `pkill stale hostapd` defensive cleanup unsticks it within the
    # NM-disconnect race window.
    subprocess.run(["sudo", "nmcli", "device", "set", WIFI_IFACE, "managed", "yes"], check=False)
    print("[wifi_ap] AP down")
