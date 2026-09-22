/** The dongle's own CGI, kept as the file it is so nothing is lost to quoting on the way in.
 *  Rust embeds the same file for the persistent install. */

import cgi from '../../../../../native/livi-helperd/bin/livi-link/web/server.cgi?raw'

export const buildServerCgiScript = (): string => cgi
