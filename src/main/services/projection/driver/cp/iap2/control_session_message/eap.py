from enum import IntEnum

from iap2.control_session_message import csm, Uint16, Uint8


@csm(0xEA00)
class StartExternalAccessoryProtocolSession:
    protocol_id: Uint8
    session_id: Uint16


@csm(0xEA01)
class StopExternalAccessoryProtocolSession:
    session_id: Uint16


class SessionStatus(IntEnum):
    OK = 0
    CLOSE = 1


@csm(0xEA03)
class StatusExternalAccessoryProtocolSession:
    session_id: Uint16
    status: SessionStatus
