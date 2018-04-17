# Setup Instructions for Raspberry Pi

These instructions are useful for using Exclave on a Raspberry Pi.  Currently, we recommend using Fedberry.

## Installing

1. Download fedberry-minimal from https://github.com/fedberry/fedberry/releases
1. Write fedberry-minimal.raw to an SD card
1. Create a file on the "Boot" partition called "headless"
1. Insert the SD card into the Raspberry Pi and power it on
1. Determine the IP address of the Pi
1. SSH to it and change the password
1. Update the hostname: `hostnamectl set-hostname exclave`
1. Update the base system: `dnf upgrade -y`
1. Install avahi: `dnf install -y avahi avahi-autoipd avahi-compat-libdns_sd avahi-glib avahi-gobject avahi-tools nss-mdns`
1. Install Exclave dependencies: `dnf install -y rust cargo git`
1. Build and install Exclave: `cargo install --git https://github.com/exclave/exclave.git --root /usr/local`
1. Build and install OpenOCD: `dnf install which libtool && git clone git://repo.or.cz/openocd.git && cd openocd && ./bootstrap && ./configure --prefix=/usr/local --enable-bcm2835gpio && make && make install`

## Systemd Unit File

````[Unit]
Description=Launcher for Exclave

[Service]
Type=simple
ExecStart=/usr/local/bin/exclave -c /root/tests
User=root
WorkingDirectory=/root/tests

[Install]
WantedBy=getty.target````

## Setup using Windows

The program "Win32DiskImager" can be used to burn the SD image.  This program is available from Redhat, and from a variety of other sources such as Chocolatey.

You can use "Internet Connection Sharing" and a USB Ethernet device to connect directly to the Raspberry Pi.  They will auto-negotiate crossover, so you can plug a Pi directly into your PC.  It is possible to give the Pi Internet access by sharing your connection.

Click the `Start` button and type `Change Ethernet settings`, then in the window that pops up click on `Change adapter options`.  Right click on your Internet connection (likely called `Wi-Fi` or something similar) and go to `Properties`.  Click on the `Sharing` tab, check `Allow other network users to connect through this computer's Internet connection` and set your USB Ethernet device to the `Home networking connection`.  Finally, click `OK`.

To determine the IP address assigned to the Pi, open a command prompt and run `arp -a`.  It is the address under `192.168.137.1`.  For example:

````Interface: 192.168.137.1 --- 0x7
  Internet Address      Physical Address      Type
  192.168.137.130       b8-27-eb-d7-25-4c     static
  224.0.0.22            01-00-5e-00-00-16     static````

In the above output, the Raspberry Pi is at address 192.168.137.130.