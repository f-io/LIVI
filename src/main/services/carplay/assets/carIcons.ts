import icon120 from './icon-120.b64?raw'
import icon180 from './icon-180.b64?raw'
import icon256 from './icon-256.b64?raw'

const trimB64 = (b64: string) => b64.trim()

// Raw base64 strings (for config.json defaults)
export const ICON_120_B64 = trimB64(icon120)
export const ICON_180_B64 = trimB64(icon180)
export const ICON_256_B64 = trimB64(icon256)

// For dongle upload: PNG bytes as Buffer
export const ICON_120_BUFFER = Buffer.from(ICON_120_B64, 'base64')
export const ICON_180_BUFFER = Buffer.from(ICON_180_B64, 'base64')
export const ICON_256_BUFFER = Buffer.from(ICON_256_B64, 'base64')
