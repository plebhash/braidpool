use bytes::Bytes;
pub use quinn::Endpoint;
use std::{error::Error, net::SocketAddr, sync::Arc};

use crate::protocol::{self, HandshakeMessage, Message, ProtocolMessage};

// todo:
// pub struct QuicEndpoint {
//     ep: quinn::Endpoint,
//     conns: Vec<QuicConnection>,
// }
//
// pub struct QuicConnection {
//     streams: Vec<QuicStream>,
// }

pub struct QuicBiDirectionalStream {
    reader: quinn::RecvStream,
    writer: quinn::SendStream,
}

impl QuicBiDirectionalStream {
    pub fn new(
        reader: quinn::RecvStream,
        writer: quinn::SendStream,
    ) -> QuicBiDirectionalStream {
        QuicBiDirectionalStream {
            reader,
            writer,
        }
    }

    pub async fn start_from_connect(&mut self, addr: &SocketAddr) -> Result<(), Box<dyn Error>> {
        use futures::SinkExt;
        log::info!("Starting from connect");
        let message = HandshakeMessage::start(addr).unwrap();
        self.writer.write_all(message.as_bytes().unwrap().as_slice()).await?;
        // this ends the stream!!!!
        // self.writer.finish().await?;
        self.start_read_loop().await?;
        Ok(())
    }

    pub async fn start_from_accept(&mut self) -> Result<(), Box<dyn Error>> {
        log::info!("Starting from accept");
        self.start_read_loop().await?;
        Ok(())
    }

    pub async fn start_read_loop(&mut self) -> Result<(), Box<dyn Error>> {
        use futures::StreamExt;
        log::info!("Start read loop....");
        loop {
            match self.reader.read_to_end(64).await {
                // Ok(None) => {
                //     return Err("peer closed connection".into());
                // }
                // Ok(Some(a)) => {
                //     println!("aaaaa {:?}", a);
                //     if self.message_received(&buf).await.is_err() {
                //         return Err("peer closed connection".into());
                //     }
                // },
                Ok(msg) => {
                    println!("aaa {:?}", msg);
                    if self.message_received(&msg).await.is_err() {
                        return Err("peer closed connection".into());
                    }
                }
                Err(_) => return Err("error receiving stream".into()),
            };
        }
        Ok(())
    }

    async fn message_received(&mut self, message: &[u8]) -> Result<(), &'static str> {
        use futures::SinkExt;

        let message: Message = protocol::Message::from_bytes(message).unwrap();
        match message.response_for_received() {
            Ok(result) => {
                if let Some(response) = result {
                    if let Some(to_send) = response.as_bytes() {
                        if (self.writer.write(to_send.as_slice()).await).is_err() {
                            return Err("Send failed: Closing peer connection");
                        }
                    } else {
                        return Err("Error serializing: Closing peer connection");
                    }
                }
            }
            Err(_) => {
                return Err("Error constructing response: Closing peer connection");
            }
        }
        Ok(())
    }
}

/// Dummy certificate verifier that treats any certificate as valid.
/// NOTE, such verification is vulnerable to MITM attacks, but convenient for testing.
struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::client::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

pub fn configure_client() -> quinn::ClientConfig {
    let crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();

    let mut transport_config = quinn::TransportConfig::default();

    // infinite idle timeout
    transport_config.max_idle_timeout(None);

    let mut client = quinn::ClientConfig::new(Arc::new(crypto));
    client.transport_config(Arc::new(transport_config));
    client
}

pub fn configure_server() -> Result<(quinn::ServerConfig, Vec<u8>), Box<dyn Error>> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_der = cert.serialize_der()?;
    let priv_key = cert.serialize_private_key_der();
    let priv_key = rustls::PrivateKey(priv_key);
    let cert_chain = vec![rustls::Certificate(cert_der.clone())];

    let mut server_config = quinn::ServerConfig::with_single_cert(cert_chain, priv_key)?;
    let transport_config =
        Arc::get_mut(&mut server_config.transport).ok_or("cannot get server_config")?;

    // infinite idle timeout
    transport_config.max_idle_timeout(None);

    Ok((server_config, cert_der))
}

