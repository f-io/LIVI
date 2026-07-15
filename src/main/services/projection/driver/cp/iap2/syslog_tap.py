"""Debug: stream the iPhone's own syslog over LIVI's usbmux socket to capture why carkitd ends a wired session."""
import asyncio

_KEYWORDS = ("carplay", "carkit", "iap", "airplay", "avconference",
             "terminat", "disconnect", "timeout", "teardown")


async def run(socket_path, serial, log):
    from pymobiledevice3.lockdown import create_using_usbmux
    from pymobiledevice3.services.syslog import SyslogService
    svc = None
    try:
        lockdown = await create_using_usbmux(serial=serial, usbmux_address=socket_path)
        svc = SyslogService(lockdown)
        log("[iphone-log] syslog tap up (udid=%s), filtering CarPlay/carkit" % (serial or "?"))
        async for line in svc.watch():
            low = line.lower()
            if any(k in low for k in _KEYWORDS):
                log("[iphone-log] " + line.rstrip())
    except asyncio.CancelledError:
        raise
    except Exception as e:
        log("[iphone-log] syslog tap ended: %r" % e)
    finally:
        if svc is not None:
            try:
                svc.close()
            except Exception:
                pass
