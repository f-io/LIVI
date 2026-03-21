param(
  [string]$Bundle = "assets/gstreamer/windows-x64"
)

$ErrorActionPreference = "Stop"

$env:PATH = "$(Join-Path $Bundle 'bin');$env:PATH"
$env:GST_PLUGIN_PATH_1_0 = Join-Path $Bundle 'lib\gstreamer-1.0'
$env:GST_PLUGIN_SYSTEM_PATH_1_0 = Join-Path $Bundle 'lib\gstreamer-1.0'

$scanner = Join-Path $Bundle 'libexec\gstreamer-1.0\gst-plugin-scanner.exe'
if (Test-Path -LiteralPath $scanner) {
  $env:GST_PLUGIN_SCANNER_1_0 = $scanner
}

& (Join-Path $Bundle 'bin\gst-launch-1.0.exe') --version
& (Join-Path $Bundle 'bin\gst-inspect-1.0.exe') coreelements
& (Join-Path $Bundle 'bin\gst-launch-1.0.exe') fakesrc num-buffers=1 ! fakesink
