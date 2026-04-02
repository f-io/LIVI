import type { ExtraConfig } from '@shared/types'
import { SettingsNode } from '../types'

export const generalSchema: SettingsNode<ExtraConfig> = {
  route: 'general',
  label: 'General',
  labelKey: 'settings.general',
  type: 'route',
  path: '',
  children: [
    {
      type: 'route',
      route: 'connections',
      label: 'Connections',
      labelKey: 'settings.connections',
      path: '',
      children: [
        {
          type: 'string',
          label: 'Car Name',
          labelKey: 'settings.carName',
          path: 'carName',
          displayValue: true,
          page: {
            title: 'Car Name',
            labelTitle: 'settings.carName',
            description: 'The name of the CarPlay device',
            labelDescription: 'settings.carNameDescription'
          }
        },
        {
          type: 'string',
          label: 'UI Name',
          labelKey: 'settings.uiName',
          path: 'oemName',
          displayValue: true,
          page: {
            title: 'UI Name',
            labelTitle: 'settings.uiName',
            description: 'The name displayed in the CarPlay UI.',
            labelDescription: 'settings.uiNameDescription'
          }
        },
        {
          type: 'route',
          route: 'wifi',
          label: 'Wi-Fi',
          labelKey: 'settings.wifi',
          path: '',
          children: [
            {
              type: 'select',
              label: 'Wi-Fi Frequency',
              labelKey: 'settings.wifiFrequency',
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
                labelTitle: 'settings.wifiFrequency',
                description: 'Wi-Fi frequency selection',
                labelDescription: 'settings.wifiFrequencyDescription'
              }
            }
          ]
        },
        {
          type: 'checkbox',
          label: 'Auto Connect',
          labelKey: 'settings.autoConnect',
          path: 'autoConn'
        }
      ]
    },
    {
      type: 'route',
      route: 'firmwareSettings',
      label: 'Firmware Settings',
      labelKey: 'settings.firmwareSettings',
      path: '',
      children: [
        {
          type: 'route',
          route: 'dashboardInfo',
          label: 'Dashboard Info',
          labelKey: 'settings.DashboardInfo',
          path: '',
          children: [
            {
              type: 'checkbox',
              label: 'Media Info',
              labelKey: 'settings.dashboardMediaInfo',
              path: 'dashboardMediaInfo'
            },
            {
              type: 'checkbox',
              label: 'Vehicle Info',
              labelKey: 'settings.dashboardVehicleInfo',
              path: 'dashboardVehicleInfo'
            },
            {
              type: 'checkbox',
              label: 'Route Info',
              labelKey: 'settings.dashboardRouteInfo',
              path: 'dashboardRouteInfo'
            }
          ]
        },
        {
          type: 'route',
          route: 'gnss',
          label: 'GNSS',
          labelKey: 'settings.GNSS',
          path: '',
          children: [
            {
              type: 'checkbox',
              label: 'HU GPS Forwarding',
              labelKey: 'settings.gps',
              path: 'gps'
            },
            {
              type: 'checkbox',
              label: 'GPS',
              labelKey: 'settings.gnssGps',
              path: 'gnssGps'
            },
            {
              type: 'checkbox',
              label: 'GLONASS',
              labelKey: 'settings.gnssGlonass',
              path: 'gnssGlonass'
            },
            {
              type: 'checkbox',
              label: 'Galileo',
              labelKey: 'settings.gnssGalileo',
              path: 'gnssGalileo'
            },
            {
              type: 'checkbox',
              label: 'BeiDou',
              labelKey: 'settings.gnssBeiDou',
              path: 'gnssBeiDou'
            }
          ]
        }
      ]
    },
    {
      type: 'route',
      route: 'autoSwitch',
      label: 'Auto Switch',
      labelKey: 'settings.autoSwitch',
      path: '',
      children: [
        {
          type: 'checkbox',
          label: 'Switch on Stream Start',
          labelKey: 'settings.autoSwitchOnStream',
          path: 'autoSwitchOnStream'
        },
        {
          type: 'checkbox',
          label: 'Switch on Phone Call',
          labelKey: 'settings.autoSwitchOnPhoneCall',
          path: 'autoSwitchOnPhoneCall'
        },
        {
          type: 'checkbox',
          label: 'Switch on Guidance',
          labelKey: 'settings.autoSwitchOnGuidance',
          path: 'autoSwitchOnGuidance'
        }
      ]
    },
    {
      type: 'route',
      label: 'Key Bindings',
      labelKey: 'settings.keyBindings',
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
          bindingKey: 'siri'
        },
        {
          type: 'keybinding',
          label: 'Voice Assistant Release',
          labelKey: 'settings.voiceAssistantRelease',
          path: 'bindings',
          bindingKey: 'siriRelease'
        }
      ]
    },
    {
      type: 'select',
      label: 'Start Page',
      labelKey: 'settings.startPage',
      path: 'startPage',
      displayValue: true,
      options: [
        { label: 'Home', labelKey: 'settings.startPageHome', value: 'home' },
        { label: 'Maps', labelKey: 'settings.startPageMaps', value: 'maps' },
        { label: 'Telemetry', labelKey: 'settings.startPageTelemetry', value: 'telemetry' },
        { label: 'Media', labelKey: 'settings.startPageMedia', value: 'media' },
        { label: 'Camera', labelKey: 'settings.startPageCamera', value: 'camera' },
        { label: 'Settings', labelKey: 'settings.startPageSettings', value: 'settings' }
      ],
      page: {
        title: 'Start Page',
        labelTitle: 'settings.startPage',
        description: 'Select which page LIVI should open on startup.',
        labelDescription: 'settings.startPageDescription'
      }
    },
    {
      type: 'number',
      label: 'FFT Delay',
      labelKey: 'settings.fftDelay',
      path: 'visualAudioDelayMs',
      displayValue: true,
      valueTransform: {
        toView: (v: number) => v,
        fromView: (v: number) => v,
        format: (v: number) => `${v} ms`
      },
      page: {
        title: 'FFT Visualization Delay',
        labelTitle: 'settings.fftDelay',
        description: 'Delays the FFT visualization to compensate for audio latency.',
        labelDescription: 'settings.fftDelayDescription'
      }
    },
    {
      type: 'select',
      label: 'Steering wheel position',
      labelKey: 'settings.steeringWheelPosition',
      path: 'hand',
      displayValue: true,
      options: [
        { label: 'LHD', labelKey: 'settings.lhdr', value: 0 },
        { label: 'RHD', labelKey: 'settings.rhdr', value: 1 }
      ],
      page: {
        title: 'Steering wheel position',
        labelTitle: 'settings.steeringWheelPosition',
        description: 'Set the position of the steering wheel controls.',
        labelDescription: 'settings.steeringWheelPositionDescription'
      }
    },
    {
      type: 'route',
      label: 'Telemetry',
      labelKey: 'settings.telemetry',
      route: 'telemetry',
      path: '',
      children: [
        {
          type: 'posList',
          label: 'Dashboards',
          labelKey: 'settings.telemetryDashboards',
          path: 'telemetryDashboards',
          items: [
            { id: 'dash1', label: 'Dash 1', labelKey: 'settings.telemetryDash1' },
            { id: 'dash2', label: 'Dash 2', labelKey: 'settings.telemetryDash2' },
            { id: 'dash3', label: 'Dash 3', labelKey: 'settings.telemetryDash3' },
            { id: 'dash4', label: 'Dash 4', labelKey: 'settings.telemetryDash4' }
          ]
        }
      ]
    },
    {
      type: 'checkbox',
      label: 'Maps',
      labelKey: 'settings.maps',
      path: 'mapsEnabled'
    },
    {
      type: 'checkbox',
      label: 'Fullscreen',
      labelKey: 'settings.fullscreen',
      path: 'kiosk'
    },
    {
      type: 'number',
      label: 'UI Zoom',
      labelKey: 'settings.uiZoom',
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
        labelTitle: 'settings.uiZoom',
        description: 'Adjust the global UI zoom level of the application window.',
        labelDescription: 'settings.uiZoomDescription'
      }
    },
    {
      type: 'select',
      label: 'Language',
      labelKey: 'settings.language',
      path: 'language',
      displayValue: true,
      options: [
        { label: 'English', labelKey: 'settings.english', value: 'en' },
        { label: 'German', labelKey: 'settings.german', value: 'de' },
        { label: 'Ukrainian', labelKey: 'settings.ukrainian', value: 'ua' }
      ],
      page: {
        title: 'Language',
        labelTitle: 'settings.language',
        description: 'Select the application language',
        labelDescription: 'settings.languageDescription'
      }
    }
  ]
}
