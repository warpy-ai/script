#!/bin/bash
# HTTPS Performance Benchmark Script
#
# Compares Script HTTPS server against Actix-web benchmark targets
#
# Usage: ./scripts/run_https_bench.sh [duration_seconds]
#
# Prerequisites:
#   - wrk (brew install wrk)
#   - Test certificates (./scripts/gen_test_certs.sh)
#   - HTTPS server running (cargo run --release --features tls --example https_server)

set -e

DURATION="${1:-30}"
THREADS=4
CONNECTIONS=100
URL="https://localhost:8443/"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "=============================================="
echo "  HTTPS Performance Benchmark"
echo "=============================================="
echo
echo "Configuration:"
echo "  Duration:    ${DURATION}s"
echo "  Threads:     $THREADS"
echo "  Connections: $CONNECTIONS"
echo "  URL:         $URL"
echo
echo "Targets (Actix-web level):"
echo "  - Requests/sec: 200,000+"
echo "  - Latency avg:  <1ms"
echo "  - Latency p99:  <5ms"
echo

# Check prerequisites
check_prereqs() {
    local missing=0

    if ! command -v wrk &> /dev/null; then
        echo -e "${RED}Error: wrk is not installed${NC}"
        echo "Install with: brew install wrk (macOS)"
        missing=1
    fi

    if ! [ -f "test_certs/cert.pem" ]; then
        echo -e "${RED}Error: Test certificates not found${NC}"
        echo "Generate with: ./scripts/gen_test_certs.sh"
        missing=1
    fi

    # Check if server is running
    if ! curl -sk --connect-timeout 2 "$URL" > /dev/null 2>&1; then
        echo -e "${RED}Error: HTTPS server not responding at $URL${NC}"
        echo "Start with: cargo run --release --features tls --example https_server"
        missing=1
    fi

    if [ $missing -eq 1 ]; then
        exit 1
    fi

    echo -e "${GREEN}Prerequisites OK${NC}"
    echo
}

run_benchmark() {
    echo "Running benchmark..."
    echo "-------------------------------------------"
    echo

    # Run wrk benchmark
    wrk -t$THREADS -c$CONNECTIONS -d${DURATION}s "$URL" 2>&1 | tee /tmp/wrk_output.txt

    echo
    echo "-------------------------------------------"
    echo
}

analyze_results() {
    local output="/tmp/wrk_output.txt"

    # Parse results
    local rps=$(grep "Requests/sec" "$output" | awk '{print $2}' | tr -d ',')
    local latency_avg=$(grep "Latency" "$output" | head -1 | awk '{print $2}')
    local transfer=$(grep "Transfer/sec" "$output" | awk '{print $2}')

    echo "Results Summary:"
    echo "-------------------------------------------"

    # Check requests/sec
    if [ -n "$rps" ]; then
        local rps_int=${rps%.*}
        if [ "$rps_int" -ge 200000 ]; then
            echo -e "Requests/sec: ${GREEN}$rps${NC} (TARGET MET: 200k+)"
        elif [ "$rps_int" -ge 150000 ]; then
            echo -e "Requests/sec: ${YELLOW}$rps${NC} (Close to target)"
        else
            echo -e "Requests/sec: ${RED}$rps${NC} (Below target: 200k)"
        fi
    fi

    # Display other metrics
    echo "Avg Latency:  $latency_avg"
    echo "Transfer/sec: $transfer"
    echo

    # Session resumption check
    echo "TLS Performance Tips:"
    echo "  - Ensure session tickets are enabled"
    echo "  - Use EC certificates (not RSA) for faster handshakes"
    echo "  - aws-lc-rs provides ~20% crypto speedup"
    echo
}

compare_actix() {
    echo "Comparison with Actix-web Benchmarks:"
    echo "-------------------------------------------"
    echo "Metric         | Script   | Actix-web | Status"
    echo "-------------------------------------------"
    echo "Req/s (target) | 200k+    | ~200k     | -"
    echo "Latency avg    | <1ms     | ~0.5ms    | -"
    echo "Memory/conn    | <10KB    | ~8KB      | -"
    echo "-------------------------------------------"
    echo
    echo "Note: Actual Actix benchmarks vary by hardware."
    echo "Run Actix comparison: ./scripts/bench_vs_actix.sh"
}

# Main
check_prereqs
run_benchmark
analyze_results
compare_actix

echo
echo "Benchmark complete!"
