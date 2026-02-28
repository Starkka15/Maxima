#!/bin/bash
# Lightweight qrc:// protocol handler for maxima-cli login flow.
# Replaces maxima-bootstrap by extracting the auth code from the
# qrc:// redirect URL and forwarding it to maxima-cli's local listener.
#
# Usage: maxima-qrc-handler.sh "qrc:///html/login_successful.html?code=XXXX"

URL="$1"

if [ -z "$URL" ]; then
    exit 1
fi

# Extract the code parameter from the URL
CODE=$(echo "$URL" | grep -oP 'code=\K[^&]+')

if [ -z "$CODE" ]; then
    exit 1
fi

# Forward the code to maxima-cli's local TCP listener
curl -s "http://127.0.0.1:31033/auth?code=${CODE}" >/dev/null 2>&1
