#!/bin/bash

# Install AppArmor if not installed
if ! command -v apparmor_status &> /dev/null; then
    echo "AppArmor not found, installing..."
    apt update && apt install -y apparmor apparmor-utils
fi

# Ensure MediaHarbor AppArmor profile is in place
APPARMOR_PROFILE_PATH="/usr/share/apparmor/profiles/mediaharbor"
if [[ -f "resources/mediaharbor.apparmor" ]]; then
    echo "Copying MediaHarbor AppArmor profile..."
    cp resources/mediaharbor.apparmor "$APPARMOR_PROFILE_PATH"
    apparmor_parser -r "$APPARMOR_PROFILE_PATH"
fi

# Set permissions for chrome-sandbox
SANDBOX_PATH="/opt/MediaHarbor/chrome-sandbox"
if [[ -f "$SANDBOX_PATH" ]]; then
    echo "Setting permissions for chrome-sandbox..."
    chown root:root "$SANDBOX_PATH"
    chmod 4755 "$SANDBOX_PATH"
else
    echo "Warning: $SANDBOX_PATH not found, please ensure MediaHarbor is correctly installed."
fi

# Reload AppArmor profiles
echo "Reloading AppArmor profiles..."
service apparmor reload

echo "Setup complete. You should now be able to start MediaHarbor without sandbox issues."
