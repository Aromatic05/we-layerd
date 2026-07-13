#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/../.." && pwd)"
common_dir="${repo_root}/package/common"
out_dir="${script_dir}/out"
work_dir="${out_dir}/work"
appdir="${out_dir}/we-layerd.AppDir"
vendor_dir="${out_dir}/vendor"
tools_dir="${out_dir}/tools"
version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "${repo_root}/Cargo.toml" | head -n1)"
architecture="$(uname -m)"

if [[ "${architecture}" != "x86_64" ]]; then
  printf 'AppImage packaging currently supports x86_64 only; got %s\n' "${architecture}" >&2
  exit 1
fi

for command in cargo curl file find gcc grep gst-inspect-1.0 install patch patchelf readelf sha256sum strip tar xdotool; do
  if ! command -v "${command}" >/dev/null 2>&1; then
    printf 'Missing required command: %s\n' "${command}" >&2
    exit 1
  fi
done

# shellcheck source=../common/versions.env
source "${common_dir}/versions.env"
"${common_dir}/fetch-dependencies.sh" appimage
linuxdeploy="${WE_LAYERD_DOWNLOAD_CACHE}/${LINUXDEPLOY_ARCHIVE}"
appimage_runtime="${WE_LAYERD_DOWNLOAD_CACHE}/${APPIMAGE_RUNTIME_ARCHIVE}"
chmod 0755 "${linuxdeploy}" "${appimage_runtime}"

rm -rf "${out_dir}"
mkdir -p "${work_dir}" "${vendor_dir}/cef" "${vendor_dir}/dxc" "${appdir}" "${tools_dir}"

source_archive="$("${common_dir}/prepare-source.sh" "${work_dir}")"
tar -xJf "${source_archive}" -C "${work_dir}"
source_dir="${work_dir}/we-layerd-${version}"

(
  cd "${source_dir}"
  patch -p1 < "${common_dir}/build-without-git-metadata.patch"
)

tar -xjf "${WE_LAYERD_DOWNLOAD_CACHE}/${CEF_ARCHIVE}" \
  -C "${vendor_dir}/cef" --strip-components=1
tar -xzf "${WE_LAYERD_DOWNLOAD_CACHE}/${DXC_ARCHIVE}" \
  -C "${vendor_dir}/dxc"

export CEF_ROOT="${vendor_dir}/cef"
export DXC_ROOT="${vendor_dir}/dxc"
export CMAKE_PREFIX_PATH="${DXC_ROOT}"
export PATH="${DXC_ROOT}/bin:${PATH}"
export WE_LAYERD_INSTALL_PREFIX=/usr
export CARGO_NET_OFFLINE=true

(
  cd "${source_dir}"
  cargo build --frozen --offline --release -p we-layerd -p we-gui
)

install -Dm0755 "${source_dir}/target/release/we-layerd" "${appdir}/usr/bin/we-layerd"
install -Dm0755 "${source_dir}/target/release/we-gui" "${appdir}/usr/bin/we-gui"
install -Dm0755 "$(command -v xdotool)" "${appdir}/usr/bin/xdotool"
install -Dm0755 \
  "${source_dir}/target/we-renderer-upstream/install/lib/libwallpaper-engine-renderer.so" \
  "${appdir}/usr/lib/libwallpaper-engine-renderer.so"
install -Dm0755 \
  "${source_dir}/target/we-renderer-upstream/install/lib/we-cef-helper" \
  "${appdir}/usr/lib/we-cef-helper"

install -d "${appdir}/usr/lib/we-layerd/dxc"
install -m0755 "${vendor_dir}/dxc/lib/libdxcompiler.so" \
  "${appdir}/usr/lib/we-layerd/dxc/libdxcompiler.so"
install -m0755 "${vendor_dir}/dxc/lib/libdxil.so" \
  "${appdir}/usr/lib/we-layerd/dxc/libdxil.so"

install -d "${appdir}/usr/lib/cef"
cp -a "${vendor_dir}/cef/Release/." "${appdir}/usr/lib/cef/"
cp -a "${vendor_dir}/cef/Resources/." "${appdir}/usr/lib/cef/"
rm -f "${appdir}/usr/lib/cef/chrome-sandbox"

install -d "${appdir}/usr/share/we-layerd/gnome-shell-extension"
cp -a "${source_dir}/contrib/gnome-shell-extension/we-layerd@aromatic" \
  "${appdir}/usr/share/we-layerd/gnome-shell-extension/"

install -Dm0644 "${vendor_dir}/cef/LICENSE.txt" \
  "${appdir}/usr/share/licenses/we-layerd/CEF-LICENSE.txt"
install -Dm0644 "${vendor_dir}/dxc/LICENSE-MS.txt" \
  "${appdir}/usr/share/licenses/we-layerd/DXC-LICENSE-MS.txt"
install -Dm0644 "${vendor_dir}/dxc/LICENSE-LLVM.txt" \
  "${appdir}/usr/share/licenses/we-layerd/DXC-LICENSE-LLVM.txt"

multiarch="$(gcc -print-multiarch)"
nss_library_dir="${NSS_LIBRARY_DIR:-/usr/lib/${multiarch}}"
nss_modules=(
  libfreebl3.so
  libfreeblpriv3.so
  libnssckbi.so
  libnssdbm3.so
  libsoftokn3.so
)
nss_deploy_args=()
for module in "${nss_modules[@]}"; do
  if [[ ! -f "${nss_library_dir}/${module}" ]]; then
    printf 'Required NSS runtime module not found: %s\n' "${nss_library_dir}/${module}" >&2
    exit 1
  fi
  install -Dm0755 "${nss_library_dir}/${module}" "${appdir}/usr/lib/${module}"
  nss_deploy_args+=(--deploy-deps-only "${appdir}/usr/lib/${module}")
done

gstreamer_plugins_dir="${GSTREAMER_PLUGINS_DIR:-/usr/lib/${multiarch}/gstreamer-1.0}"
gstreamer_helpers_dir="${GSTREAMER_HELPERS_DIR:-/usr/lib/${multiarch}/gstreamer1.0/gstreamer-1.0}"
if [[ ! -d "${gstreamer_plugins_dir}" ]]; then
  printf 'GStreamer plugin directory not found: %s\n' "${gstreamer_plugins_dir}" >&2
  exit 1
fi
if [[ ! -x "${gstreamer_helpers_dir}/gst-plugin-scanner" ]]; then
  printf 'GStreamer plugin scanner not found: %s\n' "${gstreamer_helpers_dir}/gst-plugin-scanner" >&2
  exit 1
fi
install -d "${appdir}/usr/lib/gstreamer-1.0" \
  "${appdir}/usr/lib/gstreamer1.0/gstreamer-1.0"
required_gstreamer_plugins=(
  libgstapp.so
  libgstcoreelements.so
  libgstgio.so
  libgstisomp4.so
  libgstlibav.so
  libgstplayback.so
  libgsttypefindfunctions.so
  libgstvideoconvertscale.so
  libgstvideoparsersbad.so
)
optional_gstreamer_plugins=(
  libgstva.so
  libgstnvcodec.so
)
for plugin in "${required_gstreamer_plugins[@]}"; do
  if [[ ! -f "${gstreamer_plugins_dir}/${plugin}" ]]; then
    printf 'Required GStreamer plugin not found: %s\n' "${gstreamer_plugins_dir}/${plugin}" >&2
    exit 1
  fi
  cp -a "${gstreamer_plugins_dir}/${plugin}" "${appdir}/usr/lib/gstreamer-1.0/"
done
for plugin in "${optional_gstreamer_plugins[@]}"; do
  if [[ -f "${gstreamer_plugins_dir}/${plugin}" ]]; then
    cp -a "${gstreamer_plugins_dir}/${plugin}" "${appdir}/usr/lib/gstreamer-1.0/"
  fi
done
install -m0755 "${gstreamer_helpers_dir}/gst-plugin-scanner" \
  "${appdir}/usr/lib/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner"
if [[ -x "${gstreamer_helpers_dir}/gst-ptp-helper" ]]; then
  install -m0755 "${gstreamer_helpers_dir}/gst-ptp-helper" \
    "${appdir}/usr/lib/gstreamer1.0/gstreamer-1.0/gst-ptp-helper"
fi

# Remove the very large debug sections from pinned upstream binary distributions.
strip --strip-unneeded \
  "${appdir}/usr/lib/cef/libcef.so" \
  "${appdir}/usr/lib/cef/libEGL.so" \
  "${appdir}/usr/lib/cef/libGLESv2.so" \
  "${appdir}/usr/lib/cef/libvk_swiftshader.so" \
  "${appdir}/usr/lib/cef/libvulkan.so.1" \
  "${appdir}/usr/lib/we-layerd/dxc/libdxcompiler.so" \
  "${appdir}/usr/lib/we-layerd/dxc/libdxil.so"

exclude_args=()
while IFS= read -r pattern; do
  [[ -z "${pattern}" || "${pattern}" == \#* ]] && continue
  exclude_args+=(--exclude-library "${pattern}")
done < "${script_dir}/excluded-libraries.txt"

export LD_LIBRARY_PATH="${appdir}/usr/lib:${appdir}/usr/lib/cef:${appdir}/usr/lib/we-layerd/dxc${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
APPIMAGE_EXTRACT_AND_RUN=1 "${linuxdeploy}" \
  --appdir "${appdir}" \
  --deploy-deps-only "${appdir}/usr/bin" \
  --deploy-deps-only "${appdir}/usr/lib/libwallpaper-engine-renderer.so" \
  --deploy-deps-only "${appdir}/usr/lib/we-cef-helper" \
  --deploy-deps-only "${appdir}/usr/lib/cef/libcef.so" \
  --deploy-deps-only "${appdir}/usr/lib/gstreamer-1.0" \
  --deploy-deps-only "${appdir}/usr/lib/gstreamer1.0" \
  "${nss_deploy_args[@]}" \
  --desktop-file "${source_dir}/apps/we-gui/assets/we-gui.desktop" \
  --icon-file "${source_dir}/apps/we-gui/assets/we-gui-logo.svg" \
  --icon-filename we-gui \
  --custom-apprun "${script_dir}/AppRun" \
  "${exclude_args[@]}"

# linuxdeploy may normalize RUNPATH. Reapply the private runtime layout explicitly.
patchelf --set-rpath '$ORIGIN:$ORIGIN/we-layerd/dxc' \
  "${appdir}/usr/lib/libwallpaper-engine-renderer.so"
patchelf --set-rpath '$ORIGIN:$ORIGIN/cef' \
  "${appdir}/usr/lib/we-cef-helper"
find "${appdir}/usr/lib/gstreamer-1.0" -maxdepth 1 -type f -name '*.so' \
  -exec patchelf --set-rpath '$ORIGIN:$ORIGIN/..' {} \;
find "${appdir}/usr/lib/gstreamer1.0/gstreamer-1.0" -maxdepth 1 -type f \
  -exec sh -c 'file "$1" | grep -q ELF && patchelf --set-rpath '\''$ORIGIN/../..:$ORIGIN/../../..'\'' "$1" || true' sh {} \;

forbidden_name_regex='^(ld-linux[^/]*\.so(\..*)?|lib(c|pthread|dl|rt|m|resolv|util|anl|BrokenLocale)\.so(\..*)?|libnss_[^/]*\.so(\..*)?|lib(memusage|pcprofile|SegFault)\.so(\..*)?)$'
audit_no_glibc() {
  local tree="$1"
  local found
  found="$(find -L "${tree}" \( -type f -o -type l \) -printf '%f\n' | grep -E "${forbidden_name_regex}" || true)"
  if [[ -n "${found}" ]]; then
    printf 'Forbidden glibc or dynamic-loader files were bundled under %s:\n%s\n' \
      "${tree}" "${found}" >&2
    return 1
  fi
}

audit_no_host_graphics() {
  local tree="$1"
  local found
  found="$(find -L "${tree}/usr/lib" -maxdepth 1 \( -type f -o -type l \) -printf '%f\n' | \
    grep -E '^(lib(GL|EGL|GLES|OpenGL|glapi|drm|gbm|vulkan|va|vdpau)[^/]*\.so(\..*)?)$' || true)"
  if [[ -n "${found}" ]]; then
    printf 'Host graphics loader or driver libraries were bundled under %s/usr/lib:\n%s\n' \
      "${tree}" "${found}" >&2
    return 1
  fi
}

audit_no_executable_stack() {
  local tree="$1"
  local path
  local state
  local failed=0
  while IFS= read -r -d '' path; do
    if ! file -b "${path}" | grep -q '^ELF '; then
      continue
    fi
    state="$(patchelf --print-execstack "${path}" 2>/dev/null || true)"
    if [[ "${state}" == 'execstack: X' ]]; then
      printf 'ELF file requests an executable stack: %s\n' "${path}" >&2
      failed=1
    fi
  done < <(find -L "${tree}" -type f -print0)
  return "${failed}"
}

smoke_test_cef_helper_dlopen() {
  local helper="$1"
  local source_file="${tools_dir}/cef-helper-dlopen-smoke.c"
  local test_binary="${tools_dir}/cef-helper-dlopen-smoke"
  cat > "${source_file}" <<'EOF'
#include <dlfcn.h>
#include <stdio.h>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    void* handle = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (handle == NULL) {
        fprintf(stderr, "dlopen failed for %s: %s\n", argv[1], dlerror());
        return 1;
    }
    if (dlsym(handle, "we_cef_get_host_api") == NULL) {
        fprintf(stderr, "we_cef_get_host_api is missing: %s\n", dlerror());
        dlclose(handle);
        return 1;
    }
    dlclose(handle);
    return 0;
}
EOF
  "${CC:-cc}" -O2 -o "${test_binary}" "${source_file}" -ldl
  "${test_binary}" "${helper}"
}

audit_no_glibc "${appdir}"
audit_no_host_graphics "${appdir}"
audit_no_executable_stack "${appdir}"
smoke_test_cef_helper_dlopen "${appdir}/usr/lib/we-cef-helper"

gstreamer_registry="${out_dir}/gstreamer-build-check-registry.bin"
rm -f "${gstreamer_registry}"
for element in filesrc queue qtdemux h264parse decodebin videoconvert appsink giostreamsrc; do
  GST_REGISTRY_1_0="${gstreamer_registry}" \
  GST_PLUGIN_SYSTEM_PATH_1_0="${appdir}/usr/lib/gstreamer-1.0" \
  GST_PLUGIN_PATH_1_0="${appdir}/usr/lib/gstreamer-1.0" \
  GST_PLUGIN_SCANNER_1_0="${appdir}/usr/lib/gstreamer1.0/gstreamer-1.0/gst-plugin-scanner" \
    gst-inspect-1.0 "${element}" >/dev/null
done
rm -f "${gstreamer_registry}"

(
  cd "${tools_dir}"
  "${linuxdeploy}" --appimage-extract >/dev/null
)
appimagetool="${tools_dir}/squashfs-root/plugins/linuxdeploy-plugin-appimage/usr/bin/appimagetool"
if [[ ! -x "${appimagetool}" ]]; then
  printf 'Pinned linuxdeploy does not contain appimagetool at: %s\n' "${appimagetool}" >&2
  exit 1
fi

output_appimage="${out_dir}/we-layerd-${version}-x86_64.AppImage"
rm -f "${output_appimage}"
ARCH=x86_64 "${appimagetool}" \
  --runtime-file "${appimage_runtime}" \
  "${appdir}" "${output_appimage}"
chmod 0755 "${output_appimage}"

extract_dir="$(mktemp -d)"
trap 'rm -rf "${extract_dir}"' EXIT
(
  cd "${extract_dir}"
  "${output_appimage}" --appimage-extract >/dev/null
)
audit_no_glibc "${extract_dir}/squashfs-root"
audit_no_host_graphics "${extract_dir}/squashfs-root"
audit_no_executable_stack "${extract_dir}/squashfs-root"
smoke_test_cef_helper_dlopen "${extract_dir}/squashfs-root/usr/lib/we-cef-helper"

APPIMAGE_EXTRACT_AND_RUN=1 "${output_appimage}" --cli --help >/dev/null

printf '\nBuilt AppImage:\n%s\n' "${output_appimage}"
printf 'Size: %s\n' "$(du -h "${output_appimage}" | cut -f1)"
printf 'Renderer RUNPATH: %s\n' \
  "$(patchelf --print-rpath "${appdir}/usr/lib/libwallpaper-engine-renderer.so")"
printf 'CEF helper RUNPATH: %s\n' \
  "$(patchelf --print-rpath "${appdir}/usr/lib/we-cef-helper")"
