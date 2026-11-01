#!/bin/sh
set -eu
cd "$(dirname "$0")/../native"
exec rustfmt "$@"
