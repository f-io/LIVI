export enum CommandMapping {
  invalid = 0, // 'invalid'
  startRecordAudio = 1,
  stopRecordAudio = 2,
  requestHostUI = 3, // 'Projection interface My Car button clicked'
  disableBluetooth = 4,
  voiceAssistant = 5, // PTT press (VOICE_ASSIST keycode)
  voiceAssistantRelease = 6, // PTT release
  mic = 7, // 'Car Microphone'
  boxMic = 8, // 'Dongle integrated Microphone'
  frame = 12,
  hideUI = 14,
  boxMici2s = 15, // 'Box Microphone'
  enableNightMode = 16,
  disableNightMode = 17,
  startGnssReport = 18,
  stopGnssReport = 19,
  phoneMic = 21, // 'Phone Microphone'
  audioTransferOn = 22, // Disable audio
  audioTransferOff = 23, // Default - Phone streams audio to LIVI
  wifi24g = 24, // '2.4G Wifi'
  wifi5g = 25, // '5G Wifi'
  refreshFrame = 26,
  enableStandbyMode = 28,
  disableStandbyMode = 29,
  startBleAdvertising = 30,
  stopBleAdvertising = 31,

  // D-PAD
  left = 100, // 'Button Left'
  right = 101, // 'Button Right'
  up = 102, // 'Button Up'
  down = 103, // 'Button Down'
  selectDown = 104, // 'Button Select Down'
  selectUp = 105, // 'Button Select Up'
  back = 106, // 'Button Back'

  // Rotary Knob
  knobLeft = 111,
  knobRight = 112,
  knobUp = 113,
  knobDown = 114,

  // Media Control
  home = 200, // 'Button Home'
  play = 201, // 'Button Play'
  pause = 202, // 'Button Pause'
  playPause = 203, // 'Button Toggle Play/Pause'
  next = 204, // 'Button Next Track'
  prev = 205, // 'Button Prev Track'

  // Phone
  acceptPhone = 300,
  rejectPhone = 301,
  phoneKey0 = 302,
  phoneKey1 = 303,
  phoneKey2 = 304,
  phoneKey3 = 305,
  phoneKey4 = 306,
  phoneKey5 = 307,
  phoneKey6 = 308,
  phoneKey7 = 309,
  phoneKey8 = 310,
  phoneKey9 = 311,
  phoneKeyStar = 312,
  phoneKeyHash = 313,
  phoneKeyHookSwitch = 314,

  // Android Auto
  requestVideoFocus = 500,
  releaseVideoFocus = 501,
  requestAudioFocusDuck = 504,
  releaseAudioFocus = 505,
  requestNaviFocus = 506,
  releaseNaviFocus = 507,
  requestNaviScreenFocus = 508,
  releaseNaviScreenFocus = 509,

  // Connection Status Commands
  wifiEnable = 1000,
  autoConnetEnable = 1001,
  wifiConnect = 1002,
  scanningDevice = 1003,
  deviceFound = 1004,
  deviceNotFound = 1005,
  connectDeviceFailed = 1006,
  btConnected = 1007,
  btDisconnected = 1008,
  wifiConnected = 1009,
  wifiDisconnected = 1010,
  btPairStart = 1011,
  wifiPair = 1012,
  getBtOnlineList = 1013
}

export type CommandValue = keyof typeof CommandMapping

export enum AudioCommand {
  AudioOutputStart = 1,
  AudioOutputStop = 2,
  AudioInputConfig = 3,
  AudioPhonecallStart = 4,
  AudioPhonecallStop = 5,
  AudioNaviStart = 6,
  AudioNaviStop = 7,
  AudioVoiceAssistantStart = 8,
  AudioVoiceAssistantStop = 9,
  AudioMediaStart = 10,
  AudioMediaStop = 11,
  AudioAttentionStart = 12,
  AudioAttentionStop = 13,
  AudioAttentionRinging = 14,
  AudioTurnByTurnStart = 15,
  AudioTurnByTurnStop = 16
}

export enum TouchAction {
  Down = 14,
  Move = 15,
  Up = 16
}

export enum MultiTouchAction {
  Down = 1,
  Move = 2,
  Up = 0
}
