from dataclasses import dataclass
from typing import Annotated

from iap2.control_session_message import csm, NoneLike


@csm(0xFFFA)
@dataclass
class StartLocationInformation:
    gps_fix_data: Annotated[NoneLike, 1] = None
    recommended_minimum: Annotated[NoneLike, 2] = None
    satellites_in_view: Annotated[NoneLike, 3] = None
    vehicle_speed: Annotated[NoneLike, 4] = None


@csm(0xFFFB)
@dataclass
class LocationInformation:
    nmea_sentence: Annotated[str, 0] = ""


@csm(0xFFFC)
class StopLocationInformation:
    pass
