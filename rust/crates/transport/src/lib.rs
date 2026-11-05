//! Byte transports to a vehicle: serial, TCP, UDP (server and client) and WebSocket.
//! Every transport is split into an independent reader and writer so a pending read never
//! blocks a write. MAVLink framing is not done here.
use std::{
    fmt,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Serial {
        port: String,
        baud: u32,
    },
    Tcp {
        host: String,
        port: u16,
    },
    /// Listens on a local port and answers whoever sent the last datagram.
    Udp {
        port: u16,
    },
    /// Sends to a remote host and port.
    UdpClient {
        host: String,
        port: u16,
    },
    WebSocket {
        url: String,
    },
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serial { port, baud } => write!(f, "{port} @ {baud}"),
            Self::Tcp { host, port } => write!(f, "TCP {host}:{port}"),
            Self::Udp { port } => write!(f, "UDP :{port}"),
            Self::UdpClient { host, port } => write!(f, "UDPCl {host}:{port}"),
            Self::WebSocket { url } => write!(f, "WS {url}"),
        }
    }
}

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("connection closed")]
    Closed,
    #[error("timed out connecting to {0}")]
    Timeout(String),
}

impl From<std::io::Error> for TransportError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

pub trait LinkReader: Send {
    /// Reads at least one byte. Returns `Closed` when the peer goes away.
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> BoxFuture<'a, Result<usize, TransportError>>;
}

pub trait LinkWriter: Send {
    fn write<'a>(&'a mut self, data: &'a [u8]) -> BoxFuture<'a, Result<(), TransportError>>;
}

pub struct Connection {
    pub reader: Box<dyn LinkReader>,
    pub writer: Box<dyn LinkWriter>,
    pub description: String,
    /// The bound local port for UDP server endpoints (useful when asking for port 0).
    pub local_port: Option<u16>,
}

impl Connection {
    /// Wraps any async byte streams (used for in-process links such as log replay).
    pub fn from_io<R, W>(reader: R, writer: W, description: impl Into<String>) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self {
            reader: Box::new(StreamReader(reader)),
            writer: Box::new(StreamWriter(writer)),
            description: description.into(),
            local_port: None,
        }
    }
}

pub async fn open(endpoint: &Endpoint) -> Result<Connection, TransportError> {
    let description = endpoint.to_string();
    match endpoint {
        Endpoint::Serial { port, baud } => open_serial(port, *baud, description),
        Endpoint::Tcp { host, port } => open_tcp(host, *port, description).await,
        Endpoint::Udp { port } => open_udp_server(*port, description).await,
        Endpoint::UdpClient { host, port } => open_udp_client(host, *port, description).await,
        Endpoint::WebSocket { url } => open_websocket(url, description).await,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialPortInfo {
    pub name: String,
    pub description: String,
    /// USB vendor and product ids; `None` for ports that are not USB (UARTs, Bluetooth).
    pub vid: Option<u16>,
    pub pid: Option<u16>,
}

pub fn list_serial_ports() -> Vec<SerialPortInfo> {
    let mut ports: Vec<_> = tokio_serial::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let (mut vid, mut pid) = (None, None);
            let description = match p.port_type {
                tokio_serial::SerialPortType::UsbPort(usb) => {
                    (vid, pid) = (Some(usb.vid), Some(usb.pid));
                    usb.product
                        .or(usb.manufacturer)
                        .unwrap_or_else(|| format!("USB {:04x}:{:04x}", usb.vid, usb.pid))
                }
                tokio_serial::SerialPortType::BluetoothPort => "Bluetooth".into(),
                tokio_serial::SerialPortType::PciPort => "PCI".into(),
                tokio_serial::SerialPortType::Unknown => String::new(),
            };
            SerialPortInfo {
                name: p.port_name,
                description,
                vid,
                pid,
            }
        })
        .collect();
    ports.sort_by_key(|p| natural_key(&p.name));
    ports
}

/// Sorts COM2 before COM10.
fn natural_key(name: &str) -> (String, u32) {
    let digits: String = name
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect();
    let number = digits
        .chars()
        .rev()
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    (
        name.trim_end_matches(|c: char| c.is_ascii_digit())
            .to_owned(),
        number,
    )
}

struct StreamReader<R>(R);

impl<R: AsyncRead + Unpin + Send> LinkReader for StreamReader<R> {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> BoxFuture<'a, Result<usize, TransportError>> {
        Box::pin(async move {
            match self.0.read(buf).await? {
                0 => Err(TransportError::Closed),
                n => Ok(n),
            }
        })
    }
}

struct StreamWriter<W>(W);

impl<W: AsyncWrite + Unpin + Send> LinkWriter for StreamWriter<W> {
    fn write<'a>(&'a mut self, data: &'a [u8]) -> BoxFuture<'a, Result<(), TransportError>> {
        Box::pin(async move {
            self.0.write_all(data).await?;
            Ok(())
        })
    }
}

fn open_serial(port: &str, baud: u32, description: String) -> Result<Connection, TransportError> {
    use tokio_serial::SerialPortBuilderExt;
    let stream = tokio_serial::new(port, baud)
        .open_native_async()
        .map_err(|e| TransportError::Io(format!("{port}: {e}")))?;
    let (reader, writer) = tokio::io::split(stream);
    Ok(Connection {
        reader: Box::new(StreamReader(reader)),
        writer: Box::new(StreamWriter(writer)),
        description,
        local_port: None,
    })
}

async fn open_tcp(
    host: &str,
    port: u16,
    description: String,
) -> Result<Connection, TransportError> {
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .map_err(|_| TransportError::Timeout(description.clone()))??;
    stream.set_nodelay(true)?;
    let (reader, writer) = stream.into_split();
    Ok(Connection {
        reader: Box::new(StreamReader(reader)),
        writer: Box::new(StreamWriter(writer)),
        description,
        local_port: None,
    })
}

struct UdpServerReader {
    socket: Arc<UdpSocket>,
    peer: Arc<Mutex<Option<SocketAddr>>>,
}

impl LinkReader for UdpServerReader {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> BoxFuture<'a, Result<usize, TransportError>> {
        Box::pin(async move {
            let (n, from) = self.socket.recv_from(buf).await?;
            *self.peer.lock().expect("peer lock") = Some(from);
            Ok(n)
        })
    }
}

struct UdpServerWriter {
    socket: Arc<UdpSocket>,
    peer: Arc<Mutex<Option<SocketAddr>>>,
}

impl LinkWriter for UdpServerWriter {
    fn write<'a>(&'a mut self, data: &'a [u8]) -> BoxFuture<'a, Result<(), TransportError>> {
        Box::pin(async move {
            let peer = *self.peer.lock().expect("peer lock");
            // Nobody has talked to us yet: there is no one to answer, which is not an error.
            if let Some(peer) = peer {
                self.socket.send_to(data, peer).await?;
            }
            Ok(())
        })
    }
}

async fn open_udp_server(port: u16, description: String) -> Result<Connection, TransportError> {
    let socket = Arc::new(UdpSocket::bind(("0.0.0.0", port)).await?);
    let local_port = socket.local_addr()?.port();
    let peer = Arc::new(Mutex::new(None));
    Ok(Connection {
        reader: Box::new(UdpServerReader {
            socket: socket.clone(),
            peer: peer.clone(),
        }),
        writer: Box::new(UdpServerWriter { socket, peer }),
        description,
        local_port: Some(local_port),
    })
}

struct UdpClientReader(Arc<UdpSocket>);
struct UdpClientWriter(Arc<UdpSocket>);

impl LinkReader for UdpClientReader {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> BoxFuture<'a, Result<usize, TransportError>> {
        Box::pin(async move { Ok(self.0.recv(buf).await?) })
    }
}

impl LinkWriter for UdpClientWriter {
    fn write<'a>(&'a mut self, data: &'a [u8]) -> BoxFuture<'a, Result<(), TransportError>> {
        Box::pin(async move {
            self.0.send(data).await?;
            Ok(())
        })
    }
}

async fn open_udp_client(
    host: &str,
    port: u16,
    description: String,
) -> Result<Connection, TransportError> {
    let remote = tokio::net::lookup_host((host, port))
        .await?
        .next()
        .ok_or_else(|| TransportError::Io(format!("cannot resolve {host}")))?;
    let bind = if remote.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let socket = UdpSocket::bind(bind).await?;
    socket.connect(remote).await?;
    let local_port = socket.local_addr()?.port();
    let socket = Arc::new(socket);
    Ok(Connection {
        reader: Box::new(UdpClientReader(socket.clone())),
        writer: Box::new(UdpClientWriter(socket)),
        description,
        local_port: Some(local_port),
    })
}

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct WsReader {
    stream: SplitStream<WsStream>,
    pending: Vec<u8>,
}

impl LinkReader for WsReader {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> BoxFuture<'a, Result<usize, TransportError>> {
        Box::pin(async move {
            while self.pending.is_empty() {
                match self.stream.next().await {
                    Some(Ok(Message::Binary(data))) => self.pending = data.to_vec(),
                    Some(Ok(Message::Close(_))) | None => return Err(TransportError::Closed),
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(TransportError::Io(e.to_string())),
                }
            }
            let n = self.pending.len().min(buf.len());
            buf[..n].copy_from_slice(&self.pending[..n]);
            self.pending.drain(..n);
            Ok(n)
        })
    }
}

struct WsWriter(SplitSink<WsStream, Message>);

impl LinkWriter for WsWriter {
    fn write<'a>(&'a mut self, data: &'a [u8]) -> BoxFuture<'a, Result<(), TransportError>> {
        Box::pin(async move {
            self.0
                .send(Message::Binary(data.to_vec().into()))
                .await
                .map_err(|e| TransportError::Io(e.to_string()))
        })
    }
}

async fn open_websocket(url: &str, description: String) -> Result<Connection, TransportError> {
    let (stream, _) = tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(url))
        .await
        .map_err(|_| TransportError::Timeout(description.clone()))?
        .map_err(|e| TransportError::Io(e.to_string()))?;
    let (sink, stream) = stream.split();
    Ok(Connection {
        reader: Box::new(WsReader {
            stream,
            pending: Vec::new(),
        }),
        writer: Box::new(WsWriter(sink)),
        description,
        local_port: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn read_exact(reader: &mut dyn LinkReader, want: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = [0u8; 64];
        while out.len() < want {
            let n = tokio::time::timeout(Duration::from_secs(3), reader.read(&mut buf))
                .await
                .expect("read timed out")
                .expect("read failed");
            out.extend_from_slice(&buf[..n]);
        }
        out
    }

    #[tokio::test]
    async fn tcp_roundtrip_and_close() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            sock.write_all(b"hello").await.unwrap();
            let mut buf = [0u8; 4];
            sock.read_exact(&mut buf).await.unwrap();
            buf
        });
        let mut conn = open(&Endpoint::Tcp {
            host: "127.0.0.1".into(),
            port,
        })
        .await
        .unwrap();
        assert_eq!(read_exact(conn.reader.as_mut(), 5).await, b"hello");
        conn.writer.write(b"ping").await.unwrap();
        assert_eq!(&server.await.unwrap(), b"ping");
        let mut buf = [0u8; 8];
        assert!(matches!(
            conn.reader.read(&mut buf).await,
            Err(TransportError::Closed)
        ));
    }

    #[tokio::test]
    async fn tcp_connect_failure_is_an_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let result = open(&Endpoint::Tcp {
            host: "127.0.0.1".into(),
            port,
        })
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn udp_server_answers_last_sender() {
        let mut conn = open(&Endpoint::Udp { port: 0 }).await.unwrap();
        let port = conn.local_port.unwrap();
        // Writing before anyone talked to us is silently dropped.
        conn.writer.write(b"nobody").await.unwrap();
        let vehicle = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        vehicle.send_to(b"hb", ("127.0.0.1", port)).await.unwrap();
        assert_eq!(read_exact(conn.reader.as_mut(), 2).await, b"hb");
        conn.writer.write(b"reply").await.unwrap();
        let mut buf = [0u8; 16];
        let (n, _) = tokio::time::timeout(Duration::from_secs(3), vehicle.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf[..n], b"reply");
    }

    #[tokio::test]
    async fn udp_client_talks_to_remote() {
        let remote = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = remote.local_addr().unwrap().port();
        let mut conn = open(&Endpoint::UdpClient {
            host: "127.0.0.1".into(),
            port,
        })
        .await
        .unwrap();
        conn.writer.write(b"hi").await.unwrap();
        let mut buf = [0u8; 16];
        let (n, from) = tokio::time::timeout(Duration::from_secs(3), remote.recv_from(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&buf[..n], b"hi");
        remote.send_to(b"back", from).await.unwrap();
        assert_eq!(read_exact(conn.reader.as_mut(), 4).await, b"back");
    }

    #[tokio::test]
    async fn websocket_binary_roundtrip() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (sock, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(sock).await.unwrap();
            while let Some(Ok(msg)) = ws.next().await {
                if msg.is_binary() {
                    ws.send(msg).await.unwrap();
                }
            }
        });
        let mut conn = open(&Endpoint::WebSocket {
            url: format!("ws://127.0.0.1:{port}/mavlink"),
        })
        .await
        .unwrap();
        conn.writer.write(&[1, 2, 3, 4, 5]).await.unwrap();
        assert_eq!(read_exact(conn.reader.as_mut(), 5).await, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn serial_ports_sort_naturally() {
        assert!(natural_key("COM2") < natural_key("COM10"));
    }

    #[test]
    fn endpoint_descriptions_are_readable() {
        let e = Endpoint::Serial {
            port: "COM3".into(),
            baud: 115_200,
        };
        assert_eq!(e.to_string(), "COM3 @ 115200");
        assert_eq!(Endpoint::Udp { port: 14550 }.to_string(), "UDP :14550");
    }
}
