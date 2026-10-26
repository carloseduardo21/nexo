FROM debian:trixie-slim
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      xrdp xorgxrdp dbus-x11 openbox xterm x11-xserver-utils xclip ssl-cert procps \
  && rm -rf /var/lib/apt/lists/*
RUN useradd -m -s /bin/bash nexo && echo 'nexo:nexo123' | chpasswd && adduser xrdp ssl-cert \
  && printf '#!/bin/sh\nxterm -geometry 100x30+40+40 &\nexec openbox\n' > /home/nexo/.xsession && chmod +x /home/nexo/.xsession && chown nexo /home/nexo/.xsession
EXPOSE 3389
CMD rm -f /var/run/xrdp/*.pid; /usr/sbin/xrdp-sesman && exec /usr/sbin/xrdp --nodaemon
