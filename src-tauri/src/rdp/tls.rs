//! The TLS step of an RDP connection.
//!
//! RDP servers mostly have self-signed certificates, so no certificate authority is consulted: the caller pins the
//! certificate by fingerprint afterwards (see `check_cert` in the parent module). A pin only means something if the
//! server *owns* the certificate it shows, though. A certificate is public, so anyone on the network path can present
//! the real server's certificate; what they cannot do is sign the handshake with its private key. This module checks
//! that signature. `ironrdp-tls` 0.2 does not (its verifier answers "valid" to everything), which is why the upgrade
//! lives here and the dependency's `upgrade` is not used.

use std::io;
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt as _};
use tokio_rustls::rustls;
use tokio_rustls::rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::rustls::{DigitallySignedStruct, SignatureScheme};

pub type TlsStream<S> = tokio_rustls::client::TlsStream<S>;

/// Accepts any certificate (the caller pins it) but only from a server that proves it holds that certificate's key.
#[derive(Debug)]
struct ProvesPossession {
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for ProvesPossession {
    fn verify_server_cert(&self, _end_entity: &CertificateDer<'_>, _intermediates: &[CertificateDer<'_>], _server_name: &ServerName<'_>, _ocsp: &[u8], _now: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        // Who the certificate belongs to is decided by the pin, after the handshake.
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// Run the TLS handshake over `stream` and return the secured stream with the certificate the server presented.
/// The handshake fails unless the server holds the private key of that certificate.
pub async fn upgrade<S>(stream: S, server_name: &str) -> io::Result<(TlsStream<S>, x509_cert::Certificate)>
where
    S: Unpin + AsyncRead + AsyncWrite,
{
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(ProvesPossession { provider }))
        .with_no_client_auth();
    // CredSSP does not support TLS session resumption.
    config.resumption = rustls::client::Resumption::disabled();

    let domain = ServerName::try_from(server_name.to_owned()).map_err(io::Error::other)?;
    let mut tls = tokio_rustls::TlsConnector::from(Arc::new(config)).connect(domain, stream).await?;
    tls.flush().await?;

    let cert = {
        use x509_cert::der::Decode as _;
        let der = tls.get_ref().1.peer_certificates().and_then(|c| c.first()).ok_or_else(|| io::Error::other("peer certificate is missing"))?;
        x509_cert::Certificate::from_der(der).map_err(io::Error::other)?
    };
    Ok((tls, cert))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use rustls::pki_types::PrivateKeyDer;
    use rustls::server::{ClientHello, ResolvesServerCert};
    use rustls::sign::CertifiedKey;

    // Throwaway P-256 material made for these tests: a self-signed certificate for `CN=rdp.test` with its key (A),
    // and an unrelated key (B). Nothing else uses them.
    const CERT_A: &str = "MIIBfDCCASOgAwIBAgIUQ4ci5lcEM7rELWbD6ptPx2RaAhwwCgYIKoZIzj0EAwIwEzERMA8GA1UEAwwIcmRwLnRlc3QwIBcNMjYxMTAyMTQ0ODAwWhgPMjEyNjEwMDkxNDQ4MDBaMBMxETAPBgNVBAMMCHJkcC50ZXN0MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE4MxTLFpYPCOPAJgQpK7CP5FUZUMESsfo3g5wpYedwOPpe5ik4gAJZypNPo+OWSwx+9B/oh1edmiVLmc7B27R5aNTMFEwHQYDVR0OBBYEFCa+l1qJuMt65y3r8LpUd1nn4ZskMB8GA1UdIwQYMBaAFCa+l1qJuMt65y3r8LpUd1nn4ZskMA8GA1UdEwEB/wQFMAMBAf8wCgYIKoZIzj0EAwIDRwAwRAIgb30ehVGOLIcLAtp5KPQZe4XABw5YB6/RsfPohgHI0koCIB7QH0Q8VITDSUH7xcx2dB+o8v52aQ41aNWkOKKlOBxZ";
    const KEY_A: &str = "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgeK2soA1oQl67TSeqFC5+Rh+1fXMRysAKD+m61YxRGIihRANCAATgzFMsWlg8I48AmBCkrsI/kVRlQwRKx+jeDnClh53A4+l7mKTiAAlnKk0+j45ZLDH70H+iHV52aJUuZzsHbtHl";
    const KEY_B: &str = "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgGuvAYduTuYYXxngUZ2GfEwcGbnfWkBqROpstCq0WmnqhRANCAASCqNjiNzot0I8qBO20UpAbpqGVL5k27yRgevV/hYZBoC6Dit995oz04QtMSki03hUk+J+B2ScUjib9FSjR8Vau";

    fn b64(s: &str) -> Vec<u8> {
        base64::engine::general_purpose::STANDARD.decode(s).unwrap()
    }

    /// Serves `cert_a` signing with `key`. With key A that is an honest server; with key B it is an attacker who has
    /// only copied the public certificate.
    #[derive(Debug)]
    struct Fixed(Arc<CertifiedKey>);

    impl ResolvesServerCert for Fixed {
        fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
            Some(Arc::clone(&self.0))
        }
    }

    fn server_config(key_der: &str) -> Arc<rustls::ServerConfig> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let key = provider.key_provider.load_private_key(PrivateKeyDer::try_from(b64(key_der)).unwrap()).unwrap();
        let certified = Arc::new(CertifiedKey::new(vec![CertificateDer::from(b64(CERT_A))], key));
        Arc::new(rustls::ServerConfig::builder_with_provider(provider).with_safe_default_protocol_versions().unwrap().with_no_client_auth().with_cert_resolver(Arc::new(Fixed(certified))))
    }

    async fn handshake(server_key: &str) -> io::Result<(TlsStream<tokio::io::DuplexStream>, x509_cert::Certificate)> {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let acceptor = tokio_rustls::TlsAcceptor::from(server_config(server_key));
        let server = tokio::spawn(async move {
            // The server side may fail (the client hangs up on a bad signature); only the client's verdict matters.
            let _ = acceptor.accept(server_io).await;
        });
        let result = upgrade(client_io, "rdp.test").await;
        server.abort();
        result
    }

    #[tokio::test]
    async fn a_server_that_owns_its_certificate_connects_and_the_certificate_comes_back() {
        let (_stream, cert) = handshake(KEY_A).await.expect("an honest server is accepted");
        let info = crate::certs::cert_info(&cert).unwrap();
        assert_eq!(info.fingerprint, crate::certs::fingerprint(&b64(CERT_A)));
    }

    #[tokio::test]
    async fn a_server_replaying_a_certificate_without_its_key_is_refused() {
        let err = match handshake(KEY_B).await {
            Ok(_) => panic!("a certificate copied without its private key must not complete the handshake"),
            Err(e) => e,
        };
        let text = err.to_string().to_ascii_lowercase();
        assert!(text.contains("signature") || text.contains("certificate"), "unexpected failure: {err}");
    }
}
