import type { Config } from '@shared/types'
import { PAGES } from '@shared/types'
import { WIFI_PASSWORD_MAX, WIFI_PASSWORD_MIN } from '@shared/types/Config'
import {
  MAX_HEIGHT,
  MAX_WIDTH,
  MIN_HEIGHT,
  MIN_WIDTH
} from '../../components/pages/settings/constants'
import { Camera } from '../../components/pages/settings/pages/camera'
import { GpsHwInfo } from '../../components/pages/settings/pages/general/gps/GpsHwInfo'
import { GpsInfo } from '../../components/pages/settings/pages/general/gps/GpsInfo'
import { TelemetryIpcClientFields } from '../../components/pages/settings/pages/general/telemetryIpc/TelemetryIpcClientFields'
import { USBDongle } from '../../components/pages/settings/pages/system/usbDongle/USBDongle'
import { SelectOption, SettingsNode } from '../types'

const panelDefaultOption: SelectOption = {
  value: '',
  label: 'Panel default',
  labelKey: 'settings.displayModeDefault'
}

async function loadDisplayModes(): Promise<SelectOption[]> {
  const list = await window.app?.listDisplayModes?.()
  if (!Array.isArray(list)) return [panelDefaultOption]
  return [panelDefaultOption, ...list.map((m) => ({ value: m, label: m }))]
}

async function loadWifiChannels(): Promise<SelectOption[]> {
  const list = await window.app?.listWifiChannels?.()
  if (!Array.isArray(list)) return []
  return list.map((c) => ({ value: c, label: String(c) }))
}

async function loadWifiCountryCodes(): Promise<SelectOption[]> {
  const list = await window.app?.listWifiCountryCodes?.()
  if (!Array.isArray(list)) return []
  return list.map((c) => ({ value: c, label: c }))
}

async function loadWifiInterfaces(): Promise<SelectOption[]> {
  const list = await window.app?.listWifiInterfaces?.()
  if (!Array.isArray(list)) return []
  return list.map((i) => ({ value: i, label: i }))
}

async function loadBtAdapters(): Promise<SelectOption[]> {
  const list = await window.app?.listBtAdapters?.()
  if (!Array.isArray(list)) return []
  return list.map((i) => ({ value: i, label: i }))
}

export const generalSchema: SettingsNode<Config> = {
  route: 'general',
  label: 'General',
  labelKey: 'settings.general',
  icon: 'general',
  type: 'route',
  path: '',
  children: [
    {
      type: 'route',
      route: 'connections',
      label: 'Connections',
      labelKey: 'settings.connections',
      icon: 'connections',
      path: '',
      children: [
        {
          type: 'string',
          label: 'Car Name',
          labelKey: 'settings.carName',
          icon: 'carName',
          path: 'carName',
          displayValue: true,
          page: {
            title: 'Car Name',
            labelTitle: 'settings.carName'
          }
        },
        {
          type: 'string',
          label: 'UI Name',
          labelKey: 'settings.uiName',
          icon: 'uiName',
          path: 'oemName',
          displayValue: true,
          page: {
            title: 'UI Name',
            labelTitle: 'settings.uiName'
          }
        },
        {
          type: 'route',
          route: 'wifi',
          label: 'Wi-Fi',
          labelKey: 'settings.wifi',
          icon: 'wifi',
          path: '',
          children: [
            {
              type: 'select',
              label: 'Wi-Fi Frequency',
              labelKey: 'settings.wifiFrequency',
              icon: 'wifiFrequency',
              path: 'wifiType',
              displayValue: true,
              options: [
                {
                  label: '2.4 GHz',
                  value: '2.4ghz'
                },
                {
                  label: '5 GHz',
                  value: '5ghz'
                }
              ],
              page: {
                title: 'Wi-Fi Frequency',
                labelTitle: 'settings.wifiFrequency'
              }
            },
            {
              type: 'string',
              label: 'Wi-Fi Password',
              labelKey: 'settings.wifiPassword',
              icon: 'wifiPassword',
              path: 'wifiPassword',
              minLength: WIFI_PASSWORD_MIN,
              maxLength: WIFI_PASSWORD_MAX,
              displayValue: true,
              page: {
                title: 'Wi-Fi Password',
                labelTitle: 'settings.wifiPassword'
              }
            },
            {
              type: 'select',
              label: 'Wi-Fi Channel',
              labelKey: 'settings.wifiChannel',
              icon: 'wifiChannel',
              path: 'wifiChannel',
              displayValue: true,
              options: [],
              loadOptions: loadWifiChannels,
              page: {
                title: 'Wi-Fi Channel',
                labelTitle: 'settings.wifiChannel'
              }
            },
            {
              type: 'select',
              label: 'Wi-Fi Channel Width',
              labelKey: 'settings.wifiChannelWidth',
              icon: 'wifiChannel',
              path: 'wifiChannelWidth',
              displayValue: true,
              options: [
                {
                  label: '20 MHz',
                  value: 20
                },
                {
                  label: '40 MHz',
                  value: 40
                },
                {
                  label: '80 MHz',
                  value: 80
                }
              ],
              page: {
                title: 'Wi-Fi Channel Width',
                labelTitle: 'settings.wifiChannelWidth'
              }
            },
            {
              type: 'select',
              label: 'Wi-Fi Country',
              labelKey: 'settings.wifiCountry',
              icon: 'wifiCountry',
              path: 'country',
              displayValue: true,
              options: [],
              loadOptions: loadWifiCountryCodes,
              page: {
                title: 'Wi-Fi Country',
                labelTitle: 'settings.wifiCountry'
              }
            },
            {
              type: 'select',
              label: 'Wi-Fi Interface',
              labelKey: 'settings.wifiInterface',
              icon: 'wifiInterface',
              path: 'wifiInterface',
              displayValue: true,
              disabled: window.app?.platform !== 'linux',
              options: [],
              loadOptions: loadWifiInterfaces,
              page: {
                title: 'Wi-Fi Interface',
                labelTitle: 'settings.wifiInterface'
              }
            },
            {
              type: 'select',
              label: 'Bluetooth Interface',
              labelKey: 'settings.btAdapter',
              icon: 'btInterface',
              path: 'btAdapter',
              displayValue: true,
              disabled: window.app?.platform !== 'linux',
              options: [],
              loadOptions: loadBtAdapters,
              page: {
                title: 'Bluetooth Interface',
                labelTitle: 'settings.btAdapter'
              }
            },
            {
              type: 'checkbox',
              label: 'Dedicated Interface',
              labelKey: 'settings.wifiDedicatedInterface',
              icon: 'dedicatedInterface',
              path: 'wifiDedicatedInterface',
              disabled: window.app?.platform !== 'linux'
            }
          ]
        },
        {
          type: 'checkbox',
          label: 'Wireless Android Auto',
          labelKey: 'settings.wirelessAaEnabled',
          icon: 'wirelessAa',
          path: 'wirelessAaEnabled',
          disabled: window.app?.platform !== 'linux'
        },
        {
          type: 'checkbox',
          label: 'Wireless CarPlay',
          labelKey: 'settings.wirelessCpEnabled',
          icon: 'wirelessCp',
          path: 'wirelessCpEnabled',
          disabled: window.app?.platform !== 'linux'
        },
        {
          type: 'checkbox',
          label: 'Auto Connect',
          labelKey: 'settings.autoConnect',
          icon: 'autoConnect',
          path: 'autoConn'
        }
      ]
    },
    {
      type: 'route',
      route: 'windowSettings',
      label: 'Window Settings',
      labelKey: 'settings.windowSettings',
      icon: 'windowSettings',
      path: '',
      children: [
        {
          type: 'route',
          label: 'Main Screen',
          labelKey: 'settings.mainScreen',
          icon: 'mainScreen',
          route: 'mainScreen',
          path: '',
          children: [
            {
              type: 'select',
              label: 'Display Mode',
              labelKey: 'settings.displayMode',
              icon: 'displayMode',
              path: 'displayMode',
              displayValue: true,
              options: [panelDefaultOption],
              loadOptions: loadDisplayModes,
              page: {
                title: 'Display Mode',
                labelTitle: 'settings.displayMode'
              }
            },
            {
              type: 'number',
              label: 'Width',
              labelKey: 'settings.mainScreenWidth',
              icon: 'width',
              path: 'mainScreenWidth',
              min: MIN_WIDTH,
              max: MAX_WIDTH,
              step: 1,
              displayValue: true,
              page: {
                title: 'Main Screen Width',
                labelTitle: 'settings.mainScreenWidth'
              }
            },
            {
              type: 'number',
              label: 'Height',
              labelKey: 'settings.mainScreenHeight',
              icon: 'height',
              path: 'mainScreenHeight',
              min: MIN_HEIGHT,
              max: MAX_HEIGHT,
              step: 1,
              displayValue: true,
              page: {
                title: 'Main Screen Height',
                labelTitle: 'settings.mainScreenHeight'
              }
            },
            {
              type: 'checkbox',
              label: 'Fullscreen',
              labelKey: 'settings.fullscreen',
              icon: 'fullscreen',
              path: 'kiosk.main'
            }
          ]
        },
        {
          type: 'route',
          label: 'Dash Screen',
          labelKey: 'settings.dashScreen',
          icon: 'dashScreen',
          route: 'dashScreen',
          path: '',
          children: [
            {
              type: 'checkbox',
              label: 'Active',
              labelKey: 'settings.dashScreenActive',
              icon: 'active',
              path: 'dashScreenActive'
            },
            {
              type: 'number',
              label: 'Width',
              labelKey: 'settings.dashScreenWidth',
              icon: 'width',
              path: 'dashScreenWidth',
              min: MIN_WIDTH,
              max: MAX_WIDTH,
              step: 1,
              displayValue: true,
              page: {
                title: 'Dash Screen Width',
                labelTitle: 'settings.dashScreenWidth'
              }
            },
            {
              type: 'number',
              label: 'Height',
              labelKey: 'settings.dashScreenHeight',
              icon: 'height',
              path: 'dashScreenHeight',
              min: MIN_HEIGHT,
              max: MAX_HEIGHT,
              step: 1,
              displayValue: true,
              page: {
                title: 'Dash Screen Height',
                labelTitle: 'settings.dashScreenHeight'
              }
            },
            {
              type: 'checkbox',
              label: 'Fullscreen',
              labelKey: 'settings.fullscreen',
              icon: 'fullscreen',
              path: 'kiosk.dash'
            }
          ]
        },
        {
          type: 'route',
          label: 'Aux Screen',
          labelKey: 'settings.auxScreen',
          icon: 'auxScreen',
          route: 'auxScreen',
          path: '',
          children: [
            {
              type: 'checkbox',
              label: 'Active',
              labelKey: 'settings.auxScreenActive',
              icon: 'active',
              path: 'auxScreenActive'
            },
            {
              type: 'number',
              label: 'Width',
              labelKey: 'settings.auxScreenWidth',
              icon: 'width',
              path: 'auxScreenWidth',
              min: MIN_WIDTH,
              max: MAX_WIDTH,
              step: 1,
              displayValue: true,
              page: {
                title: 'Aux Screen Width',
                labelTitle: 'settings.auxScreenWidth'
              }
            },
            {
              type: 'number',
              label: 'Height',
              labelKey: 'settings.auxScreenHeight',
              icon: 'height',
              path: 'auxScreenHeight',
              min: MIN_HEIGHT,
              max: MAX_HEIGHT,
              step: 1,
              displayValue: true,
              page: {
                title: 'Aux Screen Height',
                labelTitle: 'settings.auxScreenHeight'
              }
            },
            {
              type: 'checkbox',
              label: 'Fullscreen',
              labelKey: 'settings.fullscreen',
              icon: 'fullscreen',
              path: 'kiosk.aux'
            }
          ]
        }
      ]
    },
    {
      type: 'route',
      label: 'Tab Settings',
      labelKey: 'settings.tabSettings',
      icon: 'tabSettings',
      route: 'tabSettings',
      path: '',
      children: [
        {
          type: 'route',
          label: 'Dashboards',
          labelKey: 'settings.telemetryDashboards',
          icon: 'dashboards',
          route: 'dashboards',
          path: '',
          children: [
            {
              type: 'posList',
              label: 'Dashboards',
              labelKey: 'settings.telemetryDashboards',
              path: 'dashboards',
              items: [
                { id: 'dash1', label: 'Dash 1', labelKey: 'settings.telemetryDash1' },
                { id: 'dash2', label: 'Dash 2', labelKey: 'settings.telemetryDash2' },
                { id: 'dash3', label: 'Dash 3', labelKey: 'settings.telemetryDash3' },
                { id: 'dash4', label: 'Dash 4', labelKey: 'settings.telemetryDash4' }
              ]
            },
            ...(['dash1', 'dash2', 'dash3', 'dash4'] as const).map((id, i) => ({
              type: 'route' as const,
              label: `Dash ${i + 1}`,
              labelKey: `settings.telemetry${id.charAt(0).toUpperCase()}${id.slice(1)}`,
              route: id,
              path: '',
              hidden: true,
              children: [
                {
                  type: 'checkbox' as const,
                  label: 'Main',
                  labelKey: 'settings.mainScreen',
                  icon: 'mainScreen',
                  path: `dashboards.${id}.main`
                },
                {
                  type: 'checkbox' as const,
                  label: 'Dash',
                  labelKey: 'settings.dashScreen',
                  icon: 'dashScreen',
                  path: `dashboards.${id}.dash`
                },
                {
                  type: 'checkbox' as const,
                  label: 'Aux',
                  labelKey: 'settings.auxScreen',
                  icon: 'auxScreen',
                  path: `dashboards.${id}.aux`
                }
              ]
            }))
          ]
        },
        {
          type: 'route',
          label: 'Media',
          labelKey: 'settings.media',
          icon: 'media',
          route: 'media',
          path: '',
          children: [
            {
              type: 'checkbox',
              label: 'Main',
              labelKey: 'settings.mainScreen',
              icon: 'mainScreen',
              path: 'media.main'
            },
            {
              type: 'checkbox',
              label: 'Dash',
              labelKey: 'settings.dashScreen',
              icon: 'dashScreen',
              path: 'media.dash'
            },
            {
              type: 'checkbox',
              label: 'Aux',
              labelKey: 'settings.auxScreen',
              icon: 'auxScreen',
              path: 'media.aux'
            }
          ]
        },
        {
          type: 'route',
          label: 'Reverse Camera',
          labelKey: 'settings.reverseCamera',
          icon: 'reverseCamera',
          route: 'camera',
          path: '',
          displayValue: true,
          children: [
            {
              type: 'checkbox',
              label: 'Automatic on reverse gear',
              labelKey: 'settings.autoSwitchOnReverse',
              icon: 'autoReverse',
              path: 'autoSwitchOnReverse'
            },
            {
              type: 'checkbox',
              label: 'Main',
              labelKey: 'settings.mainScreen',
              icon: 'mainScreen',
              path: 'camera.main'
            },
            {
              type: 'checkbox',
              label: 'Dash',
              labelKey: 'settings.dashScreen',
              icon: 'dashScreen',
              path: 'camera.dash'
            },
            {
              type: 'checkbox',
              label: 'Aux',
              labelKey: 'settings.auxScreen',
              icon: 'auxScreen',
              path: 'camera.aux'
            },
            {
              type: 'checkbox',
              label: 'Mirror',
              labelKey: 'settings.cameraMirror',
              icon: 'cameraMirror',
              path: 'cameraMirror'
            },
            {
              type: 'select',
              label: 'Rotation',
              labelKey: 'settings.cameraRotation',
              icon: 'cameraRotation',
              path: 'cameraRotation',
              displayValue: true,
              options: [
                { label: '0°', value: 0 },
                { label: '90°', value: 90 },
                { label: '180°', value: 180 },
                { label: '270°', value: 270 }
              ],
              page: {
                title: 'Camera Rotation',
                labelTitle: 'settings.cameraRotation'
              }
            },
            {
              type: 'route',
              label: 'Camera',
              labelKey: 'settings.camera',
              icon: 'camera',
              route: 'select',
              path: '',
              children: [
                {
                  path: 'cameraId',
                  type: 'custom',
                  label: 'Camera',
                  labelKey: 'settings.camera',
                  component: Camera
                }
              ]
            }
          ]
        },
        {
          type: 'route',
          label: 'Custom Tab',
          labelKey: 'settings.customTab',
          icon: 'customTab',
          route: 'custom',
          path: '',
          displayValue: true,
          children: [
            {
              type: 'checkbox',
              label: 'Main',
              labelKey: 'settings.mainScreen',
              icon: 'mainScreen',
              path: 'custom.main'
            },
            {
              type: 'checkbox',
              label: 'Dash',
              labelKey: 'settings.dashScreen',
              icon: 'dashScreen',
              path: 'custom.dash'
            },
            {
              type: 'checkbox',
              label: 'Aux',
              labelKey: 'settings.auxScreen',
              icon: 'auxScreen',
              path: 'custom.aux'
            },
            {
              type: 'string',
              label: 'Address',
              labelKey: 'settings.customUrl',
              icon: 'customUrl',
              path: 'customUrl',
              format: 'url',
              displayValue: true,
              page: {
                title: 'Address',
                labelTitle: 'settings.customUrl'
              }
            }
          ]
        }
      ]
    },
    {
      type: 'route',
      label: 'Key Bindings',
      labelKey: 'settings.keyBindings',
      icon: 'keyBindings',
      route: 'keyBindings',
      path: '',
      children: [
        {
          type: 'keybinding',
          label: 'Up',
          labelKey: 'settings.up',
          path: 'bindings',
          bindingKey: 'up'
        },
        {
          type: 'keybinding',
          label: 'Down',
          labelKey: 'settings.down',
          path: 'bindings',
          bindingKey: 'down'
        },
        {
          type: 'keybinding',
          label: 'Left',
          labelKey: 'settings.left',
          path: 'bindings',
          bindingKey: 'left'
        },
        {
          type: 'keybinding',
          label: 'Right',
          labelKey: 'settings.right',
          path: 'bindings',
          bindingKey: 'right'
        },

        {
          type: 'keybinding',
          label: 'Select Up',
          labelKey: 'settings.selectUp',
          path: 'bindings',
          bindingKey: 'selectUp'
        },
        {
          type: 'keybinding',
          label: 'Select Down',
          labelKey: 'settings.selectDown',
          path: 'bindings',
          bindingKey: 'selectDown'
        },

        {
          type: 'keybinding',
          label: 'Back',
          labelKey: 'settings.back',
          path: 'bindings',
          bindingKey: 'back'
        },

        {
          type: 'keybinding',
          label: 'Knob Left',
          labelKey: 'settings.knobLeft',
          path: 'bindings',
          bindingKey: 'knobLeft'
        },
        {
          type: 'keybinding',
          label: 'Knob Right',
          labelKey: 'settings.knobRight',
          path: 'bindings',
          bindingKey: 'knobRight'
        },
        {
          type: 'keybinding',
          label: 'Knob Up',
          labelKey: 'settings.knobUp',
          path: 'bindings',
          bindingKey: 'knobUp'
        },
        {
          type: 'keybinding',
          label: 'Knob Down',
          labelKey: 'settings.knobDown',
          path: 'bindings',
          bindingKey: 'knobDown'
        },

        {
          type: 'keybinding',
          label: 'Home',
          labelKey: 'settings.home',
          path: 'bindings',
          bindingKey: 'home'
        },
        {
          type: 'keybinding',
          label: 'Cycle Session',
          labelKey: 'settings.cycleSession',
          path: 'bindings',
          bindingKey: 'cycleSession'
        },

        {
          type: 'keybinding',
          label: 'Play/Pause',
          labelKey: 'settings.playPause',
          path: 'bindings',
          bindingKey: 'playPause'
        },
        {
          type: 'keybinding',
          label: 'Play',
          labelKey: 'settings.play',
          path: 'bindings',
          bindingKey: 'play'
        },
        {
          type: 'keybinding',
          label: 'Pause',
          labelKey: 'settings.pause',
          path: 'bindings',
          bindingKey: 'pause'
        },

        {
          type: 'keybinding',
          label: 'Next',
          labelKey: 'settings.next',
          path: 'bindings',
          bindingKey: 'next'
        },
        {
          type: 'keybinding',
          label: 'Previous',
          labelKey: 'settings.previous',
          path: 'bindings',
          bindingKey: 'prev'
        },

        {
          type: 'keybinding',
          label: 'Accept Call',
          labelKey: 'settings.acceptCall',
          path: 'bindings',
          bindingKey: 'acceptPhone'
        },
        {
          type: 'keybinding',
          label: 'Reject Call',
          labelKey: 'settings.rejectCall',
          path: 'bindings',
          bindingKey: 'rejectPhone'
        },

        {
          type: 'keybinding',
          label: 'Phone Key 0',
          labelKey: 'settings.phoneKey0',
          path: 'bindings',
          bindingKey: 'phoneKey0'
        },
        {
          type: 'keybinding',
          label: 'Phone Key 1',
          labelKey: 'settings.phoneKey1',
          path: 'bindings',
          bindingKey: 'phoneKey1'
        },
        {
          type: 'keybinding',
          label: 'Phone Key 2',
          labelKey: 'settings.phoneKey2',
          path: 'bindings',
          bindingKey: 'phoneKey2'
        },
        {
          type: 'keybinding',
          label: 'Phone Key 3',
          labelKey: 'settings.phoneKey3',
          path: 'bindings',
          bindingKey: 'phoneKey3'
        },
        {
          type: 'keybinding',
          label: 'Phone Key 4',
          labelKey: 'settings.phoneKey4',
          path: 'bindings',
          bindingKey: 'phoneKey4'
        },
        {
          type: 'keybinding',
          label: 'Phone Key 5',
          labelKey: 'settings.phoneKey5',
          path: 'bindings',
          bindingKey: 'phoneKey5'
        },
        {
          type: 'keybinding',
          label: 'Phone Key 6',
          labelKey: 'settings.phoneKey6',
          path: 'bindings',
          bindingKey: 'phoneKey6'
        },
        {
          type: 'keybinding',
          label: 'Phone Key 7',
          labelKey: 'settings.phoneKey7',
          path: 'bindings',
          bindingKey: 'phoneKey7'
        },
        {
          type: 'keybinding',
          label: 'Phone Key 8',
          labelKey: 'settings.phoneKey8',
          path: 'bindings',
          bindingKey: 'phoneKey8'
        },
        {
          type: 'keybinding',
          label: 'Phone Key 9',
          labelKey: 'settings.phoneKey9',
          path: 'bindings',
          bindingKey: 'phoneKey9'
        },
        {
          type: 'keybinding',
          label: 'Phone Key *',
          labelKey: 'settings.phoneKeyStar',
          path: 'bindings',
          bindingKey: 'phoneKeyStar'
        },
        {
          type: 'keybinding',
          label: 'Phone Key #',
          labelKey: 'settings.phoneKeyHash',
          path: 'bindings',
          bindingKey: 'phoneKeyHash'
        },
        {
          type: 'keybinding',
          label: 'Hook Switch',
          labelKey: 'settings.phoneKeyHookSwitch',
          path: 'bindings',
          bindingKey: 'phoneKeyHookSwitch'
        },

        {
          type: 'keybinding',
          label: 'Voice Assistant',
          labelKey: 'settings.voiceAssistant',
          path: 'bindings',
          bindingKey: 'voiceAssistant'
        },
        {
          type: 'keybinding',
          label: 'Voice Assistant Release',
          labelKey: 'settings.voiceAssistantRelease',
          path: 'bindings',
          bindingKey: 'voiceAssistantRelease'
        }
      ]
    },
    {
      type: 'select',
      label: 'Start Page',
      labelKey: 'settings.startPage',
      icon: 'startPage',
      path: 'startPage',
      displayValue: true,
      options: PAGES.map(({ path, label, labelKey }) => ({ label, labelKey, value: path })),
      page: {
        title: 'Start Page',
        labelTitle: 'settings.startPage'
      }
    },
    {
      type: 'number',
      label: 'FFT Delay',
      labelKey: 'settings.fftDelay',
      icon: 'fftDelay',
      path: 'visualAudioDelayMs',
      displayValue: true,
      valueTransform: {
        toView: (v: number) => v,
        fromView: (v: number) => v,
        format: (v: number) => `${v} ms`
      },
      page: {
        title: 'FFT Visualization Delay',
        labelTitle: 'settings.fftDelay'
      }
    },
    {
      type: 'select',
      label: 'Steering wheel position',
      labelKey: 'settings.steeringWheelPosition',
      icon: 'steering',
      path: 'hand',
      displayValue: true,
      options: [
        { label: 'LHD', labelKey: 'settings.lhdr', value: 0 },
        { label: 'RHD', labelKey: 'settings.rhdr', value: 1 }
      ],
      page: {
        title: 'Steering wheel position',
        labelTitle: 'settings.steeringWheelPosition'
      }
    },
    {
      type: 'number',
      label: 'UI Zoom',
      labelKey: 'settings.uiZoom',
      icon: 'uiZoom',
      path: 'uiZoomPercent',
      displayValue: true,
      min: 50,
      max: 200,
      step: 10,
      valueTransform: {
        toView: (v: number) => v,
        fromView: (v: number) => v,
        format: (v: number) => `${v}%`
      },
      page: {
        title: 'UI Zoom',
        labelTitle: 'settings.uiZoom'
      }
    },
    {
      type: 'select',
      label: 'Language',
      labelKey: 'settings.language',
      icon: 'language',
      path: 'language',
      displayValue: true,
      options: [
        { label: 'English', labelKey: 'settings.english', value: 'en' },
        { label: 'German', labelKey: 'settings.german', value: 'de' },
        { label: 'Ukrainian', labelKey: 'settings.ukrainian', value: 'ua' },
        { label: 'French', labelKey: 'settings.french', value: 'fr' }
      ],
      page: {
        title: 'Language',
        labelTitle: 'settings.language'
      }
    },
    {
      type: 'route',
      label: 'MFi',
      labelKey: 'settings.mfi',
      icon: 'mfi',
      route: 'mfi',
      path: '',
      children: [
        {
          type: 'number',
          label: 'I2C Bus',
          labelKey: 'settings.i2cBus',
          icon: 'i2cBus',
          path: 'carPlayMfiI2cBus',
          min: 0,
          max: 20,
          step: 1,
          default: 2,
          displayValue: true,
          page: {
            title: 'I2C Bus',
            labelTitle: 'settings.i2cBus'
          }
        },
        {
          type: 'number',
          label: 'Power Pin',
          labelKey: 'settings.mfiPowerGpio',
          icon: 'powerPin',
          path: 'carPlayMfiPowerGpio',
          min: -1,
          max: 27,
          step: 1,
          default: -1,
          displayValue: true,
          valueTransform: {
            toView: (v: number) => v,
            fromView: (v: number) => v,
            format: (v: number) => (v < 0 ? '-' : `${v}`)
          },
          page: {
            title: 'Power Pin',
            labelTitle: 'settings.mfiPowerGpio'
          }
        }
      ]
    },
    {
      type: 'route',
      label: 'GPS',
      labelKey: 'settings.gps',
      icon: 'gps',
      route: 'gps',
      path: '',
      children: [
        {
          type: 'checkbox',
          label: 'Enable',
          labelKey: 'settings.gpsEnabled',
          icon: 'gpsEnabled',
          path: 'gpsEnabled'
        },
        {
          type: 'string',
          label: 'Device',
          labelKey: 'settings.gpsDevice',
          icon: 'gpsDevice',
          path: 'gpsDevice',
          page: {
            title: 'Device',
            labelTitle: 'settings.gpsDevice'
          }
        },
        {
          type: 'select',
          label: 'Baud Rate',
          labelKey: 'settings.gpsBaudRate',
          icon: 'gpsBaudRate',
          path: 'gpsBaudRate',
          options: [
            { label: '4800', value: 4800 },
            { label: '9600', value: 9600 },
            { label: '19200', value: 19200 },
            { label: '38400', value: 38400 },
            { label: '57600', value: 57600 },
            { label: '115200', value: 115200 }
          ],
          page: {
            title: 'Baud Rate',
            labelTitle: 'settings.gpsBaudRate'
          }
        },
        {
          type: 'route',
          label: 'GPS Data',
          labelKey: 'settings.gpsInfo',
          icon: 'gpsInfo',
          route: 'info',
          path: '',
          children: [
            {
              type: 'custom',
              label: 'GPS Data',
              labelKey: 'settings.gpsInfo',
              path: 'carName',
              component: GpsInfo
            }
          ]
        },
        {
          type: 'route',
          label: 'HW Info',
          labelKey: 'settings.gpsHwInfo',
          icon: 'gpsHwInfo',
          route: 'hwinfo',
          path: '',
          children: [
            {
              type: 'custom',
              label: 'HW Info',
              labelKey: 'settings.gpsHwInfo',
              path: 'carName',
              component: GpsHwInfo
            }
          ]
        }
      ]
    },
    {
      type: 'select',
      label: 'Telemetry (IPC)',
      labelKey: 'settings.telemetryIpc',
      icon: 'telemetryIpc',
      path: 'telemetryMode',
      displayValue: true,
      options: [
        { label: 'Host', labelKey: 'settings.telemetryIpcHost', value: 'host' },
        { label: 'Client', labelKey: 'settings.telemetryIpcClient', value: 'client' }
      ],
      page: {
        title: 'Telemetry (IPC)',
        labelTitle: 'settings.telemetryIpc'
      }
    },
    {
      type: 'custom',
      label: 'Telemetry (IPC) Client',
      labelKey: 'settings.telemetryIpcClientFields',
      path: 'telemetryClientIp',
      component: TelemetryIpcClientFields
    },
    {
      type: 'route',
      label: 'USB Dongle',
      labelKey: 'settings.usbDongle',
      icon: 'usbDongle',
      route: 'usbDongle',
      path: '',
      children: [
        {
          type: 'custom',
          label: 'USB Dongle',
          labelKey: 'settings.usbDongle',
          path: 'carName',
          component: USBDongle
        }
      ]
    }
  ]
}
