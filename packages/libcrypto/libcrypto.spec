Name: %{_cross_os}libcrypto
Version: 3.0.0
Release: 1%{?dist}
Summary: AWS-LC cryptographic library
License: ISC AND (Apache-2.0 OR ISC) AND OpenSSL
URL: https://github.com/aws/aws-lc

Source0: https://github.com/aws/aws-lc/archive/AWS-LC-FIPS-%{version}/aws-lc-AWS-LC-FIPS-%{version}.tar.gz

# Upstream patches from AWS-LC-FIPS 3.0 branch.
# ```
# git clone https://github.com/aws/aws-lc
# cd aws-lc
# git checkout origin/fips-2024-09-27
# git format-patch --no-numbered AWS-LC-FIPS-3.0.0..
# ```
Patch0001: 0001-Cherry-pick-BORINGSSL_bcm_text_hash-Go-utility-2221.patch
Patch0002: 0002-Cherry-pick-Fix-out-of-bound-OOB-input-read-in-AES-X.patch
Patch0003: 0003-Cherry-pick-support-for-CMake-4.0-to-fips-2024-09-27.patch

BuildRequires: %{_cross_os}glibc-devel
Requires: %{_cross_os}glibc

%description
%{summary}.

%package tools
Summary: Command line tools for the AWS-LC cryptographic library
Requires: %{name}

%description tools
%{summary}.

%package devel
Summary: Files for development using the AWS-LC cryptographic library
Requires: %{name}

%description devel
%{summary}.

%prep
%setup -n aws-lc-AWS-LC-FIPS-%{version}

%build
%cross_cmake \
  -GNinja \
  -DCMAKE_BUILD_TYPE=RelWithDebInfo \
  -DBUILD_SHARED_LIBS=ON \
  -DBUILD_TESTING=OFF \
  -DCMAKE_INSTALL_PREFIX=%{_cross_prefix} \
  -DCMAKE_INSTALL_LIBDIR=%{_cross_libdir} \
  -DCMAKE_SKIP_INSTALL_RPATH=ON \
  -DFIPS=1 \
  %{nil}

%ninja_build

%install
%ninja_install

%files
%license LICENSE NOTICE
%{_cross_attribution_file}
%{_cross_libdir}/libcrypto.so
%{_cross_libdir}/libssl.so

%files tools
%{_cross_bindir}/bssl
%{_cross_bindir}/openssl

%files devel
%{_cross_includedir}/openssl
%{_cross_pkgconfigdir}/*.pc
%exclude %{_cross_libdir}/crypto/cmake
%exclude %{_cross_libdir}/ssl/cmake
