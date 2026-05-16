import type { SelectNode, SelectOption } from '../../../../routes/types'

const cache = new Map<string, SelectOption[]>()
const inflight = new Map<string, Promise<SelectOption[]>>()

const keyOf = (node: Pick<SelectNode, 'path'>): string => node.path

export function getCachedOptions(node: Pick<SelectNode, 'path'>): SelectOption[] | undefined {
  return cache.get(keyOf(node))
}

export function setCachedOptions(node: Pick<SelectNode, 'path'>, options: SelectOption[]): void {
  cache.set(keyOf(node), options)
}

export async function resolveOptions(
  node: Pick<SelectNode, 'path' | 'options' | 'loadOptions'>,
  { force = false }: { force?: boolean } = {}
): Promise<SelectOption[]> {
  const key = keyOf(node)

  if (!force) {
    const cached = cache.get(key)
    if (cached) return cached
    const pending = inflight.get(key)
    if (pending) return pending
  }

  if (!node.loadOptions) return node.options

  const promise = (async () => {
    try {
      const resolved = await node.loadOptions!()
      cache.set(key, resolved)
      return resolved
    } catch {
      return node.options
    } finally {
      inflight.delete(key)
    }
  })()
  inflight.set(key, promise)
  return promise
}

// Test-only helper.
export function _resetSelectOptionsCache(): void {
  cache.clear()
  inflight.clear()
}
