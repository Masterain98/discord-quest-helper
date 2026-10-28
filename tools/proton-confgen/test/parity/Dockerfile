# The reference environment for the wire-parity goldens: Ubuntu 24.04 with
# Proton's own packages from their apt repository, i.e. the official Linux
# client's real HTTP stack (python-proton-core on the distro's aiohttp/OpenSSL).
FROM ubuntu:24.04
ARG RELEASE_DEB=protonvpn-stable-release_1.0.8_all.deb
RUN apt-get update -qq \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq wget ca-certificates gnupg >/dev/null \
 && wget -q "https://repo.protonvpn.com/debian/dists/stable/main/binary-all/${RELEASE_DEB}" -O /tmp/release.deb \
 && dpkg -i /tmp/release.deb \
 && apt-get update -qq \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq python3-proton-vpn-api-core tzdata >/dev/null \
 && ln -sf /usr/share/zoneinfo/Europe/Zurich /etc/localtime \
 && rm -rf /var/lib/apt/lists/* /tmp/release.deb
WORKDIR /parity
