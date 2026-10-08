#!/usr/bin/env bash
# Installs the local test site into the host nginx (needs sudo):
# self-signed cert, /etc/hosts entries, site + snippets, then reload.
set -euo pipefail
cd "$(dirname "$0")"

DOMAINS=(dropofculture.test admin.dropofculture.test pgadmin.dropofculture.test)

if [ ! -f /etc/nginx/ssl/dropofculture.test.crt ]; then
    sudo mkdir -p /etc/nginx/ssl
    sudo openssl req -x509 -nodes -newkey rsa:2048 -days 365 \
        -keyout /etc/nginx/ssl/dropofculture.test.key \
        -out /etc/nginx/ssl/dropofculture.test.crt \
        -subj "/CN=dropofculture.test" \
        -addext "subjectAltName=$(printf 'DNS:%s,' "${DOMAINS[@]}" | sed 's/,$//')"
fi

if ! grep -q 'dropofculture.test' /etc/hosts; then
    echo "127.0.0.1 ${DOMAINS[*]}" | sudo tee -a /etc/hosts
fi

sudo cp snippets/oauth2-proxy.conf snippets/oauth2-protect.conf /etc/nginx/snippets/
sudo cp dropofculture.conf /etc/nginx/sites-available/dropofculture.conf
sudo ln -sf /etc/nginx/sites-available/dropofculture.conf /etc/nginx/sites-enabled/dropofculture.conf

sudo nginx -t
sudo systemctl reload nginx
echo "ok: https://dropofculture.test, https://admin.dropofculture.test, https://pgadmin.dropofculture.test"
