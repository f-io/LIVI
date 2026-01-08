import type React from 'react'

export enum RoutePath {
  Home = 'home',
  OldSettings = 'old-settings',
  Settings = 'settings',
  Camera = 'camera',
  Media = 'media',
  Info = 'info'
}

export type ValueTransform<StoreValue = any, ViewValue = StoreValue> = {
  toView?: (value: StoreValue) => ViewValue
  fromView?: (value: ViewValue, prev?: StoreValue) => StoreValue
  format?: (value: ViewValue) => string
}

export type NodeMeta = {
  page?: {
    title?: string
    description?: string
  }
  displayValue?: boolean
  displayValueUnit?: string
  valueTransform?: ValueTransform<any, any>
  transform?: (value: any, prev?: any) => any
}

export type BaseFieldNode = NodeMeta & {
  label: string
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
  options: Array<{ label: string; value: string | number }>
}

export type ToggleNode = BaseFieldNode & {
  type: 'toggle'
}

export type SliderNode = BaseFieldNode & {
  type: 'slider'
}

export type SettingsCustomPageProps<TStore, TValue> = {
  state: TStore
  node: SettingsCustomNode<TStore>
  onChange: (v: TValue) => void
  requestRestart?: () => void
}

export type SettingsCustomNode<TStore = any> = NodeMeta & {
  type: 'custom'
  label: string
  path: string
  component: React.ComponentType<SettingsCustomPageProps<TStore, any>>
}

export type RouteNode<TStore = any> = NodeMeta & {
  type: 'route'
  label: string
  route: string
  path: string
  children: SettingsNode<TStore>[]
}

export type SettingsNode<TStore = any> =
  | RouteNode<TStore>
  | ToggleNode
  | CheckboxNode
  | SelectNode
  | NumberNode
  | StringNode
  | ColorNode
  | SliderNode
  | SettingsCustomNode<TStore>
