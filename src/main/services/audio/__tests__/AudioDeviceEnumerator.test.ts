import { parseDeviceMonitorOutput } from '../AudioDeviceEnumerator'

// Real-shape sample from `gst-device-monitor-1.0 Audio/Sink` on a Pi running
// PipeWire. Trimmed but structurally faithful.
const PIPEWIRE_SINK_SAMPLE = `Probing devices...

Device found:

	name  : Built-in Audio Analog Stereo
	class : Audio/Sink
	caps  : audio/x-raw, format=(string)S16LE
	properties:
		device.name = alsa_output.platform-fef00700.hdmi.hdmi-stereo
		device.description = Built-in Audio Analog Stereo
		device.class = sound
		node.name = alsa_output.platform-fef00700.hdmi.hdmi-stereo
	gst-launch-1.0 ... ! pulsesink

Device found:

	name  : HDA NVidia HDMI
	class : Audio/Sink
	properties:
		device.name = alsa_output.pci-0000_01_00.1.hdmi-stereo
		device.description = HDA NVidia HDMI
		is-default = true
	gst-launch-1.0 ... ! pulsesink
`

const PIPEWIRE_SOURCE_SAMPLE = `Probing devices...

Device found:

	name  : Built-in Audio Analog Mono
	class : Audio/Source
	properties:
		device.name = alsa_input.usb-Plantronics.mono
		device.description = Plantronics Audio
		is-default = true
`

const MIXED_SAMPLE = `${PIPEWIRE_SINK_SAMPLE}\n${PIPEWIRE_SOURCE_SAMPLE}`

describe('parseDeviceMonitorOutput', () => {
  test('parses Audio/Sink blocks from a PipeWire-shaped output', () => {
    const devices = parseDeviceMonitorOutput(PIPEWIRE_SINK_SAMPLE, 'sink')
    expect(devices).toHaveLength(2)
    expect(devices[0]).toEqual({
      id: 'alsa_output.platform-fef00700.hdmi.hdmi-stereo',
      name: 'Built-in Audio Analog Stereo',
      isDefault: false
    })
    expect(devices[1]).toEqual({
      id: 'alsa_output.pci-0000_01_00.1.hdmi-stereo',
      name: 'HDA NVidia HDMI',
      isDefault: true
    })
  })

  test('parses Audio/Source blocks separately', () => {
    const devices = parseDeviceMonitorOutput(PIPEWIRE_SOURCE_SAMPLE, 'source')
    expect(devices).toHaveLength(1)
    expect(devices[0].id).toBe('alsa_input.usb-Plantronics.mono')
    expect(devices[0].isDefault).toBe(true)
  })

  test('filters by kind when both sinks and sources are in the output', () => {
    expect(parseDeviceMonitorOutput(MIXED_SAMPLE, 'sink')).toHaveLength(2)
    expect(parseDeviceMonitorOutput(MIXED_SAMPLE, 'source')).toHaveLength(1)
  })

  test('returns empty when the output is empty or non-matching', () => {
    expect(parseDeviceMonitorOutput('', 'sink')).toEqual([])
    expect(parseDeviceMonitorOutput('Probing devices...\nnothing here', 'sink')).toEqual([])
  })

  test('deduplicates devices that appear under multiple providers (same device.name)', () => {
    const dup = `Device found:

	class : Audio/Sink
	properties:
		device.name = same_id
		device.description = First listing

Device found:

	class : Audio/Sink
	properties:
		device.name = same_id
		device.description = Second listing
`
    const devices = parseDeviceMonitorOutput(dup, 'sink')
    expect(devices).toHaveLength(1)
    expect(devices[0].name).toBe('First listing')
  })

  test('falls back to node.name when device.name is absent', () => {
    const block = `Device found:

	class : Audio/Sink
	properties:
		node.name = node_only_id
		node.description = Node Only Device
`
    const devices = parseDeviceMonitorOutput(block, 'sink')
    expect(devices[0]).toEqual({
      id: 'node_only_id',
      name: 'Node Only Device',
      isDefault: false
    })
  })

  test('strips surrounding quotes from property values', () => {
    const block = `Device found:

	class : Audio/Sink
	properties:
		device.name = "quoted_id"
		device.description = "Quoted Description"
`
    const devices = parseDeviceMonitorOutput(block, 'sink')
    expect(devices[0].id).toBe('quoted_id')
    expect(devices[0].name).toBe('Quoted Description')
  })

  test('macOS osxaudio-shaped output also parses', () => {
    const mac = `Device found:

	name  : MacBook Pro Speakers
	class : Audio/Sink
	properties:
		device.name = osx-built-in-output
		device.description = MacBook Pro Speakers
`
    const devices = parseDeviceMonitorOutput(mac, 'sink')
    expect(devices).toHaveLength(1)
    expect(devices[0].id).toBe('osx-built-in-output')
  })

  test('Windows wasapi-shaped output also parses', () => {
    const win = `Device found:

	name  : Speakers (Realtek High Definition Audio)
	class : Audio/Sink
	properties:
		device.name = {0.0.0.00000000}.{abc-def}
		device.description = Speakers (Realtek High Definition Audio)
`
    const devices = parseDeviceMonitorOutput(win, 'sink')
    expect(devices).toHaveLength(1)
    expect(devices[0].id).toBe('{0.0.0.00000000}.{abc-def}')
  })

  test('macOS GStreamer 1.28 unique-id launch line is parsed correctly', () => {
    // Real shape from gst-device-monitor-1.0 1.28.2 on macOS:
    //   properties:
    //     is-default = true (gboolean)
    //     unique-id = BuiltInSpeakerDevice
    //   gst-launch-1.0 ... ! osxaudiosink unique-id=BuiltInSpeakerDevice
    const mac = `Device found:

	name  : MacBook Pro-Lautsprecher
	class : Audio/Sink
	properties:
		is-default = true (gboolean)
		transport = bltn
		unique-id = BuiltInSpeakerDevice
	gst-launch-1.0 ... ! osxaudiosink unique-id=BuiltInSpeakerDevice
`
    const devices = parseDeviceMonitorOutput(mac, 'sink')
    expect(devices).toHaveLength(1)
    expect(devices[0].id).toBe('BuiltInSpeakerDevice')
    expect(devices[0].name).toBe('MacBook Pro-Lautsprecher')
    expect(devices[0].isDefault).toBe(true)
  })

  test('linux launch line gives string device-name verbatim', () => {
    const linux = `Device found:

	name  : Built-in Audio
	class : Audio/Sink
	properties:
		device.name = alsa_output.platform-fef00700.hdmi.hdmi-stereo
		device.description = Built-in Audio
	gst-launch-1.0 ... ! 'pulsesink device=alsa_output.platform-fef00700.hdmi.hdmi-stereo'
`
    const devices = parseDeviceMonitorOutput(linux, 'sink')
    expect(devices[0].id).toBe('alsa_output.platform-fef00700.hdmi.hdmi-stereo')
  })

  test('windows launch line gives device-name verbatim from the launch line', () => {
    const win = `Device found:

	name  : Speakers
	class : Audio/Sink
	properties:
		device.name = {0.0.0.00000000}.{abc-def}
	gst-launch-1.0 ... ! "wasapisink device-name={0.0.0.00000000}.{abc-def}"
`
    const devices = parseDeviceMonitorOutput(win, 'sink')
    expect(devices[0].id).toBe('{0.0.0.00000000}.{abc-def}')
  })

  test('falls back to properties when no launch line is present', () => {
    const block = `Device found:

	class : Audio/Sink
	properties:
		device.name = fallback_id
		device.description = Fallback Device
`
    const devices = parseDeviceMonitorOutput(block, 'sink')
    expect(devices[0].id).toBe('fallback_id')
  })

  test('prefers the top "name" header over device.description for the display label', () => {
    // Real shape on Pi5 with vc4-hdmi: device.description is bare "Built-in Audio",
    // top header carries the descriptive "Built-in Audio Digital Stereo (HDMI)"
    const block = `Device found:

	name  : Built-in Audio Digital Stereo (HDMI)
	class : Audio/Sink
	properties:
		device.description = Built-in Audio
		device.name = alsa_card.platform-107c706400.hdmi
	gst-launch-1.0 ... ! pulsesink device='alsa_output.platform-107c706400.hdmi.hdmi-stereo'
`
    const devices = parseDeviceMonitorOutput(block, 'sink')
    expect(devices[0].name).toBe('Built-in Audio Digital Stereo (HDMI)')
  })

  test('filters out PulseAudio/PipeWire monitor sources from the source list', () => {
    const block = `Device found:

	name  : Monitor of NEEWER UM04 Analog Stereo
	class : Audio/Source
	properties:
		device.class = monitor
		device.description = NEEWER UM04
	gst-launch-1.0 pulsesrc device='alsa_output.usb-NEEWER.analog-stereo.monitor' ! ...

Device found:

	name  : NEEWER UM04 Mono
	class : Audio/Source
	properties:
		device.class = sound
		device.description = NEEWER UM04
	gst-launch-1.0 pulsesrc device='alsa_input.usb-NEEWER.mono-fallback' ! ...
`
    const devices = parseDeviceMonitorOutput(block, 'source')
    expect(devices).toHaveLength(1)
    expect(devices[0].name).toBe('NEEWER UM04 Mono')
    expect(devices[0].id).toBe('alsa_input.usb-NEEWER.mono-fallback')
  })

  test('a block whose class does not match is skipped', () => {
    const otherKind = `Device found:

	class : Video/Source
	properties:
		device.name = camera0
`
    expect(parseDeviceMonitorOutput(otherKind, 'sink')).toEqual([])
    expect(parseDeviceMonitorOutput(otherKind, 'source')).toEqual([])
  })
})
