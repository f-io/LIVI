param(
  [string]$Out = "assets/gstreamer/windows-x64"
)

$ErrorActionPreference = "Stop"

function Copy-Required {
  param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$Destination
  )

  if (-not (Test-Path -LiteralPath $Source)) {
    Write-Error "missing required file: $Source"
    exit 1
  }

  Copy-Item -LiteralPath $Source -Destination $Destination -Force
}

function Copy-Optional {
  param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$Destination
  )

  if (-not (Test-Path -LiteralPath $Source)) {
    Write-Host "skip optional: $Source"
    return
  }

  Copy-Item -LiteralPath $Source -Destination $Destination -Force
}

$candidates = @(
  'C:\gstreamer\1.0\msvc_x86_64',
  'C:\Program Files\gstreamer\1.0\msvc_x86_64',
  'C:\Program Files\GStreamer\1.0\msvc_x86_64',
  'C:\Program Files\gstreamer',
  'C:\Program Files\GStreamer'
)

$GstRoot = $null

foreach ($candidate in $candidates) {
  if (Test-Path -LiteralPath (Join-Path $candidate 'bin\gst-launch-1.0.exe')) {
    $GstRoot = $candidate
    break
  }
}

if (-not $GstRoot) {
  Write-Error "GStreamer root not found. Checked: $($candidates -join ', ')"
  exit 1
}

$GstBin = Join-Path $GstRoot "bin"
$PluginDir = Join-Path $GstRoot "lib\gstreamer-1.0"
$ScannerDir = Join-Path $GstRoot "libexec\gstreamer-1.0"

$GstLaunch = Join-Path $GstBin "gst-launch-1.0.exe"
$GstInspect = Join-Path $GstBin "gst-inspect-1.0.exe"
$Scanner = Join-Path $ScannerDir "gst-plugin-scanner.exe"

if (-not (Test-Path -LiteralPath $GstLaunch)) {
  Write-Error "gst-launch-1.0.exe not found: $GstLaunch"
  exit 1
}

if (-not (Test-Path -LiteralPath $GstInspect)) {
  Write-Error "gst-inspect-1.0.exe not found: $GstInspect"
  exit 1
}

if (-not (Test-Path -LiteralPath $PluginDir)) {
  Write-Error "gstreamer plugin directory not found: $PluginDir"
  exit 1
}

Write-Host "Using gst-launch:  $GstLaunch"
Write-Host "Using gst-inspect: $GstInspect"
Write-Host "Using plugin dir:  $PluginDir"

if (Test-Path -LiteralPath $Scanner) {
  Write-Host "Using scanner:     $Scanner"
} else {
  Write-Host "No gst-plugin-scanner found; continuing without it"
}

Remove-Item -LiteralPath $Out -Recurse -Force -ErrorAction SilentlyContinue

New-Item -ItemType Directory -Force -Path `
  (Join-Path $Out "bin"), `
  (Join-Path $Out "lib"), `
  (Join-Path $Out "lib\gstreamer-1.0"), `
  (Join-Path $Out "libexec\gstreamer-1.0") | Out-Null

# bin tools
Copy-Required $GstLaunch (Join-Path $Out "bin\gst-launch-1.0.exe")
Copy-Required $GstInspect (Join-Path $Out "bin\gst-inspect-1.0.exe")

# scanner
if (Test-Path -LiteralPath $Scanner) {
  Copy-Required $Scanner (Join-Path $Out "libexec\gstreamer-1.0\gst-plugin-scanner.exe")
}

# exe available ?
if (-not (Test-Path -LiteralPath (Join-Path $Out "bin\gst-launch-1.0.exe"))) {
  throw "gst-launch-1.0.exe missing after copy"
}

if (-not (Test-Path -LiteralPath (Join-Path $Out "bin\gst-inspect-1.0.exe"))) {
  throw "gst-inspect-1.0.exe missing after copy"
}

if (Test-Path -LiteralPath $Scanner) {
  if (-not (Test-Path -LiteralPath (Join-Path $Out "libexec\gstreamer-1.0\gst-plugin-scanner.exe"))) {
    throw "gst-plugin-scanner.exe missing after copy"
  }
}

# runtime DLLs (always shipped by upstream)
$libs = @(
  "gstreamer-1.0-0.dll",
  "gstbase-1.0-0.dll",
  "gstapp-1.0-0.dll",
  "gstaudio-1.0-0.dll",
  "gstpbutils-1.0-0.dll",
  "gsttag-1.0-0.dll",
  "gstfft-1.0-0.dll",
  "gstvideo-1.0-0.dll",
  "glib-2.0-0.dll",
  "gobject-2.0-0.dll",
  "gmodule-2.0-0.dll",
  "gio-2.0-0.dll",
  "gthread-2.0-0.dll",
  "orc-0.4-0.dll",
  "ffi-7.dll",
  "pcre2-8-0.dll",
  "z-1.dll",
  "bz2.dll"
)

foreach ($lib in $libs) {
  Copy-Required (Join-Path $GstBin $lib) (Join-Path $Out "bin")
}

# runtime DLLs that come and go between cerbero builds
# (e.g. GStreamer 1.28.2 Windows MSVC dropped libiconv-2 / libcharset-1
# because newer GLib no longer links them on Windows)
$optionalLibs = @(
  "intl-8.dll",
  "libiconv-2.dll",
  "libcharset-1.dll"
)

foreach ($lib in $optionalLibs) {
  Copy-Optional (Join-Path $GstBin $lib) (Join-Path $Out "bin")
}

# plugins
$plugins = @(
  "gstapp.dll",
  "gstaudioconvert.dll",
  "gstaudiofx.dll",
  "gstaudiomixer.dll",
  "gstaudioparsers.dll",
  "gstaudiorate.dll",
  "gstaudioresample.dll",
  "gstaudiotestsrc.dll",
  "gstautodetect.dll",
  "gstcoreelements.dll",
  "gstequalizer.dll",
  "gstinterleave.dll",
  "gstlevel.dll",
  "gstrawparse.dll",
  "gstvolume.dll",
  "gstwasapi.dll",
  "gstdirectsound.dll"
)

foreach ($plugin in $plugins) {
  Copy-Required (Join-Path $PluginDir $plugin) (Join-Path $Out "lib\gstreamer-1.0")
}

Write-Host "Created Windows GStreamer bundle at: $Out"
Write-Host "Bundle size:"
$size = (Get-ChildItem -LiteralPath $Out -Recurse -File | Measure-Object -Property Length -Sum).Sum
"{0:N1} MB" -f ($size / 1MB)

Write-Host "Top-level contents:"
Get-ChildItem -LiteralPath $Out -Recurse | ForEach-Object { $_.FullName }
