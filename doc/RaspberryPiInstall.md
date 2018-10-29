# Exclave Tests in the Factory

Exclave's primary target is the Raspberry Pi.  These are cheap to buy, readily available, and don't require a license.  They're quick to bring up, so you can parallelize your factory line with `dd`.

## Hardware

With the current configuration, the following connections must be made from the Pi header:

````
+--------------------+
|55G---G-D-C---G-G---|
|----GRYg----G-----SG|
+--------------------+

G = GND
R = Red LED
Y = Yellow LED
g = Green LED
S = Switch
D = SWD
C = SWC
5 = 5V
````

| Pin | Target
| --- | ------
| 2   | Target Device +5V
| 6   | Target Device GND
| 9   | LEDs GND
| 11  | Red LED
| 13  | Yellow LED
| 15  | Green LED
| 18  | Target Device SWD
| 22  | Target Device SWC
| 37  | START button
| 39  | START button (GND)

## Tester Setup

In order to set up the tester, perform the following steps:

### Building Dependencies (host system, running Debian or similar)

```sh
mkdir tester
cd tester
sudo apt install gcc-arm-linux-gnueabihf
mkdir boot

# Create the new /boot/ directory and tell fedberry to launch in "headless" mode
touch boot/headless

# Inform exclave that this is a valid jig by creating a magic file.
# The exclave.jig unit file tests for the presence of this file, to
# determine what kind of jig it's running on.  Customize this to match
# your jig, or remove it altogether to disable jig-detection.
touch boot/exclave-jig

# Build openocd (if you use openocd)
git clone --recurse-submodules git://git.code.sf.net/p/openocd/code openocd
cd openocd
./bootstrap
./configure --prefix=/boot --host=arm-linux-gnueabihf --enable-bcm2835gpio --enable-sysfsgpio --disable-jlink
make install DESTDIR=$(pwd)/../
cd ..

# Build exclave
curl https://sh.rustup.rs -sSf | sh
export PATH="$PATH:~/.cargo/bin"
rustup target install armv7-unknown-linux-gnueabihf
git clone https://github.com/exclave/exclave.git
cd exclave
cargo build --target=armv7-unknown-linux-gnueabihf --release
cp target/armv7-unknown-linux-gnueabihf/release/exclave ../boot/bin/
cd ..

# Check out the tests (in this case, the Tomu tests)
git clone https://github.com/exclave/tomu-tests.git boot/exclave-tests
```

### Installing FedBerry

1. Download it from http://download.fedberry.org/releases/
1. Write it to an SD card (using dd or win32diskimager)
1. Copy the contents of `boot/` from above onto the root of partition #2 (the FAT partition)
1. Insert the SD card into the Raspberry Pi and boot it
1. Connect to the Raspberry Pi via ssh.  Username is `root` and password is `fedberry`
1. Log back in after setting the password
1. Run the following script to set everything up:

```sh
echo "kernel.sysrq=0" >> /etc/sysrq.conf
echo 'dwc_otg.lpm_enable=0 root=/dev/mmcblk0p2 quiet splash loglevel=0 logo.nologo vt.global_cursor_default=0 ro rootfstype=ext4 elevator=deadline fsck.repair=yes rootwait libahci.ignore_sss=1 raid=noautodetect nortc selinux=0 audit=0 quiet' > /boot/cmdline.txt
systemctl mask serial-getty@ttyAMA0.service
echo exclave > /etc/hostname
fedberry-config --dto-enable pi3-disable-wifi --dto-enable pi3-disable-bt --vc4-disable --dto-enable watchdog
dnf install -y dfu-util rsync # if necessary, install other packages here
cat > /etc/systemd/system/exclave.service <<EOF
[Unit]
Description=Launcher for Exclave

[Service]
Type=simple
ExecStart=/boot/bin/exclave -c /boot/tomu-tests
User=root
WorkingDirectory=/boot/tomu-tests

[Install]
WantedBy=getty.target
EOF
cat > /etc/fstab <<EOF
/dev/mmcblk0p2  /       ext4    defaults,noatime,ro         0 0
/dev/mmcblk0p1  /boot   vfat    defaults,noatime,ro         0 0
tmpfs           /tmp    tmpfs   defaults,noatime,size=100m  0 0
EOF
systemctl enable exclave.service
reboot
```

Note that the filesystem is much bigger than it needs to be.  You can copy it to a smaller disk by connecting a replacement SD card via USB and running the following:
```sh
dd if=/dev/zero of=/dev/sda bs=1M count=1
fdisk /dev/sda <<EOF
n
p
1

+200M
t
c
n
p
2


a
1
w
EOF
mkfs.vfat -F 16 /dev/sda1
mkfs.ext4 -F /dev/sda2
mount /dev/sda1 /mnt
rsync -avxHAX --progress /boot/ /mnt
umount /mnt
mount /dev/sda2 /mnt
rsync -avxHAX --progress / /mnt
umount /mnt
```

## Previous Configuration

Previously, the recommendation was to build exclave on the Raspberry Pi itself.  This is no longer recommended, as it takes too long.  Plus, Rust has gotten to the point where it's relatively easy to cross-compile.

The previous instructions for on-device development were:

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

A pre-built base image executed according to the above instructions can be downloaded at: https://bunniefoo.com/exclave/exclave-initial-image-oct-13-2018.img.gz. It starts with the fedberry-minimal image version 28 (20180725) and contains all the updates available as of October 13 2018. The version of openocd built and installed is based on the migen fork, which includes FPGA and SPI over JTAG programming.
