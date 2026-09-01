FROM debian:10-slim

ARG DEB_FILENAME

RUN printf '%s\n' \
      'deb http://archive.debian.org/debian buster main' \
      'deb http://archive.debian.org/debian-security buster/updates main' \
      > /etc/apt/sources.list \
    && apt-get -o Acquire::Check-Valid-Until=false update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      ca-certificates \
      dbus-x11 \
      fontconfig \
      fonts-noto-cjk \
      imagemagick \
      procps \
      python3-minimal \
      x11-apps \
      x11-utils \
      xauth \
      xdg-utils \
      xdotool \
      xvfb \
    && rm -rf /var/lib/apt/lists/*

COPY ${DEB_FILENAME} /tmp/drpa-next-uos20.deb
COPY uos20_container_smoke.sh /usr/local/bin/uos20_container_smoke.sh

RUN chmod 0755 /usr/local/bin/uos20_container_smoke.sh \
    && apt-get -o Acquire::Check-Valid-Until=false update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y /tmp/drpa-next-uos20.deb \
    && rm -rf /var/lib/apt/lists/*

CMD ["/usr/local/bin/uos20_container_smoke.sh"]
