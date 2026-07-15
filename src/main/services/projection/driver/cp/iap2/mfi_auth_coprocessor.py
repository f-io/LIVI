import smbus2
import threading
import time
from struct import Struct

from shared.config import CP_GEN, MFI_I2C_BUS, MFI_POWER_GPIO

Word = Struct(">H")
DEV_ADDR = 0x10
# Opened lazily by init() so importing this module never touches i2c/GPIO.
bus = None
# Serialise all chip access so the BT-auth path and the HTTP bridge never
# interleave transactions on the shared i2c bus.
_i2c_lock = threading.Lock()
# GPIO that powers the coprocessor. Default BCM 21, override with LIVI_CP_MFI_POWER_GPIO.
CHIP_POWER = int(MFI_POWER_GPIO) if MFI_POWER_GPIO else 21

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
    if soc == "BCM2712":
        # Raspberry Pi 5 / CM5
        import lgpio

        h = lgpio.gpiochip_open(0)
        lgpio.gpio_claim_output(h, CHIP_POWER, 1)
        time.sleep(0.1)
        lgpio.gpiochip_close(h)
    elif soc == "BCM2711":
        # Raspberry Pi 4 / CM4
        import RPi.GPIO as GPIO

        GPIO.setwarnings(False)
        GPIO.setmode(GPIO.BCM)
        GPIO.setup(CHIP_POWER, GPIO.OUT)
        GPIO.output(CHIP_POWER, GPIO.HIGH)
        time.sleep(0.1)
    else:
        raise RuntimeError(f"unknown SoC type: {soc}")


def init():
    """SoC-detect, power the coprocessor and open the i2c bus. Idempotent: called once at
    helper startup so the chip has boot time before the first auth, and as a no-op safety
    net from the auth entry points."""
    global bus
    if bus is not None:
        return
    soc = _get_soc_model()
    print(f"[mfi] gen={CP_GEN} i2c_bus={MFI_I2C_BUS} power_gpio={CHIP_POWER} soc={soc}", flush=True)
    _power_on(soc)
    bus = smbus2.SMBus(MFI_I2C_BUS)

def _read_i2c(addr, n):
    for _ in range(5):
        try:
            bus.i2c_rdwr(smbus2.i2c_msg.write(DEV_ADDR, [addr]))
            read_msg = smbus2.i2c_msg.read(DEV_ADDR, n)
            bus.i2c_rdwr(read_msg)
            return bytes(read_msg)
        except OSError:
            time.sleep(0.0005)
    raise Exception(f"timeout during read at 0x{addr:02X}")

def _write_i2c(addr, arr):
    for _ in range(5):
        try:
            msg = smbus2.i2c_msg.write(DEV_ADDR, [addr] + list(arr))
            bus.i2c_rdwr(msg)
            return
        except OSError:
            time.sleep(0.0005)
    raise Exception(f"timeout during write at 0x{addr:02X}")

def read_certificate():
    with _i2c_lock:
        init()
        size = Word.unpack(_read_i2c(0x30, 2))[0]  # Read Accessory Certificate Data Length
        return _read_i2c(0x31, size)  # Read Accessory Certificate Data

def generate_challenge_response(challenge):
    if len(challenge) != 32:
        raise ValueError("Challenge must be exactly 32 bytes")

    with _i2c_lock:
        init()
        _write_i2c(0x20, Word.pack(32))  # Write Challenge Data Length

        try:
            _write_i2c(0x21, challenge)
        except Exception as e:
            raise Exception("Failed to write challenge data") from e

        for _ in range(3):
            try:
                msg = smbus2.i2c_msg.write(DEV_ADDR, [0x10, 0x01])
                bus.i2c_rdwr(msg)
                break
            except OSError:
                time.sleep(0.0005)
        else:
            raise Exception("Failed to start authentication")

        time.sleep(0.01)
        for _ in range(10):
            try:
                bus.i2c_rdwr(smbus2.i2c_msg.write(DEV_ADDR, [0x10]))
                status_msg = smbus2.i2c_msg.read(DEV_ADDR, 1)
                bus.i2c_rdwr(status_msg)
                if list(status_msg)[0] == 0x10:
                    break
            except OSError:
                pass
            time.sleep(0.1)
        else:
            try:
                err = _read_i2c(0x05, 1)[0]
                raise Exception(f"timeout or auth failed (error code 0x{err:02X})")
            except Exception as e:
                raise Exception("timeout or auth failed, and failed to read error code") from e

        size = Word.unpack(_read_i2c(0x11, 2))[0]
        return _read_i2c(0x12, size)

if __name__ == "__main__":
    print("CERT", read_certificate().hex())
    challenge = bytes([0xAB] * 32)
    print("RESP", generate_challenge_response(challenge).hex())
