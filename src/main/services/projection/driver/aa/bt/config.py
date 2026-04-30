"""
config.py — runtime configuration for the LIVI Android Auto python stack.
Reads from ~/.config/LIVI/config.json first, then falls back to environment variables.
"""

import os
import json
from pathlib import Path

def _get_config_path() -> Path:
    original_user = os.environ.get('SUDO_USER', os.environ.get('USER', ''))
    if original_user and original_user != 'root':
        return Path(f"/home/{original_user}") / ".config" / "LIVI" / "config.json"
    return Path.home() / ".config" / "LIVI" / "config.json"

def _load_from_json() -> dict:
    """Load configuration from ~/.config/LIVI/config.json if it exists."""
    config_path = _get_config_path()

    try:
        with open(config_path, 'r') as f:
            data = json.load(f)
            print(f"[config] loaded from {config_path}")
            return data
    except FileNotFoundError:
        print(f"[config] no config.json found at {config_path}")
    except json.JSONDecodeError as e:
        print(f"[config] error parsing {config_path}: {e}")

    return {}

def _get_value(key: str, json_key: str, default: str) -> str:
    """Get value from JSON, then env var, then default."""
    # First try config.json
    val = _JSON_CONFIG.get(json_key, "")
    if val:
        return str(val)

    # Then try environment variable
    env_val = os.environ.get(key, "")
    if env_val:
        return env_val

    # Finally default
    return default

# Load JSON once at module import
_JSON_CONFIG = _load_from_json()

# ── Wi-Fi ─────────────────────────────────────────────────────────────────────
SSID         = _get_value("LIVI_SSID",         "ssid",          "LIVI")
PASSPHRASE   = _get_value("LIVI_PASSPHRASE",   "wifiPassword",  "12345678")
CHANNEL      = int(_get_value("LIVI_CHANNEL",  "wifiChannel",   "36"))
COUNTRY_CODE = _get_value("LIVI_COUNTRY",      "country",       "DE")
PORT         = int(_get_value("LIVI_PORT",     "port",          "5277"))
WIFI_IFACE   = _get_value("LIVI_WIFI_IFACE",   "wifiInterface", "wlan0")

# ── Bluetooth ─────────────────────────────────────────────────────────────────
BTNAME       = _get_value("LIVI_BTNAME",       "carName",       "LIVI")
BT_ADAPTER   = _get_value("LIVI_BT_ADAPTER",   "btAdapter",     "hci0")
