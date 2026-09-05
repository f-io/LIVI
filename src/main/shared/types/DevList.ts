export type DevListEntry = {
  id?: string
  type?: string
  name?: string
  index?: string | number
  time?: string
  rfcomm?: string | number
  source?: 'host'
  class?: number
  connected?: boolean
}
