Name: %{_cross_os}dbus-broker
Version: 37
Release: 1%{?dist}
Summary: D-BUS message broker
License: Apache-2.0
URL: https://github.com/bus1/dbus-broker
Source0: https://github.com/bus1/dbus-broker/releases/download/v%{version}/dbus-broker-%{version}.tar.xz
Source1: https://github.com/bus1/dbus-broker/releases/download/v%{version}/dbus-broker-%{version}.tar.xz.asc
Source2: gpgkey-BE5FBC8C9C1C9F60A4F0AEAE7A4F3A09EBDEFF26.asc

Source11: dbus.socket
Source12: dbus-sysusers.conf

BuildRequires: meson
BuildRequires: %{_cross_os}glibc-devel
BuildRequires: %{_cross_os}libselinux-devel
BuildRequires: %{_cross_os}systemd-devel
Requires: %{_cross_os}libselinux
Requires: %{_cross_os}systemd
Requires: %{_cross_os}dbus-broker(launcher)

# Work around an aliasing rules violation.
Patch0001: 0001-c-utf8-disable-strict-aliasing-optimizations.patch
# Allow building the journal catalogs when dbus-launcher is excluded
Patch0002: 0002-meson.build-remove-condition-to-build-the-journal-ca.patch

%description
%{summary}.

%prep
%{gpgverify} --data=%{S:0} --signature=%{S:1} --keyring=%{S:2}
%autosetup -n dbus-broker-%{version} -p1

%build
CONFIGURE_OPTS=(
 -Dapparmor=false
 -Daudit=false
 -Ddocs=false
 -Dlauncher=false
 -Dselinux=true
 -Dcatalogdir=%{_cross_journalcatalogdir}
)

%cross_meson "${CONFIGURE_OPTS[@]}"
%cross_meson_build

%install
%cross_meson_install

install -d %{buildroot}%{_cross_unitdir}
install -p -m 0644 %{S:11} %{buildroot}%{_cross_unitdir}

install -d %{buildroot}%{_cross_sysusersdir}
install -p -m 0644 %{S:12} %{buildroot}%{_cross_sysusersdir}/dbus.conf

%files
%license LICENSE
%{_cross_attribution_file}
%{_cross_bindir}/dbus-broker
%{_cross_journalcatalogdir}/dbus-broker.catalog
%{_cross_sysusersdir}/dbus.conf
%{_cross_unitdir}/dbus.socket

%changelog
