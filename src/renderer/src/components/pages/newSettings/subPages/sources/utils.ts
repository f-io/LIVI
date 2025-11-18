export function coerceSelectValue<T extends string | number>(
  value: number,
  options: string[]
): '' | T {
  // TODO FIXME
  // eslint-disable-next-line @typescript-eslint/ban-ts-comment
  // @ts-ignore
  return value != null && options.includes(value as T) ? (value as T) : ''
}
