#!/bin/sh
set -eu

GROK_HOME=${GROK_HOME:-/var/lib/grok}
SOCK=${GROK_LEADER_SOCKET:-$GROK_HOME/leader.sock}
PKI=${AQUALUNG_PKI:-/run/aqualung/pki}
TOKEN=${TOPSIDE_TOKEN:-}

mkdir -p "$GROK_HOME" "$PKI"

if [ -z "$TOKEN" ]; then
  TOKEN=$(openssl rand -hex 16)
  echo "TOPSIDE_TOKEN=$TOKEN" >&2
fi
printf '%s\n' "$TOKEN" >"$PKI/token"

openssl genrsa -out "$PKI/ca.key" 2048 2>/dev/null
openssl req -new -x509 -key "$PKI/ca.key" -out "$PKI/ca.pem" -days 2 -subj "/CN=aqualung-verify-ca" 2>/dev/null
openssl genrsa -out "$PKI/server.key" 2048 2>/dev/null
openssl req -new -key "$PKI/server.key" -out "$PKI/server.csr" -subj "/CN=127.0.0.1" 2>/dev/null
printf '%s\n' "subjectAltName=IP:127.0.0.1" "extendedKeyUsage=serverAuth" >"$PKI/server.ext"
openssl x509 -req -in "$PKI/server.csr" -CA "$PKI/ca.pem" -CAkey "$PKI/ca.key" -CAcreateserial \
  -out "$PKI/server.pem" -days 2 -extfile "$PKI/server.ext" 2>/dev/null
openssl genrsa -out "$PKI/client.key" 2048 2>/dev/null
openssl req -new -key "$PKI/client.key" -out "$PKI/client.csr" -subj "/CN=snorkel-client" 2>/dev/null
printf '%s\n' "extendedKeyUsage=clientAuth" >"$PKI/client.ext"
openssl x509 -req -in "$PKI/client.csr" -CA "$PKI/ca.pem" -CAkey "$PKI/ca.key" -CAcreateserial \
  -out "$PKI/client.pem" -days 2 -extfile "$PKI/client.ext" 2>/dev/null

stop() {
  kill $TOPSIDE_PID $GROK_PID $SNORKEL_PID 2>/dev/null || true
  wait 2>/dev/null || true
}
trap stop TERM INT

topside \
  --cert "$PKI/server.pem" \
  --key "$PKI/server.key" \
  --ca "$PKI/ca.pem" \
  --client-cert "$PKI/client.pem" \
  --token "$TOKEN" \
  --snorkel 0.0.0.0:1943 \
  --phone 0.0.0.0:7678 &
TOPSIDE_PID=$!

grok agent leader \
  --no-exit-on-disconnect \
  --relay-on-demand \
  --no-auto-update \
  --leader-socket "$SOCK" &
GROK_PID=$!

i=0
while [ ! -S "$SOCK" ]; do
  i=$((i + 1))
  if [ "$i" -gt 100 ]; then
    echo "leader socket did not appear: $SOCK" >&2
    stop
    exit 1
  fi
  sleep 0.1
done

snorkel \
  --socket "$SOCK" \
  --server 127.0.0.1:1943 \
  --cert "$PKI/client.pem" \
  --key "$PKI/client.key" \
  --ca "$PKI/ca.pem" &
SNORKEL_PID=$!

wait
