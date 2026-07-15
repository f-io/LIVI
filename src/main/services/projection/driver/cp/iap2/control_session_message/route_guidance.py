from dataclasses import dataclass
from typing import Annotated, Optional

from iap2.control_session_message import csm, Int16, Uint8, Uint16, Uint32, Uint64


@csm(0x5200)
@dataclass
class StartRouteGuidanceUpdates:
    display_component_id: Annotated[Optional[Uint16], 0] = None


@csm(0x5201)
@dataclass
class RouteGuidanceUpdate:
    display_component_id: Annotated[Optional[Uint16], 0] = None
    state: Annotated[Optional[Uint8], 1] = None
    maneuver_state: Annotated[Optional[Uint8], 2] = None
    current_road_name: Annotated[Optional[str], 3] = None
    destination_name: Annotated[Optional[str], 4] = None
    eta: Annotated[Optional[Uint64], 5] = None
    time_remaining: Annotated[Optional[Uint64], 6] = None
    distance_remaining: Annotated[Optional[Uint32], 7] = None
    distance_to_maneuver: Annotated[Optional[Uint32], 10] = None
    current_maneuver_list: Annotated[Optional[bytes], 13] = None


@csm(0x5202)
@dataclass
class RouteGuidanceManeuverUpdate:
    display_component_id: Annotated[Optional[Uint16], 0] = None
    index: Annotated[Optional[Uint16], 1] = None
    maneuver_type: Annotated[Optional[Uint8], 3] = None
    after_maneuver_road_name: Annotated[Optional[str], 4] = None
    driving_side: Annotated[Optional[Uint8], 8] = None
    junction_type: Annotated[Optional[Uint8], 9] = None
    exit_angle: Annotated[Optional[Int16], 11] = None


@csm(0x5203)
@dataclass
class StopRouteGuidanceUpdates:
    display_component_id: Annotated[Optional[Uint16], 0] = None
