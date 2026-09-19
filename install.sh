#!/usr/bin/env bash
# otelview installer — downloads the latest release binary for this platform.
#
#   curl -fsSL https://raw.githubusercontent.com/tsirysndr/otelview/main/install.sh | bash
#
# Options (environment variables):
#   OTELVIEW_VERSION   tag to install, e.g. v0.1.0 (default: latest release)
#   OTELVIEW_INSTALL   install directory (default: /usr/local/bin, falls back
#                      to ~/.local/bin when not writable)
set -euo pipefail

REPO="tsirysndr/otelview"

main() {
  local os arch triple version asset url tmp dest

  case "$(uname -s)" in
    Darwin) os="apple-darwin" ;;
    Linux)  os="unknown-linux-gnu" ;;
    *) err "unsupported OS: $(uname -s)" ;;
  esac
  case "$(uname -m)" in
    arm64|aarch64) arch="aarch64" ;;
    x86_64|amd64)  arch="x86_64" ;;
    *) err "unsupported architecture: $(uname -m)" ;;
  esac
  triple="${arch}-${os}"
  [ "$triple" = "x86_64-apple-darwin" ] && err "no pre-built binary for Intel macs yet — build from source or use Docker"

  version="${OTELVIEW_VERSION:-}"
  if [ -z "$version" ]; then
    version=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
      | grep '"tag_name"' | head -1 | cut -d '"' -f 4)
    [ -n "$version" ] || err "could not determine the latest release"
  fi

  asset="otelview-${version}-${triple}.tar.gz"
  url="https://github.com/${REPO}/releases/download/${version}/${asset}"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT

  say "downloading otelview ${version} (${triple})"
  curl -fsSL -o "${tmp}/${asset}" "$url"

  if curl -fsSL -o "${tmp}/${asset}.sha256" "${url}.sha256" 2>/dev/null; then
    (cd "$tmp" && shasum -a 256 -c "${asset}.sha256" >/dev/null) \
      && say "checksum verified" \
      || err "checksum verification failed"
  fi

  tar -xzf "${tmp}/${asset}" -C "$tmp"

  dest="${OTELVIEW_INSTALL:-/usr/local/bin}"
  if [ ! -w "$dest" ] && [ -z "${OTELVIEW_INSTALL:-}" ]; then
    dest="${HOME}/.local/bin"
    mkdir -p "$dest"
  fi
  install -m 755 "${tmp}/otelview" "${dest}/otelview"

  say "installed ${dest}/otelview"
  case ":$PATH:" in
    *":${dest}:"*) ;;
    *) say "note: ${dest} is not on your PATH" ;;
  esac
  "${dest}/otelview" --version
}

say() { printf 'otelview: %s\n' "$1"; }
err() { printf 'otelview: error: %s\n' "$1" >&2; exit 1; }

main "$@"
