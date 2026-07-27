#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/../.." && pwd)"
common_dir="${repo_root}/package/common"
out_dir="${script_dir}/out"
work_dir="${out_dir}/work"
vendor_dir="${out_dir}/vendor"
version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "${repo_root}/Cargo.toml" | head -n1)"

for command in cargo curl dpkg-buildpackage sha256sum tar; do
  if ! command -v "${command}" >/dev/null 2>&1; then
    printf 'Missing required command: %s\n' "${command}" >&2
    exit 1
  fi
done

# shellcheck source=../common/versions.env
source "${common_dir}/versions.env"
"${common_dir}/fetch-dependencies.sh" all

rm -rf "${out_dir}"
mkdir -p "${work_dir}" "${vendor_dir}/cef" "${vendor_dir}/dxc"

source_archive="$("${common_dir}/prepare-source.sh" "${work_dir}")"
orig_archive="${work_dir}/we-layerd_${version}.orig.tar.xz"
cp -f "${source_archive}" "${orig_archive}"
tar -xJf "${source_archive}" -C "${work_dir}"
source_dir="${work_dir}/we-layerd-${version}"
cp -a "${script_dir}/debian" "${source_dir}/debian"
chmod 0755 "${source_dir}/debian/rules"
mkdir -p "${source_dir}/debian/patches"
cp -f "${common_dir}/build-without-git-metadata.patch" \
  "${source_dir}/debian/patches/build-without-git-metadata.patch"
printf '%s\n' \
  build-without-git-metadata.patch \
  > "${source_dir}/debian/patches/series"

tar -xjf "${WE_LAYERD_DOWNLOAD_CACHE}/${CEF_ARCHIVE}" \
  -C "${vendor_dir}/cef" --strip-components=1
tar -xzf "${WE_LAYERD_DOWNLOAD_CACHE}/${DXC_ARCHIVE}" \
  -C "${vendor_dir}/dxc"

export CEF_ROOT="${vendor_dir}/cef"
export DXC_ROOT="${vendor_dir}/dxc"
export CMAKE_PREFIX_PATH="${DXC_ROOT}"
export PATH="${DXC_ROOT}/bin:${PATH}"

(
  cd "${source_dir}"
  dpkg_build_args=(--build=binary --no-sign)
  if [[ -n "${WE_LAYERD_PREBUILT_ROOT:-}" ]]; then
    dpkg_build_args+=(--no-check-builddeps)
  fi
  dpkg-buildpackage "${dpkg_build_args[@]}"
)

find "${work_dir}" -maxdepth 1 -type f \
  \( -name '*.deb' -o -name '*.ddeb' -o -name '*.buildinfo' -o -name '*.changes' \) \
  -exec mv -f {} "${out_dir}/" \;

printf '\nBuilt Ubuntu packages:\n'
find "${out_dir}" -maxdepth 1 -type f \
  \( -name '*.deb' -o -name '*.ddeb' \) -print | sort
