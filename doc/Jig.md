Setting up a Jig
================

1. Install Fedberry.  Use the Minimal image from http://download.fedberry.org/releases and write it to an SD card.
1. Create an empty file "headless.txt" on the boot partition, to enable "headless" mode.
1. Log in via SSH.  The username is "root" and the password is "fedberry".
1. Disable sysrq: `echo "kernel.sysrq=0" >> /etc/sysrq.conf`
1. Disable systemd on the console: `systemctl mask serial-getty@ttyAMA0.service`
1. Disable the firewall: `systemctl disable firewalld`
1. Disable Bluetooth and resize the rootfs (this resets the board): `fedberry-config --rootfs-grow --bt-disable`
1. Enable "fastestmirror": `echo 'fastestmirror=true' >> /etc/dnf/dnf.conf`
1. Upgrade everything: `dnf upgrade`
1. Install rustup.  `curl https://sh.rustup.rs -sSf | sudo sh`
1. Install development tools: `dnf groupinstall "Development Tools" "Development Libraries"`
1. Install avahi: `dnf install avahi-dnsconfd.armv7hl avahi-autoipd.armv7hl avahi-tools.armv7hl avahi.armv7hl; systemctl enable avahi-daemon`
1. Set the hostname: Edit /etc/hostname
1. Create a user: `adduser pi; passwd pi; vigr # add pi to 'wheel'; vigr -g # add pi to 'wheel'; visudo # set NOPASSWD for 'wheel'`
1. Install libpng-dev, required for QR code support: `dnf install libpng-devel`
1. Install other support libraries: `dnf install libtool which vim emacs gdb net-tools screen nmap-ncat`
1. Clone openocd: `mkdir /opt/openocd; mkdir code; cd code; git clone --recursive git://git.code.sf.net/p/openocd/code openocd; cd openocd; ./bootstrap; ./configure --enable-bcm2835gpio --enable-sysfsgpio --disable-werror --prefix=/opt/openocd; make; make install`
