const path = require('path')

const addonPath = path.join(__dirname, 'build', 'Release', 'gst_video.node')

if (process.platform === 'linux') {
  const os = require('os')
  const dl = os.constants.dlopen
  const mod = { exports: {} }
  process.dlopen(mod, addonPath, dl.RTLD_LAZY | dl.RTLD_DEEPBIND)
  module.exports = mod.exports
} else {
  module.exports = require(addonPath)
}
