#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/../.." && pwd)"
common_dir="${repo_root}/package/common"
out_dir="${script_dir}/out"
top_dir="${out_dir}"

for command in cargo curl rpmbuild sha256sum tar; do
  if ! command -v "${command}" >/dev/null 2>&1; then
    printf 'Missing required command: %s\n' "${command}" >&2
    exit 1
  fi
done

# shellcheck source=../common/versions.env
source "${common_dir}/versions.env"

"${common_dir}/fetch-dependencies.sh" dxc

rm -rf "${out_dir}"
mkdir -p \
  "${top_dir}/BUILD" \
  "${top_dir}/BUILDROOT" \
  "${top_dir}/RPMS" \
  "${top_dir}/SOURCES" \
  "${top_dir}/SPECS" \
  "${top_dir}/SRPMS"

source_archive="$("${common_dir}/prepare-source.sh" "${top_dir}/SOURCES")"
cp -f "${WE_LAYERD_DOWNLOAD_CACHE}/${DXC_ARCHIVE}" "${top_dir}/SOURCES/${DXC_ARCHIVE}"
cp -f "${common_dir}/renderer-system-cef.patch" \
  "${top_dir}/SOURCES/renderer-system-cef.patch"
cp -f "${common_dir}/build-without-git-metadata.patch" \
  "${top_dir}/SOURCES/build-without-git-metadata.patch"
cp -f "${script_dir}/we-layerd.spec" "${top_dir}/SPECS/we-layerd.spec"

rpmbuild \
  --define "_topdir ${top_dir}" \
  --define "_sourcedir ${top_dir}/SOURCES" \
  --define "_specdir ${top_dir}/SPECS" \
  --define "_srcrpmdir ${top_dir}/SRPMS" \
  --define "_rpmdir ${top_dir}/RPMS" \
  -ba "${top_dir}/SPECS/we-layerd.spec"

printf '\nBuilt Fedora packages:\n'
find "${top_dir}/RPMS" "${top_dir}/SRPMS" -type f \
  \( -name '*.rpm' -o -name '*.src.rpm' \) -print | sort
