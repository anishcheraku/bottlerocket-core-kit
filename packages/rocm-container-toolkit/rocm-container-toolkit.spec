%global goproject github.com/ROCm
%global gorepo container-toolkit
%global goimport %{goproject}/%{gorepo}

%global gover 1.2.0
%global rpmver %{gover}

Name: %{_cross_os}rocm-container-toolkit
Version: %{rpmver}
Release: 1%{?dist}
Summary: AMD ROCm container toolkit for GPU access
License: Apache-2.0
URL: https://github.com/ROCm/container-toolkit

Source: container-toolkit-%{gover}.tar.gz
Source1: bundled-container-toolkit-%{gover}.tar.gz

BuildRequires: %{_cross_os}glibc-devel

%description
%{summary}.

%prep
%autosetup -Sgit -n %{gorepo}-%{gover} -p1
%setup -T -D -n %{gorepo}-%{gover} -b 1 -q

%build
%set_cross_go_flags

go build -ldflags="${GOLDFLAGS}" -o amd-container-runtime ./cmd/container-runtime
go build -ldflags="${GOLDFLAGS}" -o amd-ctk ./cmd/amd-ctk

%install
install -d %{buildroot}%{_cross_bindir}
install -p -m 0755 amd-container-runtime %{buildroot}%{_cross_bindir}
install -p -m 0755 amd-ctk %{buildroot}%{_cross_bindir}

%cross_scan_attribution go-vendor vendor

%files
%license LICENSE
%{_cross_attribution_file}
%{_cross_attribution_vendor_dir}
%{_cross_bindir}/amd-container-runtime
%{_cross_bindir}/amd-ctk
