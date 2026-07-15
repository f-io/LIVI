import { MessageHeader, MessageType } from './common'
import { MediaData, MediaType, NavigationData, NavigationMetaType } from './readable'

export function buildMediaJsonMessage(media: Record<string, unknown>): MediaData {
  const json = JSON.stringify(media)
  const payload = Buffer.from(json + '\0', 'utf8')
  return new MediaData(
    new MessageHeader(payload.length, MessageType.MetaData),
    MediaType.Data,
    payload
  )
}

export function buildAlbumArtMessage(albumArt: Buffer): MediaData {
  return new MediaData(
    new MessageHeader(albumArt.length, MessageType.MetaData),
    MediaType.AlbumCover,
    albumArt
  )
}

export function buildNaviJsonMessage(navi: Record<string, unknown>): NavigationData {
  const json = JSON.stringify(navi)
  const payload = Buffer.from(json + '\0', 'utf8')
  return new NavigationData(
    new MessageHeader(payload.length, MessageType.MetaData),
    NavigationMetaType.DashboardInfo,
    payload
  )
}

export function buildNaviImageMessage(image: Buffer): NavigationData {
  return new NavigationData(
    new MessageHeader(image.length, MessageType.MetaData),
    NavigationMetaType.DashboardImage,
    image
  )
}
