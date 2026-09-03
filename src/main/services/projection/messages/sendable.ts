import {
  CommandMapping,
  type CommandValue,
  type MultiTouchAction,
  type TouchAction
} from '@shared/types/ProjectionEnums'
import type { MultiTouchPoint } from '@shared/types/TouchTypes'

/**
 * Driver-agnostic commands towards the phone. Every driver interprets these
 * in its own protocol; the dongle's wire encoding lives in
 * driver/dongle/protocol.
 */
export abstract class SendableMessage {}

export class SendCommand extends SendableMessage {
  value: CommandMapping

  constructor(value: CommandValue) {
    super()
    this.value = CommandMapping[value]
  }
}

export class SendBluetoothPairedList extends SendableMessage {
  readonly listText: string

  constructor(listText: string) {
    super()
    this.listText = listText
  }
}

export class SendTouch extends SendableMessage {
  x: number
  y: number
  action: TouchAction

  constructor(x: number, y: number, action: TouchAction) {
    super()
    this.x = x
    this.y = y
    this.action = action
  }
}

export class TouchItem {
  x: number
  y: number
  action: MultiTouchAction
  id: number

  constructor(x: number, y: number, action: MultiTouchAction, id: number) {
    this.x = x
    this.y = y
    this.action = action
    this.id = id
  }
}

export class SendMultiTouch extends SendableMessage {
  touches: TouchItem[]

  constructor(points: MultiTouchPoint[]) {
    super()
    this.touches = points.map((p) => new TouchItem(p.x, p.y, p.action, p.id))
  }
}

export class SendCloseDongle extends SendableMessage {}

export class SendDisconnectPhone extends SendableMessage {}

export class SendAutoConnectByBtAddress extends SendableMessage {
  readonly btMac: string

  constructor(btMac: string) {
    super()
    this.btMac = btMac
  }
}

export class SendForgetBluetoothAddr extends SendableMessage {
  readonly btMac: string

  constructor(btMac: string) {
    super()
    this.btMac = btMac
  }
}

export class SendClusterFocusRequest extends SendableMessage {}

export class SendClusterFocusRelease extends SendableMessage {}
