//! QUIC needs a TLS layer; here it is only the transport envelope. The peer is
//! not authenticated by this certificate, it is authenticated by the Noise
//! handshake inside the stream (see [`super::noise`]). So the server presents a
//! throwaway self-signed certificate and the client accepts any certificate.
//!
//! This is sound because the certificate is not what proves identity: a
//! man-in-the-middle who terminates the TLS layer still cannot complete the
//! Noise NNpsk0 handshake without the identity master, so it learns nothing and
//! can inject nothing. Pinning a real key here would add a second, redundant
//! authentication; the design keeps identity in exactly one place.

use std::sync::Arc;
use std::time::Duration;

use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

use super::noise::net;
use crate::error::Result;

const ALPN: &[u8] = b"horizon-constellation/0";

pub fn server_config() -> Result<quinn::ServerConfig> {
    let cert = rcgen::generate_simple_self_signed(vec!["horizon".to_string()]).map_err(net)?;
    let chain = vec![cert.cert.der().clone()];
    let key = PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der());

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut tls = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(net)?
        .with_no_client_auth()
        .with_single_cert(chain, key.into())
        .map_err(net)?;
    tls.alpn_protocols = vec![ALPN.to_vec()];

    let qsc = QuicServerConfig::try_from(tls).map_err(net)?;
    let mut cfg = quinn::ServerConfig::with_crypto(Arc::new(qsc));
    cfg.transport_config(transport());
    Ok(cfg)
}

pub fn client_config() -> Result<quinn::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut tls = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(net)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyServer(provider)))
        .with_no_client_auth();
    tls.alpn_protocols = vec![ALPN.to_vec()];

    let qcc = QuicClientConfig::try_from(tls).map_err(net)?;
    let mut cfg = quinn::ClientConfig::new(Arc::new(qcc));
    cfg.transport_config(transport());
    Ok(cfg)
}

// Shared transport tuning. A bounded idle timeout means a dead peer or a stalled
// handshake fails in seconds instead of hanging; the keep-alive (shorter than
// the timeout) holds an otherwise quiet link open mid-sync.
fn transport() -> Arc<quinn::TransportConfig> {
    let mut tc = quinn::TransportConfig::default();
    tc.max_idle_timeout(Some(Duration::from_secs(20).try_into().expect("20s idle")));
    tc.keep_alive_interval(Some(Duration::from_secs(8)));
    Arc::new(tc)
}

// Accept any server certificate. The Noise handshake, not the certificate, is
// what authenticates the peer as a holder of the identity. Signature checks
// still run through the crypto provider so a malformed handshake is rejected.
#[derive(Debug)]
struct AnyServer(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for AnyServer {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
