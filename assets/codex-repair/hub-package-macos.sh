#!/bin/sh
# Invoked only by the native repair window, optionally through macOS's normal
# administrator authorization dialog. Paths are arguments, never shell code.
set -eu
app=$1
candidate=$2
staged=$3
previous=$4
expected=$5
prepared=$6
mode=${7:-swap}
case "$app" in /*.app) ;; *) exit 40;; esac
parent=$(/usr/bin/dirname "$app")
case "$staged" in "$parent"/.hub-repair-*.app) ;; *) exit 41;; esac
case "$previous" in "$parent"/.hub-before-*.app) ;; *) exit 42;; esac
current_hash() { /usr/bin/shasum -a 256 "$1/Contents/Resources/app.asar" | /usr/bin/awk '{print $1}'; }
if [ "$mode" = recover ]; then
  [ ! -e "$app" ] && [ "$(current_hash "$previous")" = "$expected" ] || exit 51
  /bin/mv "$previous" "$app"
  exit 0
fi
[ "$mode" = swap ] || exit 52
[ ! -e "$staged" ] && [ ! -e "$previous" ] || exit 43
[ "$(current_hash "$app")" = "$expected" ] || exit 44
[ "$(current_hash "$candidate")" = "$prepared" ] || exit 45
if /usr/bin/pgrep -x 'ChatGPT|Codex' >/dev/null 2>&1; then exit 46; fi
/usr/bin/ditto --rsrc --extattr "$candidate" "$staged"
/usr/bin/codesign --verify --deep --strict "$staged"
[ "$(current_hash "$staged")" = "$prepared" ] || exit 47
[ "$(current_hash "$app")" = "$expected" ] || exit 48
/bin/mv "$app" "$previous"
if ! /bin/mv "$staged" "$app"; then /bin/mv "$previous" "$app"; exit 49; fi
if ! /usr/bin/codesign --verify --deep --strict "$app" || [ "$(current_hash "$app")" != "$prepared" ]; then
  /bin/mv "$app" "$staged"
  /bin/mv "$previous" "$app"
  exit 50
fi
# Keep the old same-parent bundle until Rust verifies the complete tree. The
# caller never recursively deletes it through this privileged executor.
