from typing import Callable, Dict, Optional

SETUP = 0x04
START = 0x01
FIRST_DATA = 0x80
FIRST_AND_ONLY_DATA = 0xC0
DATA = 0x00
LAST_DATA = 0x40
CANCEL = 0x02
PAUSE = 0x03
SUCCESS = 0x05


class FileTransferReceiver:
    def __init__(self,
                 send: Callable[[bytes], None],
                 on_file: Callable[[int, bytes], None],
                 log: Optional[Callable[..., None]] = None):
        self._send = send
        self._on_file = on_file
        self._log = log or (lambda *a: None)
        self._buffers: Dict[int, bytearray] = {}

    def feed(self, datagram: bytes):
        if len(datagram) < 2:
            return
        ftid = datagram[0]
        ctrl = datagram[1]
        data = datagram[2:]
        if ctrl == SETUP:
            self._buffers[ftid] = bytearray()
            self._send(bytes([ftid, START]))
        elif ctrl == FIRST_DATA:
            self._buffers[ftid] = bytearray(data)
        elif ctrl == DATA:
            self._buffers.setdefault(ftid, bytearray()).extend(data)
        elif ctrl == FIRST_AND_ONLY_DATA:
            self._complete(ftid, bytes(data))
        elif ctrl == LAST_DATA:
            buf = self._buffers.pop(ftid, bytearray())
            buf.extend(data)
            self._complete(ftid, bytes(buf))
        elif ctrl == CANCEL:
            self._buffers.pop(ftid, None)

    def _complete(self, ftid: int, data: bytes):
        self._buffers.pop(ftid, None)
        self._send(bytes([ftid, SUCCESS]))
        self._on_file(ftid, data)
