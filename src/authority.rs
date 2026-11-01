//! Persistent project authority. Loading/key generation is performed by the project worker.
use std::{
    collections::HashMap,
    io,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_rustls::{
    TlsAcceptor, TlsConnector,
    rustls::{
        self,
        pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName},
    },
};
pub struct Authority {
    issuer: rcgen::Issuer<'static, rcgen::KeyPair>,
    pub certificate: CertificateDer<'static>,
    leaves: Mutex<HashMap<String, Arc<rustls::ServerConfig>>>,
    connector: TlsConnector,
}
impl std::fmt::Debug for Authority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Authority").finish_non_exhaustive()
    }
}
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
impl Authority {
    pub fn open(path: &Path) -> io::Result<Self> {
        let key_path = path.join("ca.key.pem");
        let cert_path = path.join("ca.der");
        let (issuer, certificate) = if key_path.exists() || cert_path.exists() {
            let key =
                rcgen::KeyPair::from_pem(&std::fs::read_to_string(key_path)?).map_err(error)?;
            let der = CertificateDer::from(std::fs::read(cert_path)?);
            (
                rcgen::Issuer::from_ca_cert_der(&der, key).map_err(error)?,
                der,
            )
        } else {
            let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).map_err(error)?;
            params.not_before = time::OffsetDateTime::now_utc() - time::Duration::minutes(5);
            params.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(3650);
            params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
            params
                .distinguished_name
                .push(rcgen::DnType::CommonName, "HTTPied project CA");
            params.key_usages = vec![
                rcgen::KeyUsagePurpose::KeyCertSign,
                rcgen::KeyUsagePurpose::CrlSign,
            ];
            let key = rcgen::KeyPair::generate().map_err(error)?;
            let certificate = params.self_signed(&key).map_err(error)?;
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options
                .open(&key_path)?
                .write_all(key.serialize_pem().as_bytes())?;
            std::fs::write(&cert_path, certificate.der())?;
            std::fs::write(path.join("ca.pem"), certificate.pem())?;
            (rcgen::Issuer::new(params, key), certificate.der().clone())
        };
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        Self::with_roots(issuer, certificate, roots)
    }
    pub fn with_roots(
        issuer: rcgen::Issuer<'static, rcgen::KeyPair>,
        certificate: CertificateDer<'static>,
        roots: rustls::RootCertStore,
    ) -> io::Result<Self> {
        let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(error)?
        .with_root_certificates(roots)
        .with_no_client_auth();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Self {
            issuer,
            certificate,
            leaves: Mutex::new(HashMap::new()),
            connector: TlsConnector::from(Arc::new(config)),
        })
    }
    pub async fn acceptor(self: &Arc<Self>, host: String) -> io::Result<TlsAcceptor> {
        let authority = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut cache = authority.leaves.lock().map_err(error)?;
            if let Some(config) = cache.get(&host) {
                return Ok(TlsAcceptor::from(config.clone()));
            }
            let mut params = rcgen::CertificateParams::new(vec![host.clone()]).map_err(error)?;
            params.not_before = time::OffsetDateTime::now_utc() - time::Duration::minutes(5);
            params.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(7);
            params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
            let key = rcgen::KeyPair::generate().map_err(error)?;
            let leaf = params.signed_by(&key, &authority.issuer).map_err(error)?;
            let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .map_err(error)?
            .with_no_client_auth()
            .with_single_cert(
                vec![leaf.der().clone()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .map_err(error)?;
            config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
            let config = Arc::new(config);
            if cache.len() >= 128 {
                cache.clear();
            }
            cache.insert(host, config.clone());
            Ok(TlsAcceptor::from(config))
        })
        .await
        .map_err(error)?
    }
    pub async fn connect(
        &self,
        host: &str,
        stream: tokio::net::TcpStream,
        deadline: Duration,
    ) -> io::Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>> {
        let name = ServerName::try_from(host.to_owned()).map_err(error)?;
        tokio::time::timeout(deadline, self.connector.connect(name, stream))
            .await
            .map_err(error)?
            .map_err(error)
    }
    pub async fn connect_public(
        host: &str,
        stream: tokio::net::TcpStream,
        deadline: Duration,
    ) -> io::Result<tokio_rustls::client::TlsStream<tokio::net::TcpStream>> {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(error)?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let name = ServerName::try_from(host.to_owned()).map_err(error)?;
        tokio::time::timeout(
            deadline,
            TlsConnector::from(Arc::new(config)).connect(name, stream),
        )
        .await
        .map_err(error)?
        .map_err(error)
    }
}
