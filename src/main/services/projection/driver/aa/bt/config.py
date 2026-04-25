"""
config.py — runtime configuration for the LIVI Android Auto python stack.

All values come from `LIVI_*` environment variables that aaBluetoothSupervisor.ts
injects into the python child. The hardcoded defaults below are a safety net
so a missing env var doesn't crash the script — under normal operation LIVI
provides every variable explicitly.

"""

import os


def _env(key: str, default: str) -> str:
    """Return the env var, treating empty strings as missing."""
    val = os.environ.get(key, "")
    return val if val else default


# ── Wi-Fi ─────────────────────────────────────────────────────────────────────
SSID         = _env("LIVI_SSID",         "LIVI")
PASSPHRASE   = _env("LIVI_PASSPHRASE",   "12345678")
CHANNEL      = int(_env("LIVI_CHANNEL",  "36"))
COUNTRY_CODE = _env("LIVI_COUNTRY",      "DE")
PORT         = int(_env("LIVI_PORT",     "5277"))
WIFI_IFACE   = _env("LIVI_WIFI_IFACE",   "wlan0")

# ── Bluetooth ─────────────────────────────────────────────────────────────────
BTNAME       = _env("LIVI_BTNAME",       "LIVI")
BT_ADAPTER   = _env("LIVI_BT_ADAPTER",   "hci0")
