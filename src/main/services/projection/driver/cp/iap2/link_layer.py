__all__ = ["IAP2Connection", "IAP2Stream"]

import asyncio
from collections import namedtuple
from dataclasses import dataclass
from functools import reduce
from struct import Struct
from typing import ClassVar, List, Callable, Any

from shared.config import DEBUG
from iap2.control_session_message import CSM_START, CSM_STRUCT, _MESSAGE_TYPES

CONTROL_SYN = 0x80
CONTROL_ACK = 0x40
CONTROL_EAK = 0x20
CONTROL_RST = 0x10

loop = asyncio.get_event_loop()


@dataclass
class LinkPacketHeader:
    struct: ClassVar = Struct(">HHBBBB")
    start: ClassVar = 0xFF5A
    length: int
    control: int
    seq: int
    ack: int
    session_id: int

    @staticmethod
    def from_bytes(header_bytes):
        if not check_checksum(header_bytes):
            return None
        (start, length, control, seq, ack,
         session_id) = LinkPacketHeader.struct.unpack(header_bytes[:-1])
        if start != LinkPacketHeader.start:
            return None
        return LinkPacketHeader(length, control, seq, ack, session_id)

    def pack(self):
        header_bytes = LinkPacketHeader.struct.pack(LinkPacketHeader.start,
                                                    self.length, self.control,
                                                    self.seq, self.ack,
                                                    self.session_id)
        return header_bytes + bytes([gen_checksum(header_bytes)])


def signed_add(a, b):
    return (a + b) & 0xff


def gen_checksum(packet):
    return -reduce(signed_add, packet) & 0xff


def check_checksum(packet):
    return reduce(signed_add, packet) == 0


LSPSession = namedtuple('LSPSession', 'id type version')


@dataclass
class LinkSynchronizationPayload:
    struct: ClassVar = Struct(">BBHHHBB")
    version: ClassVar = 0x01
    max_outgoing: int
    max_len: int
    retransmission_timeout: int
    ack_timeout: int
    max_retransmissions: int
    max_ack: int
    sessions: list

    @staticmethod
    def from_bytes(payload):
        (version, max_outgoing, max_len, retransmission_timeout, ack_timeout,
         max_retransmissions,
         max_ack) = LinkSynchronizationPayload.struct.unpack(payload[:10])
        if version != LinkSynchronizationPayload.version:
            return None

        sessions_bytes = payload[10:]
        sessions = []
        for i in range(0, len(sessions_bytes), 3):
            sessions.append(LSPSession._make(sessions_bytes[i:i + 3]))
        return LinkSynchronizationPayload(max_outgoing, max_len,
                                          retransmission_timeout, ack_timeout,
                                          max_retransmissions, max_ack,
                                          sessions)

    def pack(self):
        payload = LinkSynchronizationPayload.struct.pack(
            LinkSynchronizationPayload.version, self.max_outgoing,
            self.max_len, self.retransmission_timeout, self.ack_timeout,
            self.max_retransmissions, self.max_ack)
        return payload + b''.join([bytes([*s]) for s in self.sessions])


STATE_DETECT_IAP2_SUPPORT = 0
STATE_NEGOTIATE = 1
STATE_NORMAL = 2
STATE_DEAD = 3

IAP2_MARKER = b'\xFF\x55\x02\x00\xEE\x10'
EA_SESSION_ID_STRUCT = Struct(">H")


def _short_payload(p):
    if p is None or len(p) <= 48:
        return p
    return b"%s...(+%dB)" % (p[:48], len(p) - 48)


def _fmt_value(v):
    if isinstance(v, bytes):
        return v.hex() if len(v) <= 16 else "<%dB>" % len(v)
    if isinstance(v, list):
        return "[%s]" % ", ".join(_fmt_value(x) for x in v)
    if isinstance(v, (bool, int, float, str)):
        return repr(v)
    if hasattr(v, "__dict__"):
        return _fmt_csm(v)
    return repr(v)


def _fmt_csm(obj):
    fields = ", ".join(
        "%s=%s" % (k, _fmt_value(v))
        for k, v in vars(obj).items()
        if v is not None and v != [])
    return "%s(%s)" % (type(obj).__name__, fields)


def _describe_payload(p):
    if p is None:
        return p
    parts = []
    rest = p
    while len(rest) >= 6:
        start, length, msg_id = CSM_STRUCT.unpack(rest[:6])
        if start != CSM_START or length < 6 or length > len(rest):
            break
        desc = None
        cls = _MESSAGE_TYPES.get(msg_id)
        if cls is not None:
            try:
                inst = cls.__new__(cls)
                inst.csm_deserialize_params(rest[6:length])
                desc = _fmt_csm(inst)
            except Exception:
                desc = None
        parts.append(desc or "csm:0x%04X(%dB)" % (msg_id, length))
        rest = rest[length:]
    if not parts:
        return _short_payload(p)
    if rest:
        parts.append("+%s" % (_short_payload(rest),))
    return " ".join(parts)


class IAP2Packet:
    def __init__(self, data: bytes, psn: int = None, session_id: int = 0):
        self.psn = psn
        self.data = data
        self.session_id = session_id


class IAP2Stream:
    def __init__(self, conn: "IAP2Connection", session_id: int, stream_id: int = None):
        self.conn = conn
        self.session_id = session_id
        self.stream_id = stream_id
        self.out_buffer = bytearray()
        self.in_buffer = bytearray()
        self.in_waiter_fut = None
        self.in_waiter_count = None
        if self.stream_id != None:
            self.out_buffer += EA_SESSION_ID_STRUCT.pack(self.stream_id)
        self.closed = False

    def write(self, data):
        if self.closed:
            raise IOError("closed")
        if len(self.out_buffer) == 0:
            self.out_buffer = data
        else:
            self.out_buffer += data
        while len(self.out_buffer) >= self.conn.lsp.max_len and self.conn.write_allowed_event.is_set():
            self.conn.send_packet(
                IAP2Packet(self.out_buffer[:self.conn.lsp.max_len],
                           session_id=self.session_id))
            del self.out_buffer[:self.conn.lsp.max_len]

    async def drain(self):
        if self.closed:
            raise IOError("closed")
        if len(self.out_buffer) == 0:
            return
        await self.conn.write_allowed_event.wait()
        self.conn.send_packet(
            IAP2Packet(self.out_buffer, session_id=self.session_id))
        self.out_buffer = bytearray()
        if self.stream_id != None:
            self.out_buffer += EA_SESSION_ID_STRUCT.pack(self.stream_id)

    def received_data(self, data):
        self.in_buffer += data
        if self.in_waiter_fut and self.in_waiter_count <= len(self.in_buffer):
            self.in_waiter_fut.set_result(True)
            self.in_waiter_fut = None

    async def readexactly(self, nbytes):
        if self.in_waiter_fut:
            return

        if len(self.in_buffer) < nbytes:
            if self.closed:
                raise asyncio.exceptions.IncompleteReadError(partial=self.in_buffer, expected=nbytes)
            self.in_waiter_count = nbytes
            fut = self.conn._loop.create_future()
            self.in_waiter_fut = fut
            await fut
            if self.closed:
                raise asyncio.exceptions.IncompleteReadError(partial=self.in_buffer, expected=nbytes)

        d = self.in_buffer[:nbytes]
        del self.in_buffer[:nbytes]
        return d

    def feed_eof(self):
        self.closed = True
        if self.in_waiter_fut:
            self.in_waiter_fut.set_result(True)
            self.in_waiter_fut = None


class IAP2Connection:
    CONTROL_SESSION_ID = 10
    EA_SESSION_ID = 11
    FILE_TRANSFER_SESSION_ID = 12

    def __init__(self,
                 output: asyncio.StreamWriter,
                 input: asyncio.StreamReader,
                 loop: asyncio.AbstractEventLoop = asyncio.get_event_loop(),
                 max_outgoing: int = 30,
                 max_outgoing_delta: int = 0,
                 ack_timeout=500,
                 on_error: Callable[[Any], None] = None,
                 zero_ack: bool = False,
                 control_version: int = 1,
                 tag: str = "bt"):
        self._tag = tag
        self.on_error = on_error
        self.state = None
        self.lsp = LinkSynchronizationPayload(
            max_outgoing=max_outgoing,
            max_len=65535,
            retransmission_timeout=0 if zero_ack else 4000,
            ack_timeout=0 if zero_ack else ack_timeout,
            max_retransmissions=0 if zero_ack else 4,
            max_ack=0 if zero_ack else 3,
            sessions=[
                LSPSession(id=IAP2Connection.CONTROL_SESSION_ID,
                           type=0,
                           version=control_version),
                LSPSession(id=IAP2Connection.EA_SESSION_ID, type=2, version=1),
                LSPSession(id=IAP2Connection.FILE_TRANSFER_SESSION_ID, type=1, version=2),
            ])
        self._max_outgoing_delta = max_outgoing_delta
        self._sent_psn = 99
        self._last_sent_acknowledged_psn = None
        self._unack_packets = []
        self._queued_packets = []

        self._last_received_in_sequence_psn = 0
        self._last_acked_psn = None
        self._initial_received_psn = None
        self._received_out_of_sequence = []
        self._cumulative_received = 0
        self._loop = loop
        self._output = output
        self._input = input
        self._send_ack_timer = None
        self._recv_ack_timer = None
        self.write_allowed_event = asyncio.Event()
        self.control_session = IAP2Stream(self,
                                          IAP2Connection.CONTROL_SESSION_ID)
        self.ea_streams = dict()
        self.on_file_transfer = None
        self._receive_loop_task = None

    def create_ea_stream(self, stream_id):
        stream = IAP2Stream(self, IAP2Connection.EA_SESSION_ID, stream_id)
        self.ea_streams[stream_id] = stream
        return stream

    def send_file_transfer(self, data):
        self.send_packet(
            IAP2Packet(bytes(data),
                       session_id=IAP2Connection.FILE_TRANSFER_SESSION_ID))

    def start(self, initiate_negotiate=False):
        if self.state:
            return
        self._receive_loop_task = self._loop.create_task(self._receive_loop(initiate_negotiate))
        self.state = STATE_DETECT_IAP2_SUPPORT
        self._send_detect_iap2_support()
        if initiate_negotiate:
            self.state = STATE_NEGOTIATE
            self._send_negotiate()

    def close(self):
        self._input.feed_eof()

    def _write_packet(self, payload=None, seq=0, control=0, session_id=0):
        self._cumulative_received = 0
        if payload:
            length = len(payload) + 10
        else:
            length = 9
        header = LinkPacketHeader(control=control,
                                  length=length,
                                  seq=seq,
                                  ack=self._last_received_in_sequence_psn,
                                  session_id=session_id)
        if DEBUG:
            print(self._tag, ">", header, _describe_payload(payload))
        header_bytes = header.pack()
        if payload:
            self._output.write(header_bytes + payload +
                               bytes([gen_checksum(payload)]))

        else:
            self._output.write(header_bytes)

    def _send_ack(self):
        self._write_packet(seq=self._sent_psn, control=CONTROL_ACK)

    def _send_eak(self, num):
        self._write_packet(bytes(num), seq=self._sent_psn, control=CONTROL_EAK)

    def _send_data(self, p):
        self._write_packet(p.data,
                           seq=p.psn,
                           control=CONTROL_ACK,
                           session_id=p.session_id)

    def _send_detect_iap2_support(self):
        if self.state != STATE_DETECT_IAP2_SUPPORT:
            return
        self._output.write(IAP2_MARKER)
        self._loop.call_later(1, self._send_detect_iap2_support)

    def _send_negotiate(self):
        if self.state != STATE_NEGOTIATE:
            return
        lsp_bytes = self.lsp.pack()
        self._write_packet(lsp_bytes, self._sent_psn, CONTROL_SYN)
        self._loop.call_later(0.5, self._send_negotiate)

    async def _receive_loop(self, initiate_negotiate=False):
        try:
            if not initiate_negotiate:
                recv_marker = await self._input.readexactly(len(IAP2_MARKER))
                if recv_marker != IAP2_MARKER:
                    self._bailout("IAP2 not supported")
                    return
                if hasattr(self._input, "reset"):
                    self._input.reset()
                self.state = STATE_NEGOTIATE
                self._send_negotiate()
            while True:
                header_bytes = await self._input.readexactly(9)
                while True:
                    if int(header_bytes[0]) << 8 | int(
                            header_bytes[1]) == LinkPacketHeader.start:
                        break
                    header_bytes = header_bytes[1:] + await self._input.readexactly(
                        1)
                header = LinkPacketHeader.from_bytes(header_bytes)
                if not header:
                    continue
                payload = None
                if header.length > 9:
                    payload_with_checksum = await self._input.readexactly(
                        header.length - 9)
                    if not check_checksum(payload_with_checksum):
                        continue
                    payload = payload_with_checksum[:-1]
                if DEBUG:
                    print(self._tag, "<", header, _describe_payload(payload))
                if hasattr(self._input, "reset"):
                    self._input.reset()
                if (header.control & CONTROL_RST) != 0:
                    self._bailout("device sent reset message")
                if (header.control & CONTROL_SYN) != 0:
                    lsp = LinkSynchronizationPayload.from_bytes(payload)
                    if not lsp:
                        continue
                    self._handle_syn(lsp, header.seq)
                if (header.control & CONTROL_ACK) != 0:
                    self._cumulative_received += 1
                    self._handle_ack(header.ack)
                if (header.control & CONTROL_EAK) != 0 and payload:
                    self._handle_eak([int(x) for x in payload])
                if (header.control & ~CONTROL_ACK) == 0 and payload != None:
                    self._handle_data(
                        IAP2Packet(payload, header.seq, header.session_id))
                if self.lsp.max_ack > 0 and self._cumulative_received >= self.lsp.max_ack:
                    self._cumulative_received = 0
                    self._last_acked_psn = self._last_received_in_sequence_psn
                    self._send_ack()
        except asyncio.exceptions.IncompleteReadError:
            self._bailout(None)
        except Exception as e:
            self._bailout(e)

    def _bailout(self, error):
        if self.state == STATE_DEAD:
            return
        self._disarm_send_ack_timer()
        self._disarm_recv_ack_timer()
        self.state = STATE_DEAD
        try:
            self._output.close()
        except:
            pass
        try:
            self.control_session.feed_eof()
            for stream in self.ea_streams.values():
                stream.feed_eof()
        except:
            pass
        if self._receive_loop_task:
            try:
                self._receive_loop_task.cancel()
            except:
                pass
        if error is not None and self.on_error:
            self.on_error(error)

    def send_packet(self, p: IAP2Packet):
        if distance(self._sent_psn, self._last_sent_acknowledged_psn
                    ) > self.lsp.max_outgoing or self.state != STATE_NORMAL:
            self._queued_packets.append(p)
            self.write_allowed_event.clear()
            return

        self._sent_psn = signed_add(self._sent_psn, 1)
        p.counter = 0
        p.psn = self._sent_psn
        p.timeout = self._loop.time() + self.lsp.retransmission_timeout / 1000
        self._disarm_send_ack_timer()
        self._send_data(p)
        self._last_acked_psn = self._last_received_in_sequence_psn
        if self.lsp.max_retransmissions > 0:
            self._rearm_recv_ack_timer(p.timeout)
            self._unack_packets.append(p)
        else:
            self._last_sent_acknowledged_psn = self._sent_psn

    def _handle_syn(self, lsp: LinkSynchronizationPayload, psn: int):
        if self.state != STATE_NEGOTIATE:
            return
        if DEBUG:
            print("Device:", lsp)
            print("Accessory:", self.lsp)
        self.lsp = lsp
        self._last_received_in_sequence_psn = psn
        self._last_acked_psn = psn
        self._send_ack()

    def _handle_ack(self, num: int):
        if self.state == STATE_NEGOTIATE:
            self.state = STATE_NORMAL
            self.write_allowed_event.set()
        self._last_sent_acknowledged_psn = num

        while len(self._unack_packets) != 0:
            d = distance(
                self._unack_packets[0].psn,
                self._last_sent_acknowledged_psn)
            if 0 < d <= self.lsp.max_ack + 10:
                self._rearm_recv_ack_timer(self._unack_packets[0].timeout)
                break
            else:
                del self._unack_packets[0]
        else:
            self._disarm_recv_ack_timer()

        while distance(self._sent_psn, self._last_sent_acknowledged_psn
                       ) < self.lsp.max_outgoing and len(
            self._queued_packets) > 0:
            self.send_packet(self._queued_packets.pop(0))
            self.write_allowed_event.set()

    def _on_expect_ack_timer(self):
        if len(self._unack_packets) == 0 or self.state != STATE_NORMAL:
            return
        unack_packets = sorted(self._unack_packets, key=lambda x: x.timeout)
        p = unack_packets[0]
        p.timeout = self._loop.time() + self.lsp.retransmission_timeout / 1000
        p.counter += 1
        if p.counter == self.lsp.max_retransmissions:
            self._bailout(p)
            return
        self._send_data(p)
        self._rearm_recv_ack_timer(unack_packets[0 if len(unack_packets) == 1 else 1].timeout)

    def _handle_eak(self, nums: List[int]):
        if self.state != STATE_NORMAL:
            return
        for p in self._unack_packets:
            if p.psn in nums:
                p.counter += 1
                if p.counter == self.lsp.max_retransmissions:
                    self._bailout(p)
                    continue
                self._send_data(p)
                self._disarm_send_ack_timer()
                self._rearm_recv_ack_timer(p.timeout)

    def _on_send_ack_timer(self):
        if self.state != STATE_NORMAL:
            return
        self._last_acked_psn = self._last_received_in_sequence_psn
        self._send_ack()

    def _handle_data(self, p: IAP2Packet):
        d = distance(p.psn, self._last_received_in_sequence_psn)
        if d > self.lsp.max_outgoing + 10 or d == 0:
            self._send_ack()
            return

        if d > 1:
            self._received_out_of_sequence.append(p)
            if d >= self.lsp.max_outgoing:
                eak = []
                x = self._last_received_in_sequence_psn
                while distance(p.psn, x) > 1:
                    x = signed_add(x, 1)
                    eak.append(x)
                self._disarm_send_ack_timer()
                self._send_eak(eak)
            return

        self._received_out_of_sequence.append(p)
        for pp in sorted(self._received_out_of_sequence,
                         key=lambda x: distance(
                             x.psn, self._last_received_in_sequence_psn)):
            if distance(pp.psn, self._last_received_in_sequence_psn) > 1:
                break
            self._received_data(pp)
            self._last_received_in_sequence_psn = pp.psn
            self._received_out_of_sequence.remove(pp)

        if self.lsp.max_ack <= 0:
            pass
        elif distance(self._last_received_in_sequence_psn, self._last_acked_psn
                      ) >= self.lsp.max_outgoing - self._max_outgoing_delta:
            self._disarm_send_ack_timer()
            self._last_acked_psn = self._last_received_in_sequence_psn
            self._send_ack()
        else:
            self._rearm_send_ack_timer()

    def _received_data(self, p: IAP2Packet):
        if p.session_id == IAP2Connection.CONTROL_SESSION_ID:
            self.control_session.received_data(p.data)
        elif p.session_id == IAP2Connection.EA_SESSION_ID and len(p.data) >= 2:
            stream_id = EA_SESSION_ID_STRUCT.unpack(p.data[:2])[0]
            stream = self.ea_streams.get(stream_id)
            if stream:
                stream.received_data(p.data[2:])
        elif p.session_id == IAP2Connection.FILE_TRANSFER_SESSION_ID:
            if self.on_file_transfer:
                self.on_file_transfer(bytes(p.data))

    def _disarm_send_ack_timer(self):
        if self._send_ack_timer:
            self._send_ack_timer.cancel()
            self._send_ack_timer = None

    def _rearm_send_ack_timer(self):
        if self._send_ack_timer:
            self._send_ack_timer.cancel()
        self._send_ack_timer = self._loop.call_later(self.lsp.ack_timeout / 1000,
                                                     self._on_send_ack_timer)

    def _disarm_recv_ack_timer(self):
        if self._recv_ack_timer:
            self._recv_ack_timer.cancel()
            self._recv_ack_timer = None

    def _rearm_recv_ack_timer(self, time):
        if self._recv_ack_timer:
            self._recv_ack_timer.cancel()
        self._recv_ack_timer = self._loop.call_at(time, self._on_expect_ack_timer)


def distance(a: int, b: int):
    if b is None:
        return 0
    elif a >= b:
        return a - b
    else:
        return a + 256 - b
