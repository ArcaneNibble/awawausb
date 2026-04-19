#!/bin/bash

set -u
cd "$(dirname "$0")"

echo "Welcome to the awawausb native stub installer!"

os="$(uname -s)"

if [ "$os" = Darwin ]; then
    echo "Detected macOS"
    binary="awawausb-native-stub-mac"
    manifest_loc="$HOME/Library/Application Support/Mozilla/NativeMessagingHosts"
elif [ "$os" = Linux ]; then
    echo "Detected Linux"
    cpu="$(uname -m)"
    binary="awawausb-native-stub-linux-$cpu"
    manifest_loc="$HOME/.mozilla/native-messaging-hosts"
else
    echo "Unsupported operating system $os, sorry :("
    exit 1
fi

if ! [ -f "$binary" ]; then
    echo "I don't have a $binary binary, sorry :("
    echo "(Did you unzip all the files?)"
    exit 1
fi

read -r -p "Where should I install? [~/.local/bin]: " install_loc
install_loc=${install_loc:-"$HOME/.local/bin"}

set -e

# Copy the binary
mkdir -p "$install_loc"
cp "$binary" "$install_loc/awawausb-native-stub"

# If we're on a Mac, get rid of this
if [ "$os" = Darwin ]; then
    xattr -d com.apple.quarantine "$install_loc/awawausb-native-stub" || true
fi

# Register the JSON metadata
mkdir -p "$manifest_loc"
cat <<EOF >"$manifest_loc/awawausb_native_stub.json"
{
  "name": "awawausb_native_stub",
  "description": "Allows WebUSB extension to access USB devices",
  "path": "$install_loc/awawausb-native-stub",
  "type": "stdio",
  "allowed_extensions": ["awawausb@arcanenibble.com"]
}
EOF

echo "Installation complete!"
