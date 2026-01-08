export interface SettingsItemRowProps<T extends Record<string, any>, K extends keyof T> {
  config: Record<
    K,
    {
      label: string
      type: 'route' | 'toggle' | 'checkbox' | string
      path?: string
    }
  >
  item: K
  state: T
  transformer?: () => boolean
  onClick?: () => void
  onChange: (key: K, value: T[K]) => void
}
