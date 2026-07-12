#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=versions.env
source "${script_dir}/versions.env"

usage() {
  cat <<'EOF'
Usage: package/common/fetch-dependencies.sh {dxc|cef|all}

Downloads pinned package-build dependencies into WE_LAYERD_DOWNLOAD_CACHE
and verifies every file with SHA-256.
EOF
}

fetch() {
  local name="$1"
  local url="$2"
  local sha256="$3"
  local destination="${WE_LAYERD_DOWNLOAD_CACHE}/${name}"
  local partial="${destination}.part"

  mkdir -p "${WE_LAYERD_DOWNLOAD_CACHE}"

  if [[ -f "${destination}" ]] && printf '%s  %s\n' "${sha256}" "${destination}" | sha256sum --check --status; then
    printf 'Using cached %s\n' "${destination}" >&2
    return
  fi

  rm -f "${destination}"
  curl --fail --location --retry 3 --continue-at - --output "${partial}" "${url}"
  printf '%s  %s\n' "${sha256}" "${partial}" | sha256sum --check --status
  mv -f "${partial}" "${destination}"
  printf 'Downloaded %s\n' "${destination}" >&2
}

case "${1:-}" in
  dxc)
    fetch "${DXC_ARCHIVE}" "${DXC_URL}" "${DXC_SHA256}"
    ;;
  cef)
    fetch "${CEF_ARCHIVE}" "${CEF_URL}" "${CEF_SHA256}"
    ;;
  all)
    fetch "${DXC_ARCHIVE}" "${DXC_URL}" "${DXC_SHA256}"
    fetch "${CEF_ARCHIVE}" "${CEF_URL}" "${CEF_SHA256}"
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
