/** Rolling prerelease the build workflow replaces on every build of main. */
const NIGHTLY_TAG = 'nightly'

export function releaseFeedUrl(nightly: boolean): string {
  if (process.env.UPDATE_FEED) return process.env.UPDATE_FEED
  const repo = process.env.UPDATE_REPO || 'f-io/LIVI'
  const base = `https://api.github.com/repos/${repo}/releases`
  return nightly ? `${base}/tags/${NIGHTLY_TAG}` : `${base}/latest`
}

export function runNumberFromTitle(title?: string): string {
  const m = (title || '').match(/#(\d+)/)
  return m ? m[1] : ''
}
