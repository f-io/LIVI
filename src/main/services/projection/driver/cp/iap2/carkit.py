"""Wired CarPlay transport: LIVI's own config-6 usbmux daemon + lockdown carkit service.

Detect a wired iPhone via sysfs, bring it to config 6 and run the usbmux daemon on its
multiplexer, then let pymobiledevice3 reach lockdown + com.apple.carkit.service over that
socket. The carkit channel is the TLS iAP2 control channel handed to the iAP2 handler.
"""
import asyncio

from iap2 import livi_sock
from shared.config import CP_SYSLOG_TAP


def _bump_backoff(backoff, serial, now):
    fails = backoff.get(serial, (0.0, 0))[1] + 1
    delay = min(2.0 * (2 ** (fails - 1)), 15.0)
    backoff[serial] = (now + delay, fails)


async def _session_for(serial, socket_path, on_session, log):
    from pymobiledevice3.lockdown import create_using_usbmux
    tap_task = None
    try:
        lockdown = await create_using_usbmux(serial=serial, usbmux_address=socket_path)
        svc = await lockdown.start_lockdown_service("com.apple.carkit.service")
        if svc.reader is None or svc.writer is None:
            raise RuntimeError("no TLS reader/writer on carkit channel")
        log("[carkit] carkit TLS channel up (iAP2) udid=%s" % (serial or "?"))
        if CP_SYSLOG_TAP:
            from iap2 import syslog_tap
            tap_task = asyncio.ensure_future(syslog_tap.run(socket_path, serial, log))
        task = on_session(svc.reader, svc.writer, serial)
        if task is not None:
            await task
    except Exception as e:
        log("[carkit] session ended (udid=%s): %r" % (serial or "?", e))
    finally:
        if tap_task is not None:
            tap_task.cancel()


async def _run(loop, on_session, log):
    try:
        import pymobiledevice3.lockdown  # noqa: F401
    except Exception as e:
        log("[carkit] pymobiledevice3 unavailable, wired CarPlay disabled: %r" % e)
        return
    from iap2 import muxd

    log("[carkit] wired watcher running (config-6 usbmux)")
    sessions = {}
    backoff = {}
    last_n = -1
    while True:
        present = set(muxd.list_iphones())
        if len(present) != last_n:
            log("[carkit] wired iPhones: %d active=%d" % (len(present), len(sessions)))
            last_n = len(present)
        now = loop.time()
        for serial in present:
            if serial in sessions:
                continue
            if now < backoff.get(serial, (0.0, 0))[0]:
                continue
            d = muxd.Muxd(serial, log)
            sessions[serial] = [None, d, now]
            try:
                sock = await loop.run_in_executor(None, d.start)
            except Exception as e:
                log("[carkit] muxd start failed udid=%s: %r" % (serial[:8], e))
                await loop.run_in_executor(None, d.stop)
                sessions.pop(serial, None)
                _bump_backoff(backoff, serial, now)
                continue
            sessions[serial][0] = loop.create_task(_session_for(serial, sock, on_session, log))
        for serial in list(sessions):
            task, d, started = sessions[serial]
            gone = serial not in present
            if not gone and (task is None or not task.done()):
                continue
            if gone:
                if task is not None and not task.done():
                    task.cancel()
                await loop.run_in_executor(None, d.stop)
                del sessions[serial]
                log("[carkit] wired iPhone unplugged udid=%s" % serial)
                livi_sock.push({"type": "device-gone", "src": "carkit", "usbUdid": serial})
            else:
                log("[carkit] carkit reconnect udid=%s (config-6 daemon stays up)" % serial[:8])
                sessions[serial][0] = loop.create_task(_session_for(serial, d.socket_path, on_session, log))
                sessions[serial][2] = now
        await asyncio.sleep(1.0)


def start(loop, on_session, log):
    fut = asyncio.run_coroutine_threadsafe(_run(loop, on_session, log), loop)

    def _done(f):
        try:
            f.result()
        except Exception as e:
            log("[carkit] watcher crashed: %r" % e)

    fut.add_done_callback(_done)
    return fut
