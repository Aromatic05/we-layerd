#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/../.." && pwd)"
output_dir="${1:-${repo_root}/package/source-out}"
version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "${repo_root}/Cargo.toml" | head -n1)"

if [[ -z "${version}" ]]; then
  printf 'Failed to read package version from Cargo.toml\n' >&2
  exit 1
fi

if [[ ! -f "${repo_root}/third_party/wallpaper-engine-renderer/CMakeLists.txt" ]]; then
  printf 'Renderer submodule is missing; run git submodule update --init --recursive\n' >&2
  exit 1
fi

mkdir -p "${output_dir}"
output_dir="$(cd -- "${output_dir}" && pwd)"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT
source_dir="${tmp_dir}/we-layerd-${version}"
mkdir -p "${source_dir}"

# Copy the current checkout, including initialized nested submodules and current
# packaging edits, while excluding build products and VCS metadata.
tar \
  --exclude-vcs \
  --exclude='./target' \
  --exclude='./.deps' \
  --exclude='./package/source-out' \
  --exclude='./package/fedora/out' \
  --exclude='./package/appimage/out' \
  --exclude='./package/ubuntu/out' \
  -C "${repo_root}" -cf - . | tar -C "${source_dir}" -xf -

(
  cd "${source_dir}"
  mkdir -p .cargo
  cargo vendor --quiet --locked --versioned-dirs vendor > .cargo/vendor-sources.toml
  cat .cargo/vendor-sources.toml >> .cargo/config.toml
  rm .cargo/vendor-sources.toml
)

source_date_epoch="$(git -C "${repo_root}" log -1 --format=%ct 2>/dev/null || date +%s)"
archive="${output_dir}/we-layerd-${version}.tar.xz"
tar \
  --sort=name \
  --mtime="@${source_date_epoch}" \
  --owner=0 --group=0 --numeric-owner \
  -C "${tmp_dir}" -cJf "${archive}" "we-layerd-${version}"

printf '%s\n' "${archive}"
