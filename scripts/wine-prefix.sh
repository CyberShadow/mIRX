# Wine prefix setup shared by the Wine scripts; source it, then call
# `ensure_wineprefix <default prefix dir>`. The caller defines `die`.

# Sets up the Wine environment and makes sure $WINEPREFIX (default: $1) is
# initialised and holds the test fonts from $MIRX_WINE_FONTS. Concurrent
# callers sharing a prefix serialise on a lock, so wineboot runs once and
# no process sees a half-copied font.
ensure_wineprefix() {
  local default_prefix=$1
  : "${MIRX_WINE_FONTS:?run inside nix develop}"
  export WINEPREFIX="${WINEPREFIX:-$default_prefix}"
  export WINEARCH="${WINEARCH:-win64}"
  export WINEDLLOVERRIDES="${WINEDLLOVERRIDES:-mscoree,mshtml=}"
  export WINEDEBUG="${WINEDEBUG:--all}"
  # Timestamps show local time: pin it so the output is the same everywhere.
  export TZ=UTC

  local prefix_lock_name=${WINEPREFIX//\//_}
  local prefix_lock_file="${TMPDIR:-/var/tmp}/mirx-wineprefix-init${prefix_lock_name}.lock"
  exec 8>"$prefix_lock_file"
  flock 8
  mkdir -p "$WINEPREFIX"
  if [[ ! -f "$WINEPREFIX/system.reg" ]]; then
    wineboot -i >/dev/null 2>&1 || die "wineboot failed while initializing $WINEPREFIX"
  fi
  # Wine loads %windir%\Fonts at every process start: copying is enough.
  local fonts_dir="$WINEPREFIX/drive_c/windows/Fonts"
  local font
  for font in "$MIRX_WINE_FONTS"/*; do
    local name
    name=$(basename "$font")
    if ! cmp -s "$font" "$fonts_dir/$name"; then
      cp "$font" "$fonts_dir/$name.tmp"
      chmod u+w "$fonts_dir/$name.tmp"
      mv "$fonts_dir/$name.tmp" "$fonts_dir/$name"
    fi
  done
  flock -u 8
  exec 8>&-
}
