from dataclasses import dataclass
from typing import Annotated, Optional

from iap2.control_session_message import csm, NoneLike, Uint8, Uint16


@csm(0xAE00)
@dataclass
class StartPowerUpdates:
    maximum_current_drawn_from_accessory: Annotated[NoneLike, 0] = None
    device_battery_will_charge_if_power_is_present: Annotated[NoneLike, 1] = None
    accessory_power_mode: Annotated[NoneLike, 2] = None
    is_external_charger_connected: Annotated[NoneLike, 4] = None
    battery_charging_state: Annotated[NoneLike, 5] = None
    battery_charge_level: Annotated[NoneLike, 6] = None


@csm(0xAE01)
@dataclass
class PowerUpdate:
    maximum_current_drawn_from_accessory: Annotated[Optional[Uint16], 0] = None
    device_battery_will_charge_if_power_is_present: Annotated[Optional[bool], 1] = None
    accessory_power_mode: Annotated[Optional[Uint8], 2] = None
    is_external_charger_connected: Annotated[Optional[bool], 4] = None
    battery_charging_state: Annotated[Optional[Uint8], 5] = None
    battery_charge_level: Annotated[Optional[Uint16], 6] = None


@csm(0xAE02)
class StopPowerUpdates:
    pass


@csm(0xAE03)
@dataclass
class PowerSourceUpdate:
    available_current_for_device: Annotated[Optional[Uint16], 0] = None
    device_battery_should_charge_if_power_is_present: Annotated[Optional[bool], 1] = None
