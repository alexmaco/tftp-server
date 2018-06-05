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

Note: packet exchanges do not make protocol sense, we're just testing packet movement, so only ACKs for simplicity

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
    let (addr, rx, tx) = create_server();
    let sock = make_socket(None);

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
    let (xfer, trans_rx, trans_tx) = xfer_with_chans();
    let (addr, rx, tx) = create_server();
    let sock = make_socket(None);

    let pack_1 = Packet::ACK(33);
    let pack_2 = Packet::ACK(44);
    let pack_3 = Packet::ACK(55);
    let pack_4 = Packet::ACK(66);
    sock.send_to(&pack_1.to_bytes().unwrap(), &addr).unwrap();

    let _ = rx.recv().unwrap();
    tx.send((Some(xfer), Ok(pack_2.clone()))).unwrap();

    let (amt, remote) = sock.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..amt], pack_2.to_bytes().unwrap().as_slice());
    assert_ne!(
        remote.port(),
        addr.port(),
        "distinct TID was not allocated for transfer"
    );

    sock.send_to(&pack_3.to_bytes().unwrap(), &remote).unwrap();

    let pack_from_xfer = trans_rx.recv().unwrap();
    assert_eq!(pack_from_xfer, pack_3);

    let resp: Response = vec![ResponseItem::Packet(pack_4.clone())].into();
    trans_tx.send(Ok(resp)).unwrap();

    let (amt, remote) = sock.recv_from(&mut buf).unwrap();
    assert_ne!(remote.port(), addr.port(), "transfer TID not constant");
}

#[test]
fn error_for_different_transfer_port() {
    let mut buf = [0; 1024];
    let (xfer, trans_rx, trans_tx) = xfer_with_chans();
    let (addr, rx, tx) = create_server();
    let sock = make_socket(None);

    let pack_1 = Packet::ACK(33);
    let pack_2 = Packet::ACK(44);
    let pack_3 = Packet::ACK(55);
    sock.send_to(&pack_1.to_bytes().unwrap(), &addr).unwrap();

    let _ = rx.recv().unwrap();
    tx.send((Some(xfer), Ok(pack_2.clone()))).unwrap();
    let (_, remote) = sock.recv_from(&mut buf).unwrap();

    // now send from different port
    let sock_2 = make_socket(None);
    sock_2
        .send_to(&pack_3.to_bytes().unwrap(), &remote)
        .unwrap();

    // not doing rx from channel, this should not reach the proto impl
    let (amt, err_remote) = sock.recv_from(&mut buf).unwrap();
    assert_matches!(Packet::read(&buf[..amt]), Ok(Packet::ERROR { .. }));
    assert_eq!(err_remote, remote);
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

fn create_server() -> (SocketAddr, Receiver<Packet>, Sender<XferStart>) {
    let (tx, rx) = channel();
    let (proto, start_rx, start_tx) = proto_with_chans();

    thread::spawn(move || {
        let mut cfg: ServerConfig = Default::default();
        cfg.addrs = vec![(IpAddr::from([127, 0, 0, 1]), 0)];
        let mut server = MockedServer::create(&cfg, proto).unwrap();
        let mut addrs = vec![];
        server.local_addresses(&mut addrs).unwrap();
        tx.send(addrs[0]).unwrap();

        if let Err(e) = server.run() {
            panic!("run ended with error: {:?}", e);
        }
    });

    (rx.recv().unwrap(), start_rx, start_tx)
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
