#!/bin/bash
# Generate test certificates for HTTPS benchmarking
#
# Usage: ./scripts/gen_test_certs.sh

set -e

CERT_DIR="test_certs"
CERT_FILE="$CERT_DIR/cert.pem"
KEY_FILE="$CERT_DIR/key.pem"

echo "==================================="
echo "  TLS Test Certificate Generator"
echo "==================================="
echo

# Create directory
mkdir -p "$CERT_DIR"

# Check if openssl is available
if ! command -v openssl &> /dev/null; then
    echo "Error: openssl is required but not installed."
    echo "Install with: brew install openssl (macOS) or apt install openssl (Linux)"
    exit 1
fi

# Generate EC key (faster than RSA for handshakes)
echo "Generating EC private key (prime256v1)..."
openssl ecparam -genkey -name prime256v1 -out "$KEY_FILE" 2>/dev/null

# Generate self-signed certificate
echo "Generating self-signed certificate..."
openssl req -new -x509 \
    -key "$KEY_FILE" \
    -out "$CERT_FILE" \
    -days 365 \
    -subj "/CN=localhost/O=Script Benchmark/C=US" \
    -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" \
    2>/dev/null

echo
echo "Certificates generated successfully!"
echo "  Certificate: $CERT_FILE"
echo "  Private key: $KEY_FILE"
echo
echo "Certificate details:"
openssl x509 -in "$CERT_FILE" -noout -subject -dates
echo
echo "Next steps:"
echo "  1. Run HTTPS server: cargo run --release --features tls --example https_server"
echo "  2. Benchmark with wrk: ./scripts/run_https_bench.sh"
echo "  3. Or manually: wrk -t4 -c100 -d30s https://localhost:8443/"
