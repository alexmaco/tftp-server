use packet::*;
use server::*;
use std::io;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::result;
use std::sync::mpsc::*;
use std::thread;
use std::time::Duration;
use tftp_proto::{self, *};

use packet::TransferMode::*;

type MockedServer = TftpServerImpl<MockProto, FSAdapter>;

#[test]
fn needs_addresses() {
    let (proto, _, _) = proto_with_chans();
    let mut cfg: ServerConfig = Default::default();
    cfg.addrs = vec![];
    assert!(
        MockedServer::create(&cfg, proto).is_err(),
        "server creation succeeded without addresses"
    );
}

#[test]
fn binds_to_random_port() {
    let ip = IpAddr::from([0; 4]);

    let mut cfg: ServerConfig = Default::default();
    cfg.addrs = vec![(ip, 0)];
    let (proto, _, _) = proto_with_chans();
    let s = MockedServer::create(&cfg, proto).unwrap();

    let mut v = vec![];
    assert!(s.local_addresses(&mut v).is_ok());
    assert_matches!(v.as_slice(), &[addr] if addr.ip() == ip);
}

#[test]
fn initiating_packet_response() {
    let (proto, rx, tx) = proto_with_chans();
    let addr = create_server(proto);
    let sock = make_socket(None);

    // Note: this does not make protocol sense, we're just testing packet movement
    let pack = Packet::ACK(33);
    sock.send_to(&pack.to_bytes().unwrap(), &addr).unwrap();
    assert_eq!(rx.recv(), Ok(pack));

    let pack = Packet::ACK(7);
    tx.send((None, Ok(pack.clone()))).unwrap();

    let mut buf = [0; 1024];
    let (amt, src) = sock.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..amt], pack.to_bytes().unwrap().as_slice());
    assert_eq!(src.port(), addr.port());
}

type XferStart = (
    Option<MockTransfer>,
    result::Result<Packet, tftp_proto::TftpError>,
);

struct MockTransfer;
struct MockProto {
    tx: Sender<Packet>,
    rx: Receiver<XferStart>,
}

impl Transfer for MockTransfer {
    fn rx(&mut self, _: Packet) -> result::Result<Response, tftp_proto::TftpError> {
        Ok(ResponseItem::Done.into())
    }

    fn timeout(&self) -> Option<Duration> {
        None
    }
    fn timeout_expired(&mut self) -> ResponseItem {
        ResponseItem::Done
    }
    fn is_done(&self) -> bool {
        true
    }
}

impl<IO: IOAdapter> Proto<IO> for MockProto {
    type Transfer = MockTransfer;

    fn rx_initial(&mut self, packet: Packet) -> XferStart {
        self.tx.send(packet).expect("error sending packet to test");
        self.rx.recv().expect("error receiving packet from test")
    }

    fn new(_: IO, _: IOPolicyCfg) -> Self {
        panic!("should not be created");
    }
}

fn proto_with_chans() -> (MockProto, Receiver<Packet>, Sender<XferStart>) {
    let (out_tx, out_rx) = channel();
    let (in_tx, in_rx) = channel();
    (
        MockProto {
            tx: out_tx,
            rx: in_rx,
        },
        out_rx,
        in_tx,
    )
}

fn create_server<P: 'static + Proto<FSAdapter> + Send>(proto: P) -> SocketAddr {
    let (tx, rx) = channel();

    thread::spawn(move || {
        let mut cfg: ServerConfig = Default::default();
        cfg.addrs = vec![(IpAddr::from([127, 0, 0, 1]), 0)];
        let mut server = TftpServerImpl::create(&cfg, proto).unwrap();
        let mut addrs = vec![];
        server.local_addresses(&mut addrs).unwrap();
        tx.send(addrs[0]).unwrap();

        if let Err(e) = server.run() {
            panic!("run ended with error: {:?}", e);
        }
    });

    rx.recv().unwrap()
}

fn make_socket(timeout: Option<Duration>) -> UdpSocket {
    fn make_socket_inner(timeout: Option<Duration>) -> io::Result<UdpSocket> {
        let socket = UdpSocket::bind((IpAddr::from([127, 0, 0, 1]), 0))?;
        socket.set_nonblocking(false)?;
        socket.set_read_timeout(timeout)?;
        socket.set_write_timeout(timeout)?;
        Ok(socket)
    }

    match make_socket_inner(timeout) {
        Ok(sk) => sk,
        Err(e) => panic!("error creating socket: {:?}", e),
    }
}
