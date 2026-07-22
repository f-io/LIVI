import { releaseFeedUrl, runNumberFromTitle } from '@main/ipc/update/feed'

describe('releaseFeedUrl', () => {
  const feed = process.env.UPDATE_FEED
  const repo = process.env.UPDATE_REPO

  beforeEach(() => {
    delete process.env.UPDATE_FEED
    delete process.env.UPDATE_REPO
  })

  afterEach(() => {
    if (feed === undefined) delete process.env.UPDATE_FEED
    else process.env.UPDATE_FEED = feed
    if (repo === undefined) delete process.env.UPDATE_REPO
    else process.env.UPDATE_REPO = repo
  })

  test('release takes the latest tagged release', () => {
    expect(releaseFeedUrl(false)).toBe('https://api.github.com/repos/f-io/LIVI/releases/latest')
  })

  test('nightly takes the rolling prerelease tag', () => {
    expect(releaseFeedUrl(true)).toBe(
      'https://api.github.com/repos/f-io/LIVI/releases/tags/nightly'
    )
  })

  test('UPDATE_REPO redirects both channels', () => {
    process.env.UPDATE_REPO = 'someone/fork'
    expect(releaseFeedUrl(false)).toBe('https://api.github.com/repos/someone/fork/releases/latest')
    expect(releaseFeedUrl(true)).toBe(
      'https://api.github.com/repos/someone/fork/releases/tags/nightly'
    )
  })

  test('UPDATE_FEED overrides the channel entirely', () => {
    process.env.UPDATE_FEED = 'https://example.test/feed.json'
    expect(releaseFeedUrl(false)).toBe('https://example.test/feed.json')
    expect(releaseFeedUrl(true)).toBe('https://example.test/feed.json')
  })

  test('runNumberFromTitle reads the run out of the nightly title', () => {
    expect(runNumberFromTitle('Nightly #123 (0f404b2)')).toBe('123')
    expect(runNumberFromTitle('Nightly #7 (abc1234)')).toBe('7')
    expect(runNumberFromTitle('v8.0.0 (22.07.2026)')).toBe('')
    expect(runNumberFromTitle(undefined)).toBe('')
  })
})
