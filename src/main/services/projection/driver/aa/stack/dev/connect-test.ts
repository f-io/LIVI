/**
 * Standalone connection tester for the aa-native stack.
 *
 * Usage:
 *   npm run test:connect
 *   # or with overrides:
 *   AA_PORT=5277 AA_NAME=LIVI-dev npm run test:connect
 *
 * What it does:
 *   1. Starts TcpServer on port 5277
 *   2. Waits for a phone to connect (after BT RFCOMM handshake via aa-bluetooth.py)
 *   3. Drives the full protocol handshake (SDR, channel opens)
 *   4. Prints the first 30 H.264 frames and exits
 *
 * Connection flow:
 *   BT RFCOMM handshake → phone connects TCP (permanent connection).
 *   After WiFi credentials are established via RFCOMM, the TCP connection
 *   stays alive permanently. We never close it — the phone controls its lifetime.
 *
 * Requires aa-bluetooth.py running (for RFCOMM handshake).
 *
 * Environment (all optional):
 *   AA_PORT      — TCP port override (default: 5277)
 *   AA_NAME      — head-unit display name (default: LIVI-dev)
 *   AA_WIFI_SSID — WiFi AP SSID (sent in WifiCredentialsResponse for persistent pairing)
 *   AA_WIFI_PASS — WiFi AP password
 */

import * as net from 'node:net'
import { AAStack } from '../index.js'
import type { Session } from '../session/Session.js'
import { detectBtMac, detectWifiBssid } from '../system/hwaddr.js'

// ── IPC: notify aa-bluetooth.py of session lifecycle events ──────────────────
const AA_EVENT_SOCK = '/tmp/aa-bt.sock'

function notifyBtManager(event: string): void {
  const client = net.createConnection(AA_EVENT_SOCK, () => {
    client.write(event + '\n')
    client.end()
  })
  client.on('error', (err) => {
    // Suppress — bt manager may not be running (standalone mode)
    console.debug(`[connect-test] bt-manager notify skipped: ${err.message}`)
  })
}

const port = parseInt(process.env['AA_PORT'] ?? '5277', 10)
const huName = process.env['AA_NAME'] ?? 'LIVI'
// Auto-detected from /sys/class/bluetooth/hci*/address and /sys/class/net/wlan*/address.
// Override via env vars: AA_BT_MAC=XX:XX:XX:XX:XX:XX  AA_WIFI_BSSID=XX:XX:XX:XX:XX:XX
const btMacAddress = detectBtMac()
const wifiBssid = detectWifiBssid()
// WiFi AP credentials — sent to phone via WifiCredentialsResponse so the car is
// registered in "previously connected cars". Required for persistent pairing.
// Defaults read from bt/hostapd.conf (ssid=LIVI, wpa_passphrase=LiveInTheCar1!)
// Override via env vars: AA_WIFI_SSID=MyHotspot  AA_WIFI_PASS=mypassword
const wifiSsid = process.env['AA_WIFI_SSID'] ?? 'LIVI'
const wifiPassword = process.env['AA_WIFI_PASS'] ?? 'LiveInTheCar1!'

console.log(`[connect-test] Starting AA native stack`)
console.log(`  Port      : ${port}`)
console.log(`  HU name   : ${huName}`)
console.log(`  BT MAC    : ${btMacAddress ?? '(not detected)'}`)
console.log(`  WiFi BSSID: ${wifiBssid ?? '(not detected)'}`)
console.log(`  WiFi SSID : ${wifiSsid || '(not set — AA_WIFI_SSID env var)'}`)
console.log(`  WiFi PASS : ${wifiPassword ? '(set)' : '(not set — AA_WIFI_PASS env var)'}`)
console.log('')
console.log('[connect-test] Waiting for phone connection...')
console.log('  (complete BT RFCOMM handshake on phone first)')
console.log('')

const aa = new AAStack({
  port,
  huName,
  videoWidth: 1280,
  videoHeight: 720,
  videoDpi: 140,
  videoFps: 30,
  btMacAddress,
  wifiBssid,
  wifiSsid,
  wifiPassword
})

let sessionCount = 0

aa.on('error', (err: Error) => {
  console.error('[connect-test] Error:', err.message)
})

aa.on('session', (session: Session) => {
  sessionCount++
  const localSession = sessionCount
  let sessionWasRunning = false // true once 'connected' fires (reached RUNNING state)

  console.log(`\n[connect-test] *** Session #${localSession} connected — handshake in progress...`)

  session.on('connected', () => {
    sessionWasRunning = true
    console.log(`[connect-test] ✓ Session #${localSession} fully connected (RUNNING)`)
    console.log('[connect-test]   Waiting for first video frame...')
    // Signal bt-manager: real session running — close RFCOMM hold immediately.
    // This lets WPP_SOCKET_IO_EXCEPTION fire AFTER projection is active (harmless).
    notifyBtManager('session_running')
  })

  session.on('disconnected', (reason?: string) => {
    const sessionType = sessionWasRunning ? 'running' : 'pre-running'
    console.log(
      `[connect-test] Session #${localSession} disconnected (${sessionType}): ${reason ?? '(no reason)'}`
    )

    // IMPORTANT: previously we cycled BT on pre-RUNNING disconnects, assuming
    // those were "verifier" sessions. That was a misdiagnosis — the real cause
    // was a broken AudioFocusResponse mapping (RELEASE→GAIN instead of
    // RELEASE→LOSS). With the corrected mapping, the phone proceeds to
    // CHANNEL_OPEN_REQUEST on the SAME TCP connection; no "verifier" pattern
    // exists. Cycling BT on disconnect caused WPP_SOCKET_IO_EXCEPTION on the
    // phone and an infinite RFCOMM retry loop.
    //
    // We just inform bt-manager for telemetry and let the phone drive retries.
    notifyBtManager(
      sessionWasRunning ? 'session_disconnected:running' : 'session_disconnected:pre_running'
    )
  })
})

let frameCount = 0
aa.on('video-frame', (buf: Buffer, ts: bigint) => {
  frameCount++
  if (frameCount === 1) {
    console.log(`[connect-test] ✓ First H.264 frame!`)
    console.log(`  Size     : ${buf.length} bytes`)
    console.log(`  Timestamp: ${ts} ns`)
    console.log(`  Hex head : ${buf.subarray(0, 16).toString('hex')}`)

    const b0 =
      buf[0] === 0x00 && buf[1] === 0x00 && buf[2] === 0x00 && buf[3] === 0x01
        ? (buf[4] ?? 0)
        : (buf[0] ?? 0)
    const nalType = b0 & 0x1f
    const nalNames: Record<number, string> = {
      7: 'SPS',
      8: 'PPS',
      5: 'IDR',
      1: 'non-IDR',
      6: 'SEI'
    }
    console.log(`  NAL type : ${nalType} (${nalNames[nalType] ?? 'unknown'})`)
  }

  if (frameCount % 10 === 0) {
    console.log(`[connect-test] ${frameCount} frames received...`)
  }

  if (frameCount >= 30) {
    console.log(`\n[connect-test] Received ${frameCount} frames — test passed ✓`)
    aa.stop()
    process.exit(0)
  }
})

process.on('SIGINT', () => {
  console.log('\n[connect-test] Interrupted.')
  aa.stop()
  process.exit(0)
})

aa.start()
