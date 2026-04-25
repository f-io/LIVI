/**
 * Protobuf loader for the Android Auto protocol.
 *
 * Uses the modern aasdk/opencardev aap_protobuf proto tree for all control-plane
 * messages (SDR, ChannelOpen, Ping, AuthResponse) and the Service-wrapped channel
 * descriptors.
 *
 * AV/media setup messages still use the legacy oaa/ tree during phase-1
 * migration; they are wire-compatible (same field numbers/types) and will be
 * migrated to aap_protobuf shared.message.Setup/Config/Start/Ack in phase 2.
 *
 * Proto root resolution probes packaged → bundle-local → source paths, so the
 * unmodified .proto tree from `src/.../driver/aa/protos/` works in dev, in an
 * asar-packed AppImage, and in unpacked deb installs. We don't carry a sibling
 * mirror at the project root any more — the bundle's own `protos/` (written by
 * the `livi:copy-aa-resources` Vite plugin) is the canonical runtime location.
 */

import { existsSync } from 'node:fs'
import * as path from 'node:path'
import protobuf from 'protobufjs'

// CJS __dirname — points at out/main/ in dev/prod after bundling.
const _dir = __dirname

/**
 * Pick the first candidate directory that actually contains the proto tree.
 *  1. `${process.resourcesPath}/aa/protos` — extraResources copy (outside asar)
 *      if/when we ever ship the protos as extraResources. Cheap to keep as a
 *      first probe; today it usually doesn't exist and we fall through.
 *  2. `${_dir}/protos` — Vite plugin output. In dev this is `out/main/protos`.
 *      In a packaged AppImage `_dir` lives inside `app.asar`, so this is
 *      `app.asar/out/main/protos`. Electron's asar fs shim makes that work.
 *  3. `${_dir}/../../../src/main/services/projection/driver/aa/protos` —
 *      dev fallback if the plugin hasn't copied yet (e.g. running tsx directly
 *      against source without going through Vite).
 */
function resolveProtoRoot(): string {
  const candidates: string[] = []
  if (typeof process.resourcesPath === 'string' && process.resourcesPath.length > 0) {
    candidates.push(path.join(process.resourcesPath, 'aa', 'protos'))
  }
  candidates.push(path.join(_dir, 'protos'))
  // Dev fallback: from out/main/ up two levels to the project root, then into
  // the canonical source tree. Useful when running tsx directly against the
  // sources without going through the Vite plugin.
  candidates.push(path.resolve(_dir, '..', '..', 'src/main/services/projection/driver/aa/protos'))

  for (const c of candidates) {
    // Cheap presence check — `aap_protobuf/` is the first subtree the loader
    // touches, so its existence is a reliable smoke test for "this path is
    // actually the proto root and not a stale shell".
    if (existsSync(path.join(c, 'aap_protobuf'))) return c
  }
  // Fall back to candidate #2 even if the smoke test failed; the proto loader
  // will throw a clearer error than us guessing.
  return candidates[1]
}

const PROTO_ROOT = resolveProtoRoot()

export interface ProtoTypes {
  // ── Control channel (aap_protobuf — aasdk) ───────────────────────────────
  /** aap_protobuf.service.control.message.ServiceDiscoveryResponse — Service-wrapped channels */
  ServiceDiscoveryResponse: protobuf.Type
  /** aap_protobuf.service.control.message.ServiceDiscoveryRequest */
  ServiceDiscoveryRequest: protobuf.Type
  /** aap_protobuf.service.control.message.ChannelOpenRequest ({ priority, service_id }) */
  ChannelOpenRequest: protobuf.Type
  /** aap_protobuf.service.control.message.ChannelOpenResponse ({ status: MessageStatus }) */
  ChannelOpenResponse: protobuf.Type
  /** aap_protobuf.service.control.message.PingRequest */
  PingRequest: protobuf.Type
  /** aap_protobuf.service.control.message.PingResponse */
  PingResponse: protobuf.Type
  /** aap_protobuf.service.control.message.AuthResponse (was oaa AuthCompleteIndication) */
  AuthResponse: protobuf.Type
  /** Alias kept for call-site compatibility (identical wire format: field 1 int32 status) */
  AuthCompleteIndication: protobuf.Type
  /** aap_protobuf.service.Service — channel descriptor wrapper */
  Service: protobuf.Type
  /** Service sub-messages we may need for manual construction */
  MediaSinkService: protobuf.Type
  MediaSourceService: protobuf.Type
  SensorSourceService: protobuf.Type
  InputSourceService: protobuf.Type
  BluetoothService: protobuf.Type
  NavigationStatusService: protobuf.Type
  MediaPlaybackStatusService: protobuf.Type
  PhoneStatusService: protobuf.Type

  // ── AV channels (still oaa — wire-compatible, migrate in phase 2) ────────
  AVChannelSetupRequest: protobuf.Type
  AVChannelSetupResponse: protobuf.Type
  AVChannelStartIndication: protobuf.Type
  AVMediaAckIndication: protobuf.Type

  // ── Misc control (still oaa — no aasdk equivalent or only used as bytes) ─
  /** Binding request/response still on oaa until aap_protobuf equivalent is confirmed */
  BindingRequest: protobuf.Type
  BindingResponse: protobuf.Type
}

let _cached: ProtoTypes | null = null

export async function loadProtos(): Promise<ProtoTypes> {
  if (_cached) return _cached

  const root = new protobuf.Root()

  // Unified resolver: both aap_protobuf/... and oaa/... relative imports inside
  // .proto files resolve below PROTO_ROOT.
  root.resolvePath = (_origin: string, target: string) =>
    path.isAbsolute(target) ? target : path.join(PROTO_ROOT, target)

  await root.load([
    // --- aasdk aap_protobuf (control plane + Service descriptor tree) -----
    // ServiceDiscoveryResponse transitively imports Service.proto, which in
    // turn imports every service message (MediaSink, MediaSource, Sensor,
    // Input, Bluetooth, NavStatus, MediaPlayback, PhoneStatus, Radio,
    // Vendor, GenericNotification, WifiProjection, MediaBrowser).
    path.join(PROTO_ROOT, 'aap_protobuf/service/control/message/ServiceDiscoveryResponse.proto'),
    path.join(PROTO_ROOT, 'aap_protobuf/service/control/message/ServiceDiscoveryRequest.proto'),
    path.join(PROTO_ROOT, 'aap_protobuf/service/control/message/ChannelOpenRequest.proto'),
    path.join(PROTO_ROOT, 'aap_protobuf/service/control/message/ChannelOpenResponse.proto'),
    path.join(PROTO_ROOT, 'aap_protobuf/service/control/message/PingRequest.proto'),
    path.join(PROTO_ROOT, 'aap_protobuf/service/control/message/PingResponse.proto'),
    path.join(PROTO_ROOT, 'aap_protobuf/service/control/message/AuthResponse.proto'),

    // --- legacy oaa/ AV setup + misc (wire-compatible with aap_protobuf) --
    path.join(PROTO_ROOT, 'oaa/av/AVChannelSetupRequestMessage.proto'),
    path.join(PROTO_ROOT, 'oaa/av/AVChannelSetupResponseMessage.proto'),
    path.join(PROTO_ROOT, 'oaa/av/AVChannelStartIndicationMessage.proto'),
    path.join(PROTO_ROOT, 'oaa/av/AVMediaAckIndicationMessage.proto'),
    path.join(PROTO_ROOT, 'oaa/control/BindingRequestMessage.proto'),
    path.join(PROTO_ROOT, 'oaa/control/BindingResponseMessage.proto')
  ])

  // aap_protobuf namespace shorthand: aap_protobuf.service.control.message.*
  const ctrl = 'aap_protobuf.service.control.message'
  const svc = 'aap_protobuf.service'
  const authResponse = root.lookupType(`${ctrl}.AuthResponse`)

  _cached = {
    ServiceDiscoveryResponse: root.lookupType(`${ctrl}.ServiceDiscoveryResponse`),
    ServiceDiscoveryRequest: root.lookupType(`${ctrl}.ServiceDiscoveryRequest`),
    ChannelOpenRequest: root.lookupType(`${ctrl}.ChannelOpenRequest`),
    ChannelOpenResponse: root.lookupType(`${ctrl}.ChannelOpenResponse`),
    PingRequest: root.lookupType(`${ctrl}.PingRequest`),
    PingResponse: root.lookupType(`${ctrl}.PingResponse`),
    AuthResponse: authResponse,
    AuthCompleteIndication: authResponse, // alias — identical wire format
    Service: root.lookupType(`${svc}.Service`),
    MediaSinkService: root.lookupType(`${svc}.media.sink.MediaSinkService`),
    MediaSourceService: root.lookupType(`${svc}.media.source.MediaSourceService`),
    SensorSourceService: root.lookupType(`${svc}.sensorsource.SensorSourceService`),
    InputSourceService: root.lookupType(`${svc}.inputsource.InputSourceService`),
    BluetoothService: root.lookupType(`${svc}.bluetooth.BluetoothService`),
    NavigationStatusService: root.lookupType(`${svc}.navigationstatus.NavigationStatusService`),
    MediaPlaybackStatusService: root.lookupType(`${svc}.mediaplayback.MediaPlaybackStatusService`),
    PhoneStatusService: root.lookupType(`${svc}.phonestatus.PhoneStatusService`),

    AVChannelSetupRequest: root.lookupType('oaa.proto.messages.AVChannelSetupRequest'),
    AVChannelSetupResponse: root.lookupType('oaa.proto.messages.AVChannelSetupResponse'),
    AVChannelStartIndication: root.lookupType('oaa.proto.messages.AVChannelStartIndication'),
    AVMediaAckIndication: root.lookupType('oaa.proto.messages.AVMediaAckIndication'),
    BindingRequest: root.lookupType('oaa.proto.messages.BindingRequest'),
    BindingResponse: root.lookupType('oaa.proto.messages.BindingResponse')
  }

  return _cached
}

/** Encode a protobuf message to a Buffer. */
export function encode(type: protobuf.Type, fields: Record<string, unknown>): Buffer {
  const err = type.verify(fields)
  if (err) throw new Error(`Proto encode error [${type.name}]: ${err}`)
  return Buffer.from(type.encode(type.create(fields)).finish())
}

/** Decode a Buffer into a plain JS object. */
export function decode(type: protobuf.Type, buf: Buffer): Record<string, unknown> {
  return type.toObject(type.decode(buf), { longs: Number, enums: Number, defaults: false })
}
