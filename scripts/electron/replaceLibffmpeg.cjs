// electron-builder afterPack hook: libffmpeg.so for linux with H265 support

const fs = require('node:fs')
const path = require('node:path')

// electron-builder Arch enum: ia32=0, x64=1, armv7l=2, arm64=3, universal=4
const ARCH_NAME = { 1: 'x64', 3: 'arm64' }

module.exports = async function replaceLibffmpeg(context) {
  if (context.electronPlatformName !== 'linux') return

  const arch = ARCH_NAME[context.arch]
  if (!arch) {
    console.log(`[afterPack] skipping unknown arch ${context.arch}`)
    return
  }

  const repoLib = path.join(
    context.packager.projectDir,
    'assets',
    'ffmpeg',
    `linux-${arch}`,
    'libffmpeg.so'
  )
  if (!fs.existsSync(repoLib)) {
    console.log(
      `[afterPack] no HEVC-enabled libffmpeg.so for linux-${arch} at ${repoLib} — keeping bundled lib`
    )
    return
  }

  const targetLib = path.join(context.appOutDir, 'libffmpeg.so')
  if (!fs.existsSync(targetLib)) {
    console.warn(`[afterPack] target libffmpeg.so not found at ${targetLib} — skipping swap`)
    return
  }

  fs.copyFileSync(repoLib, targetLib)
  const sizeKb = Math.round(fs.statSync(targetLib).size / 1024)
  console.log(`[afterPack] replaced libffmpeg.so for linux-${arch} (${sizeKb} KB)`)
}
