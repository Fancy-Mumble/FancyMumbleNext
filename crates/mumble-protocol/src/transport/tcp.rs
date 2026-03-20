//! TLS-encrypted TCP transport for Mumble control messages.
//!
//! Mumble uses TLS 1.2+ for its TCP control channel. This module handles
//! connecting, framing, and sending/receiving [`ControlMessage`]s.

use std::sync::Arc;

use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;
use tracing::{debug, trace};

use crate::error::{Error, Result};
use crate::message::ControlMessage;
use crate::transport::codec;

/// Configuration for establishing a TCP connection to a Mumble server.
#[derive(Debug, Clone)]
pub struct TcpConfig {
    /// Hostname or IP address of the Mumble server.
    pub server_host: String,
    /// TCP port the server listens on (default 64738).
    pub server_port: u16,
    /// Accept invalid TLS certificates (self-signed). Defaults to `true`
    /// because most Mumble servers use self-signed certs.
    pub accept_invalid_certs: bool,
    /// Optional PEM-encoded client certificate chain for user registration.
    /// When present, these are sent during the TLS handshake.
    pub client_cert_pem: Option<Vec<u8>>,
    /// Optional PEM-encoded private key matching `client_cert_pem`.
    pub client_key_pem: Option<Vec<u8>>,
}

impl Default for TcpConfig {
    fn default() -> Self {
        Self {
            server_host: "localhost".into(),
            server_port: 64738,
            accept_invalid_certs: true,
            client_cert_pem: None,
            client_key_pem: None,
        }
    }
}

/// A connected TCP transport split into independent read/write halves.
pub struct TcpTransport {
    stream: TlsStream<TcpStream>,
    read_buf: BytesMut,
}

impl std::fmt::Debug for TcpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpTransport").finish_non_exhaustive()
    }
}

impl TcpTransport {
    /// Connect to a Mumble server over TLS.
    pub async fn connect(config: &TcpConfig) -> Result<Self> {
        let addr = format!("{}:{}", config.server_host, config.server_port);
        debug!(addr = %addr, "connecting TCP");

        let tcp_stream = TcpStream::connect(&addr).await?;

        let tls_config = build_tls_config(
            config.accept_invalid_certs,
            config.client_cert_pem.as_deref(),
            config.client_key_pem.as_deref(),
        )?;
        let connector = TlsConnector::from(Arc::new(tls_config));

        let server_name = match rustls::pki_types::ServerName::try_from(config.server_host.clone()) {
            Ok(name) => name,
            Err(_) => {
                let ip: std::net::IpAddr = config
                    .server_host
                    .parse()
                    .map_err(|e| Error::Other(format!("invalid server address '{}': {e}", config.server_host)))?;
                rustls::pki_types::ServerName::IpAddress(ip.into())
            }
        };

        let tls_stream = connector.connect(server_name, tcp_stream).await?;
        debug!("TLS handshake complete");

        Ok(Self {
            stream: tls_stream,
            read_buf: BytesMut::with_capacity(8192),
        })
    }

    /// Send a single control message.
    pub async fn send(&mut self, msg: &ControlMessage) -> Result<()> {
        let frame = codec::encode(msg)?;
        self.stream.write_all(&frame).await?;
        self.stream.flush().await?;
        trace!("sent TCP message");
        Ok(())
    }

    /// Receive the next control message (blocks until a full frame arrives).
    pub async fn recv(&mut self) -> Result<ControlMessage> {
        loop {
            if let Some(msg) = codec::decode(&mut self.read_buf)? {
                return Ok(msg);
            }

            let n = self.stream.read_buf(&mut self.read_buf).await?;
            if n == 0 {
                return Err(Error::ConnectionClosed);
            }
        }
    }

    /// Split into independent reader and writer, suitable for concurrent use
    /// in separate tasks.
    pub fn split(self) -> (TcpReader, TcpWriter) {
        let (read_half, write_half) = tokio::io::split(self.stream);
        (
            TcpReader {
                reader: read_half,
                read_buf: self.read_buf,
            },
            TcpWriter { writer: write_half },
        )
    }
}

/// Read half of a split TCP transport.
pub struct TcpReader {
    reader: tokio::io::ReadHalf<TlsStream<TcpStream>>,
    read_buf: BytesMut,
}

impl std::fmt::Debug for TcpReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpReader").finish_non_exhaustive()
    }
}

impl TcpReader {
    /// Receive the next control message from the server.
    pub async fn recv(&mut self) -> Result<ControlMessage> {
        loop {
            if let Some(msg) = codec::decode(&mut self.read_buf)? {
                return Ok(msg);
            }

            let n = self.reader.read_buf(&mut self.read_buf).await?;
            if n == 0 {
                return Err(Error::ConnectionClosed);
            }
        }
    }
}

/// Write half of a split TCP transport.
pub struct TcpWriter {
    writer: tokio::io::WriteHalf<TlsStream<TcpStream>>,
}

impl std::fmt::Debug for TcpWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpWriter").finish_non_exhaustive()
    }
}

impl TcpWriter {
    /// Send a single control message over the TCP transport.
    pub async fn send(&mut self, msg: &ControlMessage) -> Result<()> {
        let frame = codec::encode(msg)?;
        self.writer.write_all(&frame).await?;
        self.writer.flush().await?;
        trace!("sent TCP message");
        Ok(())
    }
}

// -- TLS configuration ---------------------------------------------

fn build_tls_config(
    accept_invalid_certs: bool,
    client_cert_pem: Option<&[u8]>,
    client_key_pem: Option<&[u8]>,
) -> Result<rustls::ClientConfig> {
    let builder = rustls::ClientConfig::builder();

    // Choose root certificate strategy.
    let builder_with_roots = if accept_invalid_certs {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(InsecureVerifier))
    } else {
        let root_store =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        builder.with_root_certificates(root_store)
    };

    // Attach client certificate if provided.
    let config = match (client_cert_pem, client_key_pem) {
        (Some(cert_pem), Some(key_pem)) => {
            let certs: Vec<_> = rustls_pemfile::certs(&mut &*cert_pem)
                .filter_map(std::result::Result::ok)
                .collect();
            let key = rustls_pemfile::private_key(&mut &*key_pem)
                .map_err(|e| Error::Other(format!("failed to parse client key PEM: {e}")))?
                .ok_or_else(|| Error::Other("no private key found in PEM data".into()))?;

            builder_with_roots
                .with_client_auth_cert(certs, key)
                .map_err(|e| Error::Other(format!("failed to set client auth cert: {e}")))?
        }
        _ => builder_with_roots.with_no_client_auth(),
    };

    Ok(config)
}

/// A TLS certificate verifier that accepts everything.
/// Required for self-signed Mumble server certificates.
#[derive(Debug)]
struct InsecureVerifier;

impl rustls::client::danger::ServerCertVerifier for InsecureVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
