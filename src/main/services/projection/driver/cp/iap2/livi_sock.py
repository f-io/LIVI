"""LIVI control socket — one Unix-domain-socket channel between LIVI (Electron,
non-root) and this root helper, mirroring AA's aa-bt.sock. It replaces the MFi
HTTP bridge and carries every LIVI <-> helper exchange over a single socket.

Each connection picks a mode with its first newline-terminated line:

    certificate            -> {"ok": true, "data": "<base64 MFi certificate>"}
    sign <base64 digest>   -> {"ok": true, "data": "<base64 signature>"}
    disconnect <mac>       -> {"ok": true} | {"ok": false, "error": "..."}
    tunnel                 -> raw iAP2 bytes both ways; the connection becomes the
                              iAP2 transport so the helper runs a fresh iAP2 session
                              over CarPlay Wi-Fi (used after the phone sends
                              disableBluetooth to move off Bluetooth).

The socket is chmod 0o666 so the non-root Electron process can connect. The MFi
sign/certificate calls touch the i2c coprocessor and block, so they run in an
executor to keep the asyncio loop responsive.
"""

import asyncio
import base64
import json
import os
import subprocess

from iap2.mfi_auth_coprocessor import read_certificate, generate_challenge_response

SOCK_PATH = "/tmp/cp-bt.sock"

_subscribers = set()


def push(obj):
    """Broadcast one JSON line to every 'subscribe' client; drop dead writers."""
    line = (json.dumps(obj) + "\n").encode()
    for w in list(_subscribers):
        try:
            w.write(line)
        except Exception:
            _subscribers.discard(w)


def _device_path(adapter, mac):
    return "/org/bluez/%s/dev_%s" % (adapter, mac.upper().replace(":", "_"))


def _device_disconnect(adapter, mac):
    """BlueZ Device1.Disconnect — targeted, drops this phone's ACL (and A2DP)."""
    try:
        r = subprocess.run(
            ["busctl", "--system", "call", "org.bluez", _device_path(adapter, mac),
             "org.bluez.Device1", "Disconnect"],
            capture_output=True, check=False, timeout=10)
        if r.returncode == 0:
            return True, ""
        return False, r.stderr.decode(errors="replace").strip() or "disconnect failed"
    except Exception as e:
        return False, "%s: %s" % (type(e).__name__, e)


def start(loop, on_tunnel, adapter, log, on_drop_iap2=None, on_command=None):
    """Start the control socket. on_tunnel(reader, writer, cid) runs an iAP2 session over
    a tunnel connection (cid is the pair-verify controllerId that tags its metadata).
    on_drop_iap2() closes the current BT iAP2 link (disableBluetooth handling).
    on_command(cmd, arg) handles any other RPC verb, returning True if handled."""

    async def _reply(writer, obj):
        writer.write((json.dumps(obj) + "\n").encode())
        await writer.drain()

    async def _handle_rpc(reader, writer, line):
        parts = line.split(" ", 1)
        cmd = parts[0]
        arg = parts[1].strip() if len(parts) > 1 else ""
        try:
            if cmd == "certificate":
                cert = await loop.run_in_executor(None, read_certificate)
                await _reply(writer, {"ok": True, "data": base64.b64encode(bytes(cert)).decode()})
            elif cmd == "sign":
                digest = base64.b64decode(arg)
                sig = await loop.run_in_executor(None, lambda: generate_challenge_response(digest))
                await _reply(writer, {"ok": True, "data": base64.b64encode(bytes(sig)).decode()})
            elif cmd == "disconnect":
                if not arg:
                    await _reply(writer, {"ok": False, "error": "disconnect requires a MAC"})
                else:
                    ok, err = await loop.run_in_executor(None, lambda: _device_disconnect(adapter, arg))
                    await _reply(writer, {"ok": True} if ok else {"ok": False, "error": err})
            elif cmd == "drop-iap2":
                if on_drop_iap2 is not None:
                    on_drop_iap2()
                await _reply(writer, {"ok": True})
            elif on_command is not None and on_command(cmd, arg):
                await _reply(writer, {"ok": True})
            else:
                await _reply(writer, {"ok": False, "error": "unknown command: %r" % cmd})
        except Exception as e:
            try:
                await _reply(writer, {"ok": False, "error": "%s: %s" % (type(e).__name__, e)})
            except Exception:
                pass

    async def _client(reader, writer):
        try:
            line = (await reader.readline()).decode(errors="replace").strip()
        except Exception:
            writer.close()
            return
        if line == "subscribe":
            _subscribers.add(writer)
            log("[cp-sock] event subscriber connected")
            try:
                await reader.read()
            except Exception:
                pass
            finally:
                _subscribers.discard(writer)
                try:
                    writer.close()
                except Exception:
                    pass
            return
        if line == "tunnel" or line.startswith("tunnel "):
            cid = line[len("tunnel"):].strip()
            log("[cp-sock] tunnel connection — running iAP2 over CarPlay"
                + (" (cid=%s)" % cid if cid else ""))
            # Hand ownership of the connection to the iAP2 session. Any iAP2 bytes
            # that arrived after the "tunnel\n" line stay buffered in the reader.
            on_tunnel(reader, writer, cid)
            return
        await _handle_rpc(reader, writer, line)
        try:
            writer.close()
        except Exception:
            pass

    async def _serve():
        try:
            os.unlink(SOCK_PATH)
        except OSError:
            pass
        server = await asyncio.start_unix_server(_client, path=SOCK_PATH)
        os.chmod(SOCK_PATH, 0o666)
        log("[cp-sock] listening on %s" % SOCK_PATH)
        return server

    return loop.create_task(_serve())
