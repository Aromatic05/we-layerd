%global debug_package %{nil}

Name:           we-layerd
Version:        0.2.3
Release:        1%{?dist}
Summary:        Native Wallpaper Engine wallpaper daemon for Wayland

License:        LicenseRef-Unlicensed
URL:            https://github.com/Aromatic05/we-layerd
Source0:        %{name}-%{version}.tar.xz
Source1:        linux_dxc_2026_05_26.x86_64.tar.gz
Patch0:         renderer-system-cef.patch
Patch1:         build-without-git-metadata.patch

ExclusiveArch:  x86_64

BuildRequires:  binutils
BuildRequires:  cargo
BuildRequires:  cef-devel
BuildRequires:  cmake
BuildRequires:  desktop-file-utils
BuildRequires:  fontconfig-devel
BuildRequires:  freetype-devel
BuildRequires:  gcc-c++
BuildRequires:  git
BuildRequires:  gstreamer1-devel
BuildRequires:  gstreamer1-plugins-bad-free-devel
BuildRequires:  gstreamer1-plugins-base-devel
BuildRequires:  gtk3-devel
BuildRequires:  libatomic
BuildRequires:  libdrm-devel
BuildRequires:  libva-devel
BuildRequires:  libxkbcommon-devel
BuildRequires:  lz4-devel
BuildRequires:  mesa-libGL-devel
BuildRequires:  pango-devel
BuildRequires:  patchelf
BuildRequires:  pkgconf-pkg-config
BuildRequires:  rust
BuildRequires:  vulkan-headers
BuildRequires:  vulkan-loader-devel
BuildRequires:  wayland-devel
BuildRequires:  libxdo-devel
BuildRequires:  wayland-protocols-devel

Requires:       cef
Requires:       xdotool
Suggests:       gnome-shell
Recommends:     gstreamer1-plugins-bad-free
Recommends:     gstreamer1-plugins-base
Recommends:     gstreamer1-plugins-good

%description
we-layerd runs Wallpaper Engine wallpapers through a native renderer on
Wayland. It supports layer-shell compositors and GNOME, DMA-BUF and SHM
presentation, interactive pointer input, video wallpapers, web wallpapers,
and a companion GUI.

The project source currently has no published license. This spec is intended
for local package builds until the project adopts a distributable license.

%prep
%autosetup -p1
mkdir -p .deps/dxc
tar -xzf %{SOURCE1} -C .deps/dxc

%build
export CARGO_NET_OFFLINE=true
export CMAKE_PREFIX_PATH="%{_builddir}/%{name}-%{version}/.deps/dxc"
export PATH="%{_builddir}/%{name}-%{version}/.deps/dxc/bin:${PATH}"
export WE_LAYERD_INSTALL_PREFIX=/usr
cargo build --frozen --offline --release -p we-layerd -p we-gui

%install
rm -rf %{buildroot}

install -Dm0755 target/release/we-layerd \
  %{buildroot}%{_bindir}/we-layerd
install -Dm0755 target/release/we-gui \
  %{buildroot}%{_bindir}/we-gui
install -Dm0755 target/we-renderer-upstream/install/lib/libwallpaper-engine-renderer.so \
  %{buildroot}%{_prefix}/lib/libwallpaper-engine-renderer.so
install -Dm0755 target/we-renderer-upstream/install/lib/we-cef-helper \
  %{buildroot}%{_prefix}/lib/we-cef-helper

mkdir -p %{buildroot}%{_datadir}/gnome-shell/extensions/we-layerd@aromatic
cp -a contrib/gnome-shell-extension/we-layerd@aromatic/. \
  %{buildroot}%{_datadir}/gnome-shell/extensions/we-layerd@aromatic/
find %{buildroot}%{_datadir}/gnome-shell/extensions/we-layerd@aromatic \
  -type d -exec chmod 0755 {} +
find %{buildroot}%{_datadir}/gnome-shell/extensions/we-layerd@aromatic \
  -type f -exec chmod 0644 {} +

install -Dm0644 apps/we-gui/assets/we-gui.desktop \
  %{buildroot}%{_datadir}/applications/we-gui.desktop
install -Dm0644 apps/we-gui/assets/we-gui-logo.svg \
  %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/we-gui.svg

desktop-file-validate %{buildroot}%{_datadir}/applications/we-gui.desktop

install -d %{buildroot}%{_prefix}/lib/we-layerd/dxc
install -m0755 .deps/dxc/lib/libdxcompiler.so \
  %{buildroot}%{_prefix}/lib/we-layerd/dxc/libdxcompiler.so
install -m0755 .deps/dxc/lib/libdxil.so \
  %{buildroot}%{_prefix}/lib/we-layerd/dxc/libdxil.so

install -Dm0644 .deps/dxc/LICENSE-MS.txt \
  %{buildroot}%{_licensedir}/%{name}/DXC-LICENSE-MS.txt
install -Dm0644 .deps/dxc/LICENSE-LLVM.txt \
  %{buildroot}%{_licensedir}/%{name}/DXC-LICENSE-LLVM.txt

patchelf --set-rpath '$ORIGIN/we-layerd/dxc' \
  %{buildroot}%{_prefix}/lib/libwallpaper-engine-renderer.so

%check
LD_LIBRARY_PATH=%{buildroot}%{_prefix}/lib \
  %{buildroot}%{_bindir}/we-layerd --help >/dev/null
readelf -d %{buildroot}%{_prefix}/lib/libwallpaper-engine-renderer.so \
  | grep -F '$ORIGIN/we-layerd/dxc'

%files
%{_bindir}/we-layerd
%{_bindir}/we-gui
%{_prefix}/lib/libwallpaper-engine-renderer.so
%{_prefix}/lib/we-cef-helper
%{_prefix}/lib/we-layerd/dxc/
%{_datadir}/applications/we-gui.desktop
%{_datadir}/icons/hicolor/scalable/apps/we-gui.svg
%{_datadir}/gnome-shell/extensions/we-layerd@aromatic/
%license %{_licensedir}/%{name}/DXC-LICENSE-MS.txt
%license %{_licensedir}/%{name}/DXC-LICENSE-LLVM.txt

%changelog
* Mon Jul 27 2026 Aromatic05 <noreply@example.invalid> - 0.2.3-1
- Release 0.2.3

* Tue Jul 14 2026 Aromatic05 <noreply@example.invalid> - 0.2.2-1
- Release 0.2.2

* Mon Jul 13 2026 Aromatic05 <noreply@example.invalid> - 0.2.1-1
- Release 0.2.1

* Sun Jul 12 2026 Aromatic05 <noreply@example.invalid> - 0.2.0-1
- Release 0.2.0

* Sun Jul 12 2026 Aromatic05 <noreply@example.invalid> - 0.1.0-1
- Add a local Fedora 43 package definition
