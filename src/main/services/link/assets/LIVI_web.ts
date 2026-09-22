/** The dongle's own page, kept as the file it is. Rust embeds the same file for the persistent
 *  install. */

import html from '../../../../../native/livi-helperd/bin/livi-link/web/index.html?raw'

export const buildLiviWeb = (): string => html
