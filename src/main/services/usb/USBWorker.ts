import { parentPort } from 'worker_threads'
import { findDongle } from './helpers'

if (!parentPort) throw new Error('No parent port found')

type IncomingMsg = 'check-dongle'
type OutgoingMsg =
  | { type: 'dongle-status'; connected: true; vendorId: number; productId: number }
  | { type: 'dongle-status'; connected: false }

parentPort.on('message', (msg: IncomingMsg) => {
  if (msg !== 'check-dongle') return

  void findDongle().then((dongle) => {
    const response: OutgoingMsg = dongle
      ? {
          type: 'dongle-status',
          connected: true,
          vendorId: dongle.vendorId,
          productId: dongle.productId
        }
      : { type: 'dongle-status', connected: false }

    parentPort!.postMessage(response)
  })
})
