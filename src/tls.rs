//! Optional fixed-authority TLS transport experiment. No persistent private keys.
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf},
    net::TcpStream,
    time::timeout,
};
use tokio_rustls::{
    TlsAcceptor, TlsConnector,
    rustls::{
        self,
        pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName},
    },
};

pub struct Intercept {
    pub authority: String,
    pub ca_der: CertificateDer<'static>,
    host: String,
    acceptor: TlsAcceptor,
    connector: TlsConnector,
}
impl std::fmt::Debug for Intercept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Intercept")
            .field("authority", &self.authority)
            .finish_non_exhaustive()
    }
}
impl Intercept {
    /// Generates keys synchronously. Call at startup outside the async network runtime.
    pub fn new(
        authority: String,
        roots: rustls::RootCertStore,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let parsed: http::uri::Authority = authority.parse()?;
        if parsed.port_u16().is_none() || authority.contains('@') {
            return Err("TLS authority requires explicit port and no userinfo".into());
        }
        let host = parsed
            .host()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_owned();
        let mut ca = rcgen::CertificateParams::new(Vec::<String>::new())?;
        ca.not_before = time::OffsetDateTime::now_utc() - time::Duration::minutes(5);
        ca.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(1);
        ca.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        ca.key_usages = vec![
            rcgen::KeyUsagePurpose::KeyCertSign,
            rcgen::KeyUsagePurpose::CrlSign,
        ];
        let issuer = rcgen::CertifiedIssuer::self_signed(ca, rcgen::KeyPair::generate()?)?;
        let mut leaf = rcgen::CertificateParams::new(vec![host.clone()])?;
        // Short-lived experiment certificate, with DNS/IP SAN supplied by rcgen.
        leaf.not_before = time::OffsetDateTime::now_utc() - time::Duration::minutes(5);
        leaf.not_after = time::OffsetDateTime::now_utc() + time::Duration::days(1);
        leaf.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        let key = rcgen::KeyPair::generate()?;
        let certificate = leaf.signed_by(&key, &issuer)?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut server = rustls::ServerConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(
                vec![certificate.der().clone()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )?;
        server.alpn_protocols = vec![b"http/1.1".to_vec()];
        let mut client = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth();
        client.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Self {
            authority,
            host,
            ca_der: issuer.der().clone(),
            acceptor: TlsAcceptor::from(Arc::new(server)),
            connector: TlsConnector::from(Arc::new(client)),
        })
    }

    pub async fn bridge(
        &self,
        client: &mut TcpStream,
        upstream: TcpStream,
        prefix: Vec<u8>,
        handshake_timeout: Duration,
        idle_timeout: Duration,
    ) -> io::Result<(u64, u64)> {
        // Verify the real server first. A failure is still an HTTP error before CONNECT success.
        let name = ServerName::try_from(self.host.clone()).map_err(io::Error::other)?;
        let upstream = timeout(handshake_timeout, self.connector.connect(name, upstream)).await;
        let mut upstream = match upstream {
            Ok(Ok(stream)) => stream,
            result => {
                let _ = client.write_all(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n").await;
                return Err(io::Error::other(format!(
                    "upstream TLS: {}",
                    match result {
                        Ok(Err(e)) => e.to_string(),
                        Err(_) => "handshake timeout".into(),
                        _ => unreachable!(),
                    }
                )));
            }
        };
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        let io = Prefixed {
            prefix: std::io::Cursor::new(prefix),
            socket: client,
        };
        let mut downstream = timeout(handshake_timeout, self.acceptor.accept(io))
            .await
            .map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "downstream TLS handshake timeout")
            })?
            .map_err(|e| io::Error::other(format!("downstream TLS: {e}")))?;
        crate::tunnel::copy(&mut downstream, &mut upstream, idle_timeout).await
    }
}

// CONNECT and ClientHello may share a read. Preserve those bytes for the TLS acceptor.
pub(crate) struct Prefixed<'a> {
    pub(crate) prefix: std::io::Cursor<Vec<u8>>,
    pub(crate) socket: &'a mut TcpStream,
}
impl AsyncRead for Prefixed<'_> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.prefix.position() < self.prefix.get_ref().len() as u64 {
            Pin::new(&mut self.prefix).poll_read(cx, buf)
        } else {
            Pin::new(&mut *self.socket).poll_read(cx, buf)
        }
    }
}
impl AsyncWrite for Prefixed<'_> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut *self.socket).poll_write(cx, data)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.socket).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut *self.socket).poll_shutdown(cx)
    }
}
