#!/bin/bash

# Create plugdev system group
echo "Creating plugdev system group"
sudo groupadd -r plugdev

# Add current user to plugdev group
echo "Adding $USER to the plugdev group"
sudo usermod -aG plugdev $USER

# Create udev rule for Carlinkit dongle
echo "Creating udev rule"
UDEV_FILE="/etc/udev/rules.d/52-carplay.rules"
sudo tee "$UDEV_FILE" > /dev/null <<EOF
SUBSYSTEM=="usb", ATTR{idVendor}=="1314", ATTR{idProduct}=="152*", MODE="0660", GROUP="plugdev"
EOF

# Reload udev rules
echo "Reloading udev rules"
sudo udevadm control --reload-rules
sudo udevadm trigger
echo "Reboot for changes to take effect."
