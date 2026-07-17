FROM linuxdeepin/apricot:v20.8-compatible@sha256:be6ee56f055c4d3e3b1a77badaf7b42b3d0e70337f3ea3d203304d169ccefb78

ARG DEB_FILENAME

RUN DEBIAN_FRONTEND=noninteractive apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      ca-certificates \
      dbus-x11 \
      imagemagick \
      procps \
      python3-minimal \
      x11-apps \
      x11-utils \
      xauth \
      xdg-utils \
      xvfb \
    && rm -rf /var/lib/apt/lists/*

COPY ${DEB_FILENAME} /tmp/drpa-next-uos20.deb
COPY uos20_container_smoke.sh /usr/local/bin/uos20_container_smoke.sh

RUN chmod 0755 /usr/local/bin/uos20_container_smoke.sh \
    && DEBIAN_FRONTEND=noninteractive apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y /tmp/drpa-next-uos20.deb \
    && rm -rf /var/lib/apt/lists/*

ENV DRPA_ALLOW_SYSTEM_MESA_DRI=1
CMD ["/usr/local/bin/uos20_container_smoke.sh"]
