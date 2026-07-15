from dataclasses import dataclass
from typing import Annotated, Optional

from iap2.control_session_message import csm, Int16, Uint16


@csm(0xA100)
class StartVehicleStatusUpdates:
    pass


@csm(0xA101)
@dataclass
class VehicleStatusUpdate:
    range: Annotated[Optional[Uint16], 3] = None
    outside_temperature: Annotated[Optional[Int16], 4] = None
    range_warning: Annotated[Optional[bool], 6] = None


@csm(0xA102)
class StopVehicleStatusUpdates:
    pass
