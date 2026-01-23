//! TLS/HTTPS Performance Benchmarks
//!
//! Targets: Match Actix-web performance (~200k+ HTTPS req/s)
//!
//! Run with: cargo bench --features tls
//!
//! For external benchmarking with wrk:
//! 1. Generate test certs: ./scripts/gen_test_certs.sh
//! 2. Run server: cargo run --release --features tls --example https_server
//! 3. Benchmark: wrk -t4 -c100 -d30s https://localhost:8443/

#![allow(unused)]

#[cfg(feature = "tls")]
mod tls_benchmarks {
    use std::hint::black_box;
    use std::time::{Duration, Instant};

    // Simple timing macro for benchmarks
    macro_rules! bench {
        ($name:expr, $iterations:expr, $code:block) => {{
            let start = Instant::now();
            for _ in 0..$iterations {
                black_box($code);
            }
            let elapsed = start.elapsed();
            let per_iter = elapsed / $iterations;
            println!(
                "{}: {} iterations in {:?} ({:?}/iter, {:.0} ops/sec)",
                $name,
                $iterations,
                elapsed,
                per_iter,
                $iterations as f64 / elapsed.as_secs_f64()
            );
            elapsed
        }};
    }

    /// Benchmark TLS configuration creation
    #[test]
    fn bench_tls_client_config_creation() {
        use script::runtime::r#async::tls::TlsClientConfig;

        println!("\n=== TLS Client Config Creation ===");
        bench!("TlsClientConfig::new()", 100, {
            let _ = TlsClientConfig::new().unwrap();
        });
    }

    /// Benchmark TLS server config creation (with cert loading)
    #[test]
    #[ignore] // Requires test certificates
    fn bench_tls_server_config_creation() {
        use script::runtime::r#async::tls::TlsServerConfig;
        use std::path::Path;

        let cert_path = Path::new("test_certs/cert.pem");
        let key_path = Path::new("test_certs/key.pem");

        if !cert_path.exists() {
            println!("Skipping: test certificates not found");
            println!("Run: ./scripts/gen_test_certs.sh");
            return;
        }

        println!("\n=== TLS Server Config Creation ===");
        bench!("TlsServerConfig::from_pem_files()", 100, {
            let _ = TlsServerConfig::from_pem_files(cert_path, key_path).unwrap();
        });
    }

    /// Benchmark TLS connector creation
    #[test]
    fn bench_tls_connector_creation() {
        use script::runtime::r#async::tls::TlsConnector;

        println!("\n=== TLS Connector Creation ===");
        bench!("TlsConnector::new()", 1000, {
            let _ = TlsConnector::new().unwrap();
        });
    }
}

/// Main benchmark runner
fn main() {
    #[cfg(not(feature = "tls"))]
    {
        eprintln!("Error: TLS feature not enabled");
        eprintln!("Run with: cargo bench --features tls");
        std::process::exit(1);
    }

    #[cfg(feature = "tls")]
    {
        println!("==============================================");
        println!("  TLS Performance Benchmarks");
        println!("  Target: 200k+ HTTPS req/s (Actix-level)");
        println!("==============================================\n");

        println!("For full HTTP benchmarking:");
        println!("1. Generate certs: ./scripts/gen_test_certs.sh");
        println!("2. Run server: cargo run --release --features tls --example https_server");
        println!("3. Benchmark: wrk -t4 -c100 -d30s https://localhost:8443/\n");

        // Run inline benchmarks
        use std::hint::black_box;
        use std::time::Instant;

        // Benchmark TlsClientConfig creation
        {
            use script::runtime::r#async::tls::TlsClientConfig;

            let iterations = 100;
            let start = Instant::now();
            for _ in 0..iterations {
                black_box(TlsClientConfig::new().unwrap());
            }
            let elapsed = start.elapsed();
            println!(
                "TlsClientConfig::new(): {:?}/iter ({:.0} ops/sec)",
                elapsed / iterations,
                iterations as f64 / elapsed.as_secs_f64()
            );
        }

        // Benchmark TlsConnector creation
        {
            use script::runtime::r#async::tls::TlsConnector;

            let iterations = 1000;
            let start = Instant::now();
            for _ in 0..iterations {
                black_box(TlsConnector::new().unwrap());
            }
            let elapsed = start.elapsed();
            println!(
                "TlsConnector::new(): {:?}/iter ({:.0} ops/sec)",
                elapsed / iterations,
                iterations as f64 / elapsed.as_secs_f64()
            );
        }

        println!("\nBenchmarks complete.");
    }
}
