# Fake EntryNav2 head unit: BusyBox shell, root SSH login with a password,
# and stubs for what a container cannot do (remount /, reboot, ARM loader).
FROM alpine:3.20

RUN apk add --no-cache openssh-server \
    && ssh-keygen -A \
    && echo 'root:sim' | chpasswd \
    && printf '%s\n' \
        'PermitRootLogin yes' \
        'PasswordAuthentication yes' \
        'SetEnv PATH=/sim/bin:/usr/sbin:/usr/bin:/sbin:/bin' \
        >> /etc/ssh/sshd_config \
    && echo 'EntryNav2_SIM' > /etc/version

COPY bin/ /sim/bin/
COPY ld-linux-armhf.so.3 /lib/ld-linux-armhf.so.3
COPY fake-aa-supervisor /sim/fake/aa-supervisor
RUN chmod 755 /sim/bin/* /sim/fake/* /lib/ld-linux-armhf.so.3 && mkdir -p /sim/log

EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]
