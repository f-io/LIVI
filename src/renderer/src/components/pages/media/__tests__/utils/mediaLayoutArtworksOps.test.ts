import { mediaLayoutArtworksOps } from '../../utils/mediaLayoutArtworksOps'

describe('mediaLayoutArtworksOps', () => {
  test('uses two-column artwork sizing when there is enough width', () => {
    const result = mediaLayoutArtworksOps({
      ctrlSize: 80,
      progressH: 12,
      w: 1200,
      h: 800,
      pagePad: 24,
      colGap: 24,
      titlePx: 32,
      artistPx: 24,
      albumPx: 20
    })

    expect(result.canTwoCol).toBe(true)
    expect(result.innerW).toBeGreaterThan(0)
    expect(result.artPx).toBeGreaterThanOrEqual(140)
    expect(result.artPx).toBeLessThanOrEqual(520)
  })

  test('uses single-column artwork sizing when there is not enough width', () => {
    const result = mediaLayoutArtworksOps({
      ctrlSize: 80,
      progressH: 12,
      w: 320,
      h: 800,
      pagePad: 24,
      colGap: 24,
      titlePx: 32,
      artistPx: 24,
      albumPx: 20
    })

    expect(result.canTwoCol).toBe(false)
    expect(result.innerW).toBeGreaterThanOrEqual(0)
    expect(result.artPx).toBeGreaterThanOrEqual(130)
    expect(result.artPx).toBeLessThanOrEqual(480)
  })
})
