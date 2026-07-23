import smbus2
import threading
import time
from struct import Struct

from shared.config import MFI_I2C_BUS, MFI_POWER_GPIO

Word = Struct(">H")
# i2c address is selected by the chip's RST pin at power-up, init() probes both wirings.
DEV_ADDR = 0x10
_DEV_ADDR_CANDIDATES = (0x10, 0x11)
# The chip NACKs its address while busy processing, so retries keep going with 500us
# spacing until it ACKs. Cert reads and challenge signing take tens of ms.
_BUSY_RETRY_S = 0.0005
_IO_TIMEOUT_S = 2.0
_PROBE_TIMEOUT_S = 2.0
_AUTH_POLL_S = 0.01
# Signing time (measured): ~440ms for a 3.0 and 2.0C, ~1645ms for a 2.0B
_AUTH_TIMEOUT_S = 3.0
# Opened lazily by init() so importing this module never touches i2c/GPIO.
bus = None
# Auth protocol major version reported by the chip (2 = SHA-1 / 20-byte challenge,
# 3 = SHA-256 / 32-byte challenge). Read once in init().
_protocol_major = None
# Serialise all chip access so the BT-auth path and the HTTP bridge never
# interleave transactions on the shared i2c bus.
_i2c_lock = threading.Lock()
# GPIO that powers the coprocessor. Default BCM 21, override with LIVI_CP_MFI_POWER_GPIO.
CHIP_POWER = int(MFI_POWER_GPIO) if MFI_POWER_GPIO else 21
# The chip stays powered for as long as the helper runs
_gpio_handle = None
_powered_soc = None

# SoC-detection
def _get_soc_model():
    try:
        with open("/proc/device-tree/compatible", "rb") as f:
            data = f.read()
            if b"bcm2712" in data:
                return "BCM2712"
            elif b"bcm2711" in data:
                return "BCM2711"
    except FileNotFoundError:
        pass

    # Fallback
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.startswith("Model"):
                    if "Compute Module 5" in line or "Raspberry Pi 5" in line:
                        return "BCM2712"
                    elif "Compute Module 4" in line or "Raspberry Pi 4" in line:
                        return "BCM2711"
    except FileNotFoundError:
        pass

    return "Unknown"


def _power_on(soc):
    global _gpio_handle, _powered_soc
    if soc == "BCM2712":
        # Raspberry Pi 5 / CM5
        import lgpio

        _gpio_handle = lgpio.gpiochip_open(0)
        lgpio.gpio_claim_output(_gpio_handle, CHIP_POWER, 1)
    elif soc == "BCM2711":
        # Raspberry Pi 4 / CM4
        import RPi.GPIO as GPIO

        GPIO.setwarnings(False)
        GPIO.setmode(GPIO.BCM)
        GPIO.setup(CHIP_POWER, GPIO.OUT)
        GPIO.output(CHIP_POWER, GPIO.HIGH)
    else:
        raise RuntimeError(f"unknown SoC type: {soc}")
    _powered_soc = soc
    print(f"[mfi] power on  gpio={CHIP_POWER} soc={soc}", flush=True)
    time.sleep(0.1)


def power_off():
    """Drop the coprocessor's supply. Called when the helper shuts down."""
    global _gpio_handle, _powered_soc, bus
    if _powered_soc is not None:
        print(f"[mfi] power off gpio={CHIP_POWER} soc={_powered_soc}", flush=True)
    if _powered_soc == "BCM2712" and _gpio_handle is not None:
        import lgpio

        lgpio.gpio_write(_gpio_handle, CHIP_POWER, 0)
        lgpio.gpiochip_close(_gpio_handle)
        _gpio_handle = None
    elif _powered_soc == "BCM2711":
        import RPi.GPIO as GPIO

        GPIO.output(CHIP_POWER, GPIO.LOW)
        GPIO.cleanup(CHIP_POWER)
    _powered_soc = None
    if bus is not None:
        try:
            bus.close()
        except Exception:
            pass
        bus = None


def _probe_dev_addr():
    """Find the chip's i2c address by reading the Device Version register at each candidate."""
    deadline = time.monotonic() + _PROBE_TIMEOUT_S
    while time.monotonic() < deadline:
        for cand in _DEV_ADDR_CANDIDATES:
            try:
                bus.i2c_rdwr(smbus2.i2c_msg.write(cand, [0x00]))
                read_msg = smbus2.i2c_msg.read(cand, 1)
                bus.i2c_rdwr(read_msg)
                return cand
            except OSError:
                pass
        time.sleep(_BUSY_RETRY_S)
    return None


def init():
    """SoC-detect, power the coprocessor, open the i2c bus and read the chip's generation.
    Idempotent: called once at helper startup so the chip has boot time before the first
    auth, and as a no-op safety net from the auth entry points."""
    global bus, _protocol_major, DEV_ADDR
    if bus is not None:
        return
    soc = _get_soc_model()
    _power_on(soc)
    bus = smbus2.SMBus(MFI_I2C_BUS)
    found = _probe_dev_addr()
    if found is not None:
        DEV_ADDR = found
    else:
        addrs = "/".join(f"0x{a:02X}" for a in _DEV_ADDR_CANDIDATES)
        print(f"[mfi] no coprocessor answered at {addrs}, keeping i2c_addr=0x{DEV_ADDR:02X}", flush=True)
    device_version = None
    try:
        device_version = _read_i2c(0x00, 1)[0]  # Read Device Version (0x05 = 2.0C, 0x07 = 3.0)
        _protocol_major = _read_i2c(0x02, 1)[0]  # Read Authentication Protocol Major Version
    except Exception as e:
        print(f"[mfi] chip version read failed: {e!r}", flush=True)
    print(f"[mfi] i2c_addr=0x{DEV_ADDR:02X} device_version={device_version} protocol_major={_protocol_major} "
          f"i2c_bus={MFI_I2C_BUS} power_gpio={CHIP_POWER} soc={soc}", flush=True)


def protocol_major():
    """Auth protocol major version the chip reports (2 = 2.0C, 3 = 3.0). None if unread."""
    init()
    return _protocol_major

def _i2c_retry(op, what):
    """Run an i2c transaction, retrying on NACK until the busy chip ACKs or the timeout hits."""
    deadline = time.monotonic() + _IO_TIMEOUT_S
    while True:
        try:
            return op()
        except OSError:
            if time.monotonic() >= deadline:
                raise Exception(f"timeout during {what}")
            time.sleep(_BUSY_RETRY_S)

def _read_i2c(addr, n):
    _i2c_retry(lambda: bus.i2c_rdwr(smbus2.i2c_msg.write(DEV_ADDR, [addr])),
               f"register select 0x{addr:02X}")

    def _op():
        read_msg = smbus2.i2c_msg.read(DEV_ADDR, n)
        bus.i2c_rdwr(read_msg)
        return bytes(read_msg)
    return _i2c_retry(_op, f"read at 0x{addr:02X}")

def _write_i2c(addr, arr):
    def _op():
        bus.i2c_rdwr(smbus2.i2c_msg.write(DEV_ADDR, [addr] + list(arr)))
    _i2c_retry(_op, f"write at 0x{addr:02X}")

def read_certificate():
    with _i2c_lock:
        init()
        size = Word.unpack(_read_i2c(0x30, 2))[0]  # Read Accessory Certificate Data Length
        return _read_i2c(0x31, size)  # Read Accessory Certificate Data

def generate_challenge_response(challenge):
    n = len(challenge)
    # 2.0C signs a 20-byte SHA-1 digest, 3.0 signs a 32-byte SHA-256 digest.
    if not 1 <= n <= 128:
        raise ValueError(f"Challenge must be 1..128 bytes, got {n}")

    with _i2c_lock:
        init()
        _write_i2c(0x20, Word.pack(n))  # Write Challenge Data Length

        try:
            _write_i2c(0x21, challenge)
        except Exception as e:
            raise Exception("Failed to write challenge data") from e

        _i2c_retry(lambda: bus.i2c_rdwr(smbus2.i2c_msg.write(DEV_ADDR, [0x10, 0x01])),
                   "start authentication")

        time.sleep(0.01)
        deadline = time.monotonic() + _AUTH_TIMEOUT_S
        while True:
            try:
                bus.i2c_rdwr(smbus2.i2c_msg.write(DEV_ADDR, [0x10]))
                status_msg = smbus2.i2c_msg.read(DEV_ADDR, 1)
                bus.i2c_rdwr(status_msg)
                if list(status_msg)[0] == 0x10:
                    break
            except OSError:
                pass
            if time.monotonic() >= deadline:
                try:
                    err = "0x%02X" % _read_i2c(0x05, 1)[0]
                except Exception:
                    err = "unreadable"
                raise Exception(f"timeout or auth failed (error code {err})")
            time.sleep(_AUTH_POLL_S)

        size = Word.unpack(_read_i2c(0x11, 2))[0]
        return _read_i2c(0x12, size)

if __name__ == "__main__":
    major = protocol_major()
    print("GEN", major)
    print("CERT", read_certificate().hex())
    challenge = bytes([0xAB] * (20 if major == 2 else 32))
    print("RESP", generate_challenge_response(challenge).hex())
