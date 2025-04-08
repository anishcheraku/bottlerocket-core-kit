Name: %{_cross_os}libpopt
Version: 1.19
Release: 1%{?dist}
Summary: Library for parsing command line parameters
License: MIT
URL: https://github.com/rpm-software-management/popt
Source0: http://ftp.rpm.org/popt/releases/popt-1.x/popt-%{version}.tar.gz

BuildRequires: %{_cross_os}glibc-devel

%description
%{summary}.

%package devel
Summary: Files for development using the library for parsing command line parameters
Requires: %{name}

%description devel
%{summary}.

%prep
%autosetup -n popt-%{version} -p1

%build
%cross_configure \
  --disable-static \
  --disable-nls \
  --disable-rpath \
  %{nil}

%make_build

%install
%make_install

%files
%license COPYING
%{_cross_attribution_file}
%{_cross_libdir}/libpopt.so.*
%exclude %{_cross_mandir}
%exclude %{_cross_datadir}/locale

%files devel
%{_cross_includedir}/popt.h
%{_cross_libdir}/libpopt.so
%{_cross_pkgconfigdir}/popt.pc
