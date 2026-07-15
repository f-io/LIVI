from dataclasses import dataclass
from enum import IntEnum
from typing import Optional, Annotated

from iap2.control_session_message import csm, Uint8


@csm(0x5700)
class RequestWiFiInformation:
    pass


class WiFiRequestStatus(IntEnum):
    SUCCESS = 0
    USER_DECLINED = 1
    NET_WORK_INFORMATION_UNAVAILABLE = 2


@csm(0x5701)
class WiFiInformation:
    status: WiFiRequestStatus
    ssid: Optional[str]
    passphrase: Optional[str]


@csm(0x5702)
class RequestAccessoryWiFiConfigurationInformation:
    pass


class SecurityType(IntEnum):
    NONE = 0
    WEP = 1
    WPA_WPA2 = 2
    WPA3_TRANSITION = 3
    WPA3_ONLY = 4

@csm(0x5703)
@dataclass
class AccessoryWiFiConfigurationInformation:
    ssid: Annotated[Optional[str], 1]
    passphrase: Annotated[Optional[str], 2]
    security_type: Annotated[SecurityType, 3]
    channel: Annotated[Uint8, 4]
