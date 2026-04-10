import { app } from 'electron'
import fs from 'fs'

function usernsBlocked(): boolean {
  try {
    return fs.readFileSync('/proc/sys/kernel/unprivileged_userns_clone', 'utf8').trim() === '0'
  } catch {
    return false
  }
}

const isLinux = process.platform === 'linux'
const sandboxBroken = usernsBlocked()

if (isLinux && sandboxBroken) {
  app.commandLine.appendSwitch('no-sandbox')
  app.commandLine.appendSwitch('disable-setuid-sandbox')
}
