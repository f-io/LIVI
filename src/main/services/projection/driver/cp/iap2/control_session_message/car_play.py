from dataclasses import dataclass, field
from enum import IntEnum
from typing import Annotated, List, Optional

from iap2.control_session_message import csm, Uint16, Uint8, Uint32


@csm(0x4E0E)
@dataclass
class DeviceTransportIdentifierNotification:
    bluetooth_transport_id: str
    usb_transport_id: str


class WirelessCarPlayStatus(IntEnum):
    UNAVAILABLE = 0
    AVAILABLE = 1


@csm(0x4E0D)
@dataclass
class WirelessCarPlayUpdate:
    status: WirelessCarPlayStatus


@dataclass
class CarPlayAvailabilityWiredAttributes:
    available: Annotated[Optional[bool], 0] = None
    usb_transport_identifier: Annotated[Optional[str], 1] = None


@dataclass
class CarPlayAvailabilityWirelessAttributes:
    available: Annotated[Optional[bool], 0] = None
    bluetooth_transport_identifier: Annotated[Optional[str], 1] = None


@csm(0x4300)
@dataclass
class CarPlayAvailability:
    wired_attributes: Annotated[Optional[CarPlayAvailabilityWiredAttributes], 0] = None
    wireless_attributes: Annotated[Optional[CarPlayAvailabilityWirelessAttributes], 1] = None


@dataclass
class CarPlayStartSessionWiredAttributes:
    ip_address: Annotated[List[str], 0] = field(default_factory=list)


@csm(0x4301)
@dataclass
class CarPlayStartSession:
    wired_attributes: Annotated[Optional[CarPlayStartSessionWiredAttributes], 0] = None
    port: Annotated[Optional[Uint32], 2] = None
    device_identifier: Annotated[Optional[str], 3] = None
    public_key: Annotated[Optional[str], 4] = None
    source_version: Annotated[Optional[str], 5] = None
