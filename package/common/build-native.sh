#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/../.." && pwd)"
renderer_root="${repo_root}/third_party/wallpaper-engine-renderer"
build_root="${WE_LAYERD_NATIVE_BUILD_ROOT:-${repo_root}/target/we-renderer-upstream/build}"
install_root="${WE_LAYERD_NATIVE_INSTALL_ROOT:-${repo_root}/target/we-renderer-upstream/install}"

for command in cmake strip; do
  if ! command -v "${command}" >/dev/null 2>&1; then
    printf 'Missing required command: %s\n' "${command}" >&2
    exit 1
  fi
done

if [[ ! -f "${renderer_root}/CMakeLists.txt" ]]; then
  printf 'Renderer submodule is missing at %s\n' "${renderer_root}" >&2
  exit 1
fi

if [[ -f "${install_root}/lib/libwallpaper-engine-renderer.so" && \
      -f "${install_root}/lib/we-cef-helper" && \
      "${WE_LAYERD_REUSE_NATIVE_INSTALL:-0}" == 1 ]]; then
  printf 'Using cached native renderer install at %s\n' "${install_root}"
  exit 0
fi

cmake_args=(
  -S "${renderer_root}"
  -B "${build_root}"
  -DCMAKE_BUILD_TYPE=Release
  -DCMAKE_INSTALL_PREFIX="${install_root}"
  -DCMAKE_INSTALL_LIBDIR=lib
  -DBUILD_WEWEB=ON
)
if [[ -n "${CMAKE_C_COMPILER_LAUNCHER:-}" ]]; then
  cmake_args+=("-DCMAKE_C_COMPILER_LAUNCHER=${CMAKE_C_COMPILER_LAUNCHER}")
fi
if [[ -n "${CMAKE_CXX_COMPILER_LAUNCHER:-}" ]]; then
  cmake_args+=("-DCMAKE_CXX_COMPILER_LAUNCHER=${CMAKE_CXX_COMPILER_LAUNCHER}")
fi
cmake "${cmake_args[@]}"

cmake --build "${build_root}" \
  --target wallpaper-engine-renderer we-cef-helper \
  --parallel "${CMAKE_BUILD_PARALLEL_LEVEL:-1}"
cmake --install "${build_root}"

for artifact in \
  "${install_root}/lib/libwallpaper-engine-renderer.so" \
  "${install_root}/lib/we-cef-helper"; do
  if [[ ! -f "${artifact}" ]]; then
    printf 'Native build did not produce %s\n' "${artifact}" >&2
    exit 1
  fi
  strip --strip-unneeded "${artifact}"
done
