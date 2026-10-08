//! TLS for the engines that open their own socket (Redis): the same two behaviours as PostgreSQL's. "Require"
//! encrypts and accepts any certificate; "verify full" checks the chain against the operating system's trust store
//! and the name against the certificate.

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{ring as ring_provider, verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use rustls_platform_verifier::BuilderVerifierExt;

use super::{DbError, DbResult, TlsMode};

#[derive(Debug)]
struct AcceptAny(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAny {
    fn verify_server_cert(&self, _end: &CertificateDer<'_>, _inter: &[CertificateDer<'_>], _name: &ServerName<'_>, _ocsp: &[u8], _now: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// The client configuration for a mode, or `None` for "no TLS".
pub fn client_config(mode: TlsMode) -> DbResult<Option<Arc<ClientConfig>>> {
    if mode == TlsMode::Disable {
        return Ok(None);
    }
    let provider = Arc::new(ring_provider::default_provider());
    let tls_err = |e: rustls::Error| DbError::Invalid(format!("could not set up TLS: {e}"));
    let builder = ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions().map_err(tls_err)?;
    let config = match mode {
        TlsMode::Require => builder.dangerous().with_custom_certificate_verifier(Arc::new(AcceptAny(provider))).with_no_client_auth(),
        _ => builder.with_platform_verifier().map_err(tls_err)?.with_no_client_auth(),
    };
    Ok(Some(Arc::new(config)))
}
