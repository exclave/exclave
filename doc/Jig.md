Setting up a Jig
================

1. Install Fedberry.  Use the Minimal image from http://download.fedberry.org/releases and write it to an SD card.
2. Create a file "headless.txt" on the boot partition, to enable "headless" mode.
3. Log in via SSH.  The username is "root" and the password is "fedberry".
4. Disable sysrq by writing "kernel.sysrq=0" in /etc/sysrq.conf
5. Disable systemd on the console: systemctl mask serial-getty@ttyAMA0.service
6. Disable the firewall: systemctl disable firewalld
6. Disable Bluetooth and resize the rootfs: fedberry-config --rootfs-grow --bt-disable
7. Add "fastestmirror=true" to /etc/dnf/dnf.conf
8. Upgrade everything: "dnf upgrade"
9. Install rustup.  curl https://sh.rustup.rs -sSf | sudo sh
10. Install development tools: groupinstall "Development Tools" "Development Libraries"
11. Install avahi: dnf install avahi-dnsconfd.armv7hl avahi-autoipd.armv7hl avahi-tools.armv7hl avahi.armv7hl; systemctl enable avahi-daemon
12. Set the hostname: Edit /etc/hostname
13. Create a user: adduser pi; passwd pi; vigr # add pi to 'wheel'; vigr -g # add pi to 'wheel'; visudo # set NOPASSWD for 'wheel'
14. Install libpng-dev, required for QR code support: sudo dnf install libpng-devel
15. Install other support libraries: dnf install libtool which vim emacs gdb net-tools screen nmap-ncat
15. Clone openocd: mkdir code; cd code; git clone --recursive git://git.code.sf.net/p/openocd/code openocd; cd openocd; ./bootstrap; ./configure --enable-bcm2835gpio --enable-sysfsgpio --disable-werror; make; make install