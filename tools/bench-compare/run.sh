#!/bin/sh
set -eu
exec pwsh -NoProfile -File "$(dirname "$0")/run.ps1" "$@"
