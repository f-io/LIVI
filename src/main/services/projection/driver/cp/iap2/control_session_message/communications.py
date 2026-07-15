from dataclasses import dataclass
from typing import Annotated, Optional

from iap2.control_session_message import csm, NoneLike, Uint8


@csm(0x4154)
@dataclass
class StartCallStateUpdates:
    remote_id: Annotated[NoneLike, 0] = None
    display_name: Annotated[NoneLike, 1] = None
    status: Annotated[NoneLike, 2] = None
    direction: Annotated[NoneLike, 3] = None
    call_uuid: Annotated[NoneLike, 4] = None
    address_book_id: Annotated[NoneLike, 6] = None
    label: Annotated[NoneLike, 7] = None
    service: Annotated[NoneLike, 8] = None
    is_conferenced: Annotated[NoneLike, 9] = None
    conference_group: Annotated[NoneLike, 10] = None
    disconnect_reason: Annotated[NoneLike, 11] = None
    start_timestamp: Annotated[NoneLike, 12] = None


@csm(0x4155)
@dataclass
class CallStateUpdate:
    remote_id: Annotated[Optional[str], 0] = None
    display_name: Annotated[Optional[str], 1] = None
    status: Annotated[Optional[Uint8], 2] = None
    direction: Annotated[Optional[Uint8], 3] = None
    call_uuid: Annotated[Optional[str], 4] = None
    disconnect_reason: Annotated[Optional[Uint8], 11] = None


@csm(0x4156)
class StopCallStateUpdates:
    pass


@csm(0x4157)
@dataclass
class StartCommunicationsUpdates:
    signal_strength: Annotated[NoneLike, 0] = None
    registration_status: Annotated[NoneLike, 1] = None
    airplane_mode_status: Annotated[NoneLike, 2] = None
    carrier_name: Annotated[NoneLike, 4] = None
    cellular_supported: Annotated[NoneLike, 5] = None


@csm(0x4158)
@dataclass
class CommunicationsUpdate:
    signal_strength: Annotated[Optional[bytes], 0] = None
    registration_status: Annotated[Optional[bytes], 1] = None
    airplane_mode_status: Annotated[Optional[bool], 2] = None
    carrier_name: Annotated[Optional[str], 4] = None
    cellular_supported: Annotated[Optional[bool], 5] = None


@csm(0x4159)
class StopCommunicationsUpdates:
    pass
