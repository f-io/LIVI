import { GhAsset } from '@main/types'

export function pickAssetForPlatform(assets: GhAsset[]): { url?: string } {
  if (!Array.isArray(assets)) return {}

  const nameOf = (a: GhAsset) => a?.name || a?.browser_download_url || ''
  const urlOf = (a?: GhAsset) => a?.browser_download_url

  if (process.platform === 'darwin') {
    const dmgs = assets.filter((a) => /\.dmg$/i.test(nameOf(a)))
    if (dmgs.length === 0) return {}
    const arch = process.arch
    const preferred =
      arch === 'arm64'
        ? (dmgs.find((a) => /(arm64|aarch64|apple[-_]?silicon|universal)/i.test(nameOf(a))) ??
          dmgs[0])
        : (dmgs.find((a) => /(x86_64|amd64|x64|universal)/i.test(nameOf(a))) ?? dmgs[0])
    return { url: urlOf(preferred) }
  }

  if (process.platform === 'linux') {
    const appImages = assets.filter((a) => /\.AppImage$/i.test(nameOf(a)))
    if (appImages.length === 0) return {}
    let patterns: RegExp[] = []
    if (process.arch === 'x64') {
      patterns = [/[-_.]x86_64\.AppImage$/i, /[-_.]amd64\.AppImage$/i, /[-_.]x64\.AppImage$/i]
    } else if (process.arch === 'arm64') {
      patterns = [/[-_.]arm64\.AppImage$/i, /[-_.]aarch64\.AppImage$/i]
    } else {
      return {}
    }
    const match = appImages.find((a) => patterns.some((re) => re.test(nameOf(a))))
    return { url: urlOf(match) }
  }

  return {}
}
