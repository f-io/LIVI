# livi-aa-proto

LIVI's description of the Android Auto wire protocol, as protobuf definitions in `proto/` and the Rust types prost builds from them.

Copyright (C) 2026 Lasse Heitgres, licensed under the GNU General Public License v3.0 or later.

## Where it comes from

The definitions are our own work, written from our analysis of the Android Auto app 17.6.663454:

- Structure comes straight from the message schemas the app parses with: field numbers, types, labels, repeated and packed, oneofs, proto2 or proto3.
- Enum values are the numbers the app accepts, with the app's own value names where it has them.
- Message ids per channel are the ones the app sends and parses, named as the app names them where it does.
- Names of messages and fields say what the app does with them. A field the app carries but never reads is named `unknown_<number>` until we know better.

Only the current protocol is described. Fields and messages the app no longer has are not part of it.

## Layout

One package, `livi.aa`, one file per service: `control`, `service_discovery`, `media`, `video`, `audio`, `input`, `sensor`, `navigation`, `media_playback`, `phone_status`, `radio`, `bluetooth`, `wifi_projection`, `wireless_setup` (the Bluetooth bootstrap before the TCP session), `car_control`, `car_property`, `car_local_media`, `buffered_media`, `car_intent`, `vehicle_energy`, `notification`, `media_browser`, `vendor_extension`, plus `common` and `duration` for shared types.

Every enum value starts with its enum's name, so prost strips it: `MediaMessageId::Start`, `SensorType::Location`.
