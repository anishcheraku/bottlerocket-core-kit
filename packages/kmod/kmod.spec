Name: %{_cross_os}kmod
Version: 34.2
Release: 1%{?dist}
Summary: Tools for kernel module loading and unloading
License: GPL-2.0-or-later AND LGPL-2.1-or-later
URL: http://git.kernel.org/?p=utils/kernel/kmod/kmod.git;a=summary
Source0: https://www.kernel.org/pub/linux/utils/kernel/kmod/kmod-%{version}.tar.xz
Source1: https://www.kernel.org/pub/linux/utils/kernel/kmod/kmod-%{version}.tar.sign
Source2: gpgkey-EAB33C9690013C733916AC839BA2A5A630CBEA53.asc
Patch1001: 0001-meson-add-support-for-static-builds.patch
Patch1002: 0002-meson-create-pkgconfig-files-in-default-path.patch
BuildRequires: %{_cross_os}glibc-devel
BuildRequires: %{_cross_os}libz-devel
BuildRequires: %{_cross_os}libzstd-devel
Requires: %{_cross_os}libz
Requires: %{_cross_os}libzstd

%description
%{summary}.

%package devel
Summary: Files for development using the tools for kernel module loading and unloading
Requires: %{name}
Requires: %{_cross_os}libz-devel
Requires: %{_cross_os}libzstd-devel

%description devel
%{summary}.

%prep
%{gpgverify} --data=<(xzcat %{S:0}) --signature=%{S:1} --keyring=%{S:2}
%autosetup -n kmod-%{version} -p1
cp COPYING COPYING.LGPL
cp tools/COPYING COPYING.GPL

%build
CONFIGURE_OPTS=(
  -Dzlib=enabled
  -Dzstd=enabled
  -Dopenssl=disabled
  -Dmanpages=false
  -Dxz=disabled
  -Dfishcompletiondir=no
  -Dbashcompletiondir=no
  -Dzshcompletiondir=no
)

%global _cross_vpath_builddir dynamic-build
mkdir dynamic-build
%cross_meson \
  --sbindir=%{_cross_bindir} \
  "${CONFIGURE_OPTS[@]}"
%cross_meson_build

%global _cross_vpath_builddir static-build
mkdir static-build
%{cross_meson} \
  --default-library=static \
  --sbindir=%{_cross_bindir} \
  --prefer-static \
  "${CONFIGURE_OPTS[@]}"
export LDFLAGS="-all-static"
%cross_meson_build

%install
pushd dynamic-build
%ninja_install
popd

pushd static-build
install -d %{buildroot}%{_cross_bindir}
install -p kmod %{buildroot}%{_cross_bindir}

install -d %{buildroot}%{_cross_sbindir}
ln -s ../bin/kmod %{buildroot}%{_cross_sbindir}/modprobe
popd

%files
%license COPYING.LGPL COPYING.GPL
%{_cross_attribution_file}
%{_cross_bindir}/kmod
%{_cross_bindir}/depmod
%{_cross_bindir}/insmod
%{_cross_bindir}/lsmod
%{_cross_bindir}/modinfo
%{_cross_bindir}/modprobe
%{_cross_bindir}/rmmod
%{_cross_sbindir}/modprobe
%{_cross_libdir}/*.so.*

%files devel
%{_cross_libdir}/*.so
%{_cross_includedir}/*.h
%{_cross_pkgconfigdir}/*.pc
%changelog
