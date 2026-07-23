const path = require('path')

// Self-contained addon (no external libraries), so a plain require works on every platform.
module.exports = require(path.join(__dirname, 'build', 'Release', 'livi_crypto.node'))
