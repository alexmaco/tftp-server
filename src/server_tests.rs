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

/*

How these tests work:
The idea is we spawn the server on a separate thread, and talk to it via a socket.
The server uses MockProto and MockTransfer in place of the real ones.
These 2 have to pairs of channels, and they simply forward and return via the channels in rx_initial and rx
The test receives, checks, and sends via the channels, alternating with sending/receiving packets via the socket

Note: FSAdapter is just there because rust doesn't have proper higher kinded types. FSAdapter never does anything here

*/

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

#[test]
fn transfer_start_exchange() {
    let mut buf = [0; 1024];
    let (proto, rx, tx) = proto_with_chans();
    let (xfer, trans_rx, trans_tx) = xfer_with_chans();
    let addr = create_server(proto);
    let sock = make_socket(None);

    // Note: this does not make protocol sense, we're just testing packet movement
    let pack = Packet::ACK(33);
    sock.send_to(&pack.to_bytes().unwrap(), &addr).unwrap();

    let _ = rx.recv().unwrap();
    tx.send((Some(xfer), Ok(pack.clone()))).unwrap();

    let (amt, remote) = sock.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..amt], pack.to_bytes().unwrap().as_slice());
    assert_ne!(
        remote.port(),
        addr.port(),
        "distinct TID was not allocated for transfer"
    );

    let pack = Packet::ACK(42);
    sock.send_to(&pack.to_bytes().unwrap(), &remote).unwrap();

    let pack_from_xfer = trans_rx.recv().unwrap();
    assert_eq!(pack_from_xfer, pack);

    let resp_pack = Packet::ACK(5);
    let resp: Response = vec![ResponseItem::Packet(resp_pack.clone())].into();
    trans_tx.send(Ok(resp));

    let (amt, remote) = sock.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..amt], resp_pack.to_bytes().unwrap().as_slice());
}

type XferStart = (
    Option<MockTransfer>,
    result::Result<Packet, tftp_proto::TftpError>,
);

struct MockTransfer {
    tx: Sender<Packet>,
    rx: Receiver<result::Result<Response, tftp_proto::TftpError>>,
}

struct MockProto {
    tx: Sender<Packet>,
    rx: Receiver<XferStart>,
}

impl Transfer for MockTransfer {
    fn rx(&mut self, packet: Packet) -> result::Result<Response, tftp_proto::TftpError> {
        self.tx.send(packet).expect("error sending from transfer");
        self.rx.recv().expect("error receiving in transfer")
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

fn xfer_with_chans() -> (
    MockTransfer,
    Receiver<Packet>,
    Sender<result::Result<Response, tftp_proto::TftpError>>,
) {
    let (out_tx, out_rx) = channel();
    let (in_tx, in_rx) = channel();
    (
        MockTransfer {
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
