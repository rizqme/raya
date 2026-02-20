//! TLS configuration and utilities shared across fetch, http, and net modules.
//!
//! Uses rustls (pure Rust TLS) with Mozilla CA certificates via webpki-roots.

use rustls::pki_types::ServerName;
use std::io::{self, Read, Write};
use std::net;
use std::sync::{Arc, OnceLock};

/// Cached default client config with Mozilla root certificates.
static DEFAULT_CLIENT_CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();

/// Get or create the default TLS client config with system/Mozilla root certificates.
pub fn default_client_config() -> Arc<rustls::ClientConfig> {
    DEFAULT_CLIENT_CONFIG
        .get_or_init(|| {
            let mut root_store = rustls::RootCertStore::empty();
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            Arc::new(
                rustls::ClientConfig::builder()
                    .with_root_certificates(root_store)
                    .with_no_client_auth(),
            )
        })
        .clone()
}

/// Create a TLS client config with custom CA certificates (PEM-encoded).
pub fn client_config_with_ca(ca_pem: &str) -> Result<Arc<rustls::ClientConfig>, String> {
    let mut root_store = rustls::RootCertStore::empty();
    // Start with Mozilla roots
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    // Add custom CA certs
    let mut cursor = io::Cursor::new(ca_pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut cursor)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to parse CA PEM: {}", e))?;
    for cert in certs {
        root_store
            .add(cert)
            .map_err(|e| format!("Failed to add CA cert: {}", e))?;
    }
    Ok(Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth(),
    ))
}

/// Create a TLS server config from PEM certificate chain and private key.
pub fn server_config(cert_pem: &str, key_pem: &str) -> Result<Arc<rustls::ServerConfig>, String> {
    let mut cert_cursor = io::Cursor::new(cert_pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut cert_cursor)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to parse certificate PEM: {}", e))?;
    if certs.is_empty() {
        return Err("No certificates found in PEM".to_string());
    }

    let mut key_cursor = io::Cursor::new(key_pem.as_bytes());
    let key = rustls_pemfile::private_key(&mut key_cursor)
        .map_err(|e| format!("Failed to parse private key PEM: {}", e))?
        .ok_or_else(|| "No private key found in PEM".to_string())?;

    Arc::new(
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| format!("TLS server config error: {}", e))?,
    )
    .pipe(Ok)
}

/// Establish a TLS connection to host:port over an existing TCP stream.
pub fn connect_tls(
    stream: net::TcpStream,
    host: &str,
    config: Arc<rustls::ClientConfig>,
) -> Result<rustls::StreamOwned<rustls::ClientConnection, net::TcpStream>, String> {
    let server_name = ServerName::try_from(host.to_string())
        .map_err(|e| format!("Invalid server name '{}': {}", host, e))?;
    let conn = rustls::ClientConnection::new(config, server_name)
        .map_err(|e| format!("TLS handshake setup failed: {}", e))?;
    let mut tls_stream = rustls::StreamOwned::new(conn, stream);
    // Force the handshake by writing zero bytes (flush triggers handshake)
    tls_stream.flush().map_err(|e| format!("TLS handshake failed: {}", e))?;
    Ok(tls_stream)
}

/// Accept a TLS connection on a server stream.
pub fn accept_tls(
    stream: net::TcpStream,
    config: Arc<rustls::ServerConfig>,
) -> Result<rustls::StreamOwned<rustls::ServerConnection, net::TcpStream>, String> {
    let conn = rustls::ServerConnection::new(config)
        .map_err(|e| format!("TLS server connection setup failed: {}", e))?;
    let mut tls_stream = rustls::StreamOwned::new(conn, stream);
    // Read a byte and unread it to trigger handshake; or just peek
    // Actually, the handshake happens on first read/write, so we trigger it:
    let mut buf = [0u8; 0];
    let _ = tls_stream.read(&mut buf);
    Ok(tls_stream)
}

/// Type alias for a client TLS stream.
pub type ClientTlsStream = rustls::StreamOwned<rustls::ClientConnection, net::TcpStream>;

/// Type alias for a server TLS stream.
pub type ServerTlsStream = rustls::StreamOwned<rustls::ServerConnection, net::TcpStream>;

// Helper trait for pipe syntax
trait Pipe: Sized {
    fn pipe<F, R>(self, f: F) -> R
    where
        F: FnOnce(Self) -> R,
    {
        f(self)
    }
}
impl<T> Pipe for T {}
