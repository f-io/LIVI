"""
wifi_ap.py — WiFi Access Point for Android Auto

Direct port of iap2/wifi_ap.py — same structure, same function names.
Apple-specific parts removed (WAC vendor_elements, Bonjour/Avahi).
"""

import re
import os
import subprocess
import time

from config import SSID, PASSPHRASE, CHANNEL, COUNTRY_CODE

HOSTAPD_CONFIG_PATH = "/tmp/livi-hostapd.conf"
DNSMASQ_CONFIG_PATH = "/tmp/livi-dnsmasq.conf"
AP_IP = "10.10.0.1"


def get_wlan_mac(iface: str = "wlan0") -> str:
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
interface=wlan0
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
    subprocess.run(["sudo", "ip", "link", "set", "wlan0", "up"], check=True)
    subprocess.run(["sudo", "ip", "addr", "flush", "dev", "wlan0"], check=True)
    subprocess.run(["sudo", "ip", "addr", "add", f"{AP_IP}/24", "dev", "wlan0"], check=True)


def disable_existing_wifi_network_services():
    subprocess.run(["sudo", "systemctl", "stop", "wpa_supplicant@wlan0"], check=False)
    subprocess.run(["sudo", "systemctl", "disable", "wpa_supplicant@wlan0"], check=False)
    subprocess.run(["sudo", "ip", "addr", "flush", "dev", "wlan0"], check=False)
    subprocess.run(["sudo", "dhcpcd", "-k", "wlan0"], check=False)
    subprocess.run(["sudo", "ip", "link", "set", "wlan0", "up"], check=False)
    time.sleep(1)


def kill_network_manager_and_supplicant():
    # Only release wlan0 — leave NM running for other interfaces (e.g. wlan1/MT7925)
    subprocess.run(["sudo", "nmcli", "device", "set", "wlan0", "managed", "no"], check=False)
    subprocess.run(["sudo", "nmcli", "device", "disconnect", "wlan0"], check=False)
    subprocess.run(["sudo", "systemctl", "stop", "wpa_supplicant@wlan0"], check=False)
    subprocess.run(["sudo", "rfkill", "unblock", "wifi"], check=False)
    time.sleep(1)


def start_dnsmasq():
    dnsmasq_config = """
interface=wlan0
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
    Block until hostapd has actually brought wlan0 up as an AP, not just
    until the Popen returned. Polls `iw dev wlan0 info` for `type AP` and
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
                ["iw", "dev", "wlan0", "info"],
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
    # Belt-and-suspenders: kill any hostapd/dnsmasq left over from a previous
    # crashed run before kill_network_manager_and_supplicant grabs wlan0 again.
    # Without this, a stale hostapd holding wlan0 makes our new instance log
    # "could not configure driver: nl80211 driver initialization failed".
    subprocess.run(["sudo", "pkill", "-f", f"hostapd.*{HOSTAPD_CONFIG_PATH}"], check=False)
    subprocess.run(["sudo", "pkill", "-f", f"dnsmasq.*{DNSMASQ_CONFIG_PATH}"], check=False)
    time.sleep(0.3)

    kill_network_manager_and_supplicant()
    disable_existing_wifi_network_services()
    setup_network_interface()
    write_hostapd_config()
    start_dnsmasq()
    start_hostapd()

    # Don't return until hostapd is *actually* beaconing — see comment in
    # wait_for_ap_ready. Caller (aa-bluetooth.py main) gates BT advertising
    # on this returning so the phone never sees BT before WiFi.
    if wait_for_ap_ready():
        print(f"[wifi_ap] AP up — SSID={SSID!r}  IP={AP_IP}  channel={CHANNEL}")
    else:
        print(f"[wifi_ap] AP wait timed out — proceeding anyway (BT may race the phone's AP-join)")


def teardown_ap():
    subprocess.run(["sudo", "pkill", "-f", f"hostapd.*{HOSTAPD_CONFIG_PATH}"], check=False)
    subprocess.run(["sudo", "pkill", "-f", f"dnsmasq.*{DNSMASQ_CONFIG_PATH}"], check=False)
    subprocess.run(["sudo", "ip", "addr", "flush", "dev", "wlan0"], check=False)
    # Hand wlan0 back to NetworkManager — symmetric with kill_network_manager_and_supplicant
    # at start. If NM happens to grab it for Wi-Fi-client mode before the next
    # AA session starts, setup_ap()'s `nmcli device set wlan0 managed no` plus
    # the new `pkill stale hostapd` defensive cleanup unsticks it within the
    # NM-disconnect race window.
    subprocess.run(["sudo", "nmcli", "device", "set", "wlan0", "managed", "yes"], check=False)
    print("[wifi_ap] AP down")
