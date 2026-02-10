export interface SettingsItemRowProps<
  T extends Record<string, unknown>,
  K extends Extract<keyof T, string>
> {
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
