import type React from 'react'

export enum RoutePath {
  Camera = 'camera',
  Home = 'home',
  Maps = 'maps',
  Media = 'media',
  Settings = 'settings',
  // Telemetry = 'telemetry',
  Telemetry_ = 'telemetry_'
}

type BivariantCallback<T extends (...args: never[]) => unknown> = {
  __bivariant(...args: Parameters<T>): ReturnType<T>
}['__bivariant']

export type ValueTransform<StoreValue = unknown, ViewValue = StoreValue> = {
  toView?: BivariantCallback<(value: StoreValue) => ViewValue>
  fromView?: BivariantCallback<(value: ViewValue, prev?: StoreValue) => StoreValue>
  format?: BivariantCallback<(value: ViewValue) => string>
}

export type NodeMeta = {
  page?: {
    title?: string
    labelTitle?: string
    description?: string
    labelDescription?: string
  }
  displayValue?: boolean
  displayValueUnit?: string
  valueTransform?: ValueTransform<unknown, unknown>
  transform?: (value: unknown, prev?: unknown) => unknown
}

export type BaseFieldNode = NodeMeta & {
  label: string // TODO deleted in favor of i18n
  labelKey?: string
  path: string
}

export type CheckboxNode = BaseFieldNode & {
  type: 'checkbox'
}

export type NumberNode = BaseFieldNode & {
  type: 'number'
  min?: number
  max?: number
  step?: number
  default?: number
}

export type StringNode = BaseFieldNode & {
  type: 'string'
}

export type ColorNode = BaseFieldNode & {
  type: 'color'
}

export type SelectNode = BaseFieldNode & {
  type: 'select'
  options: Array<{
    label: string // TODO deleted in favor of i18n
    labelKey?: string
    value: string | number
  }>
}

export type ToggleNode = BaseFieldNode & {
  type: 'toggle'
}

export type SliderNode = BaseFieldNode & {
  type: 'slider'
}

export type KeyBindingKey =
  | 'up'
  | 'down'
  | 'left'
  | 'right'
  | 'selectUp'
  | 'selectDown'
  | 'back'
  | 'home'
  | 'playPause'
  | 'play'
  | 'pause'
  | 'next'
  | 'prev'
  | 'acceptPhone'
  | 'rejectPhone'
  | 'siri'

export type KeyBindingNode = NodeMeta & {
  type: 'keybinding'
  label: string // TODO deleted in favor of i18n
  labelKey?: string
  path: string
  bindingKey: KeyBindingKey
  defaultValue?: string
  resetLabel?: string
  clearLabel?: string
}

export type SettingsCustomPageProps<TStore, TValue> = {
  state: TStore
  node: SettingsCustomNode<TStore, TValue>
  onChange: BivariantCallback<(v: TValue) => void>
  requestRestart?: () => void
}

export type SettingsCustomNode<TStore, TValue = unknown> = NodeMeta & {
  type: 'custom'
  label: string // TODO deleted in favor of i18n
  labelKey?: string
  path: string
  component: React.ComponentType<SettingsCustomPageProps<TStore, TValue>>
}

export type PosListItem = {
  id: string
  label: string // TODO deleted in favor of i18n
  labelKey?: string
}

export type PosListNode = NodeMeta & {
  type: 'posList'
  label: string // TODO deleted in favor of i18n
  labelKey?: string
  path: string
  items: PosListItem[]
}

export type RouteNode<TStore> = NodeMeta & {
  type: 'route'
  label: string // TODO deleted in favor of i18n
  labelKey?: string
  route: string
  path: string
  children: SettingsNode<TStore>[]
}

export type SettingsNode<TStore> =
  | RouteNode<TStore>
  | ToggleNode
  | CheckboxNode
  | SelectNode
  | NumberNode
  | StringNode
  | ColorNode
  | SliderNode
  | KeyBindingNode
  | SettingsCustomNode<TStore>
  | PosListNode
