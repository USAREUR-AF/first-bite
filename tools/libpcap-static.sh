# shellcheck shell=bash
# Sourced by build-portable.sh and build-static.sh; not run on its own.
#
#   build_static_libpcap <work directory>
#
# Builds libpcap from its checksummed source tarball as a static library with
# only the Linux packet capture module (no D-Bus, netlink, RDMA, Bluetooth or
# USB support, which are what make a distribution's libpcap pull in other
# shared libraries), and exports LIBPCAP_LIBDIR and LIBPCAP_VER, which the
# pcap crate's build script reads. A library already built in the work
# directory is reused.
#
# Needs: a C compiler, make, flex, bison, curl, xz, the kernel headers.

# 1.10.7 fixes CVE-2026-0799 and CVE-2026-6554 in the BPF interpreter and,
# from 1.10.6, CVE-2025-11961 in pcap_ether_aton().
LIBPCAP_VERSION=1.10.7
LIBPCAP_SHA256=68fa62cffb974f4275641ce14c2e2d75739251f30e00e6a0900903b247d76a03

build_static_libpcap() {
  local work="$1"
  local prefix="$work/libpcap"
  local tarball="$work/libpcap-$LIBPCAP_VERSION.tar.xz"
  mkdir -p "$work"
  if [ ! -f "$prefix/lib/libpcap.a" ]; then
    if [ ! -f "$tarball" ]; then
      curl -fsSL -o "$tarball" "https://www.tcpdump.org/release/libpcap-$LIBPCAP_VERSION.tar.xz"
    fi
    echo "$LIBPCAP_SHA256  $tarball" | sha256sum -c - >/dev/null
    rm -rf "$work/libpcap-$LIBPCAP_VERSION"
    tar -xJf "$tarball" -C "$work"
    (
      cd "$work/libpcap-$LIBPCAP_VERSION" || exit 1
      ./configure --quiet --prefix="$prefix" --disable-shared \
        --disable-dbus --disable-rdma --disable-bluetooth --disable-usb \
        --disable-netmap --without-dpdk --without-libnl
      make --quiet -j "${CARGO_BUILD_JOBS:-$(nproc)}"
      make --quiet install
    )
  fi
  # Only the static archive may be in the directory the linker searches.
  rm -f "$prefix"/lib/libpcap.so*
  export LIBPCAP_LIBDIR="$prefix/lib"
  export LIBPCAP_VER="$LIBPCAP_VERSION"
}
