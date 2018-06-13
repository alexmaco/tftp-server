use packet::*;
use server::*;
use std::io;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::result;
use std::sync::mpsc::*;
use std::sync::*;
use std::thread;
use std::time::Duration;
use tftp_proto::{self, *};

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
    let (proto, _, _) = MockProto::new();
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
    let (proto, _, _) = MockProto::new();
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
    let (xfer, trans) = MockTransfer::new();
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

    let pack_from_xfer = trans.rx.recv().unwrap();
    assert_eq!(pack_from_xfer, pack_3);

    let resp: Response = vec![ResponseItem::Packet(pack_4.clone())].into();
    trans.tx.send(Ok(resp)).unwrap();

    let (_, remote) = sock.recv_from(&mut buf).unwrap();
    assert_ne!(remote.port(), addr.port(), "transfer TID not constant");
}

#[test]
fn error_for_different_transfer_port() {
    let mut buf = [0; 1024];
    let (xfer, _) = MockTransfer::new();
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
    let (amt, err_remote) = sock_2.recv_from(&mut buf).unwrap();
    assert_matches!(Packet::read(&buf[..amt]), Ok(Packet::ERROR { .. }));
    assert_eq!(err_remote, remote);
}

#[test]
fn transfer_start_end() {
    let mut buf = [0; 1024];
    let (xfer, trans) = MockTransfer::new();
    let (addr, rx, tx) = create_server();
    let sock = make_socket(None);

    let pack = Packet::ACK(5);

    sock.send_to(&pack.to_bytes().unwrap(), &addr).unwrap();
    let _ = rx.recv().unwrap();

    tx.send((Some(xfer), Ok(pack.clone()))).unwrap();
    let (_, remote) = sock.recv_from(&mut buf).unwrap();

    sock.send_to(&pack.to_bytes().unwrap(), &remote).unwrap();
    let _ = trans.rx.recv().unwrap();

    let resp: Response = ResponseItem::Done.into();
    trans.set_done(true);
    trans.tx.send(Ok(resp)).unwrap();

    // no packet expected after Done
    sock.send_to(&pack.to_bytes().unwrap(), &remote).unwrap();
    assert_eq!(
        trans.rx.recv_timeout(Duration::from_millis(3500)),
        Err(RecvTimeoutError::Timeout)
    );
}

#[test]
fn transfer_start_timeout_repeat() {
    let mut buf = [0; 1024];
    let (xfer, trans) = MockTransfer::new();
    let (addr, rx, tx) = create_server();
    let sock = make_socket(None);

    let pack_1 = Packet::ACK(33);
    let pack_2 = Packet::ACK(44);

    sock.send_to(&pack_1.to_bytes().unwrap(), &addr).unwrap();
    let _ = rx.recv().unwrap();

    tx.send((Some(xfer), Ok(pack_2.clone()))).unwrap();
    let (_, remote) = sock.recv_from(&mut buf).unwrap();

    thread::sleep(Duration::from_millis(3500));
    trans.timeout_tx.send(ResponseItem::RepeatLast(1)).unwrap();

    let (amt, remote_2) = sock.recv_from(&mut buf).unwrap();
    assert_eq!(&buf[..amt], pack_2.to_bytes().unwrap().as_slice());
    assert_eq!(remote, remote_2, "packet repeated from different address");
}

type XferStart = (
    Option<MockTransfer>,
    result::Result<Packet, tftp_proto::TftpError>,
);

struct TransferHandle {
    rx: Receiver<Packet>,
    tx: Sender<result::Result<Response, tftp_proto::TftpError>>,
    timeout_tx: SyncSender<ResponseItem>,
    done: Arc<Mutex<bool>>,
}

impl TransferHandle {
    fn set_done(&self, state: bool) {
        *self
            .done
            .lock()
            .expect("error locking done from test thread") = state;
    }
}

struct MockTransfer {
    tx: Sender<Packet>,
    rx: Receiver<result::Result<Response, tftp_proto::TftpError>>,
    timeout_rx: Receiver<ResponseItem>,
    done: Arc<Mutex<bool>>,
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
        self.timeout_rx.recv().expect("cannot receive timeout response")
    }
    fn is_done(&self) -> bool {
        *self.done.lock().expect("error locking 'done'")
    }
}

impl<IO: IOAdapter> Proto<IO> for MockProto {
    type Transfer = MockTransfer;

    fn rx_initial(&mut self, packet: Packet) -> XferStart {
        self.tx.send(packet).expect("error sending packet to test");
        self.rx.recv().expect("error receiving packet from test")
    }
}

impl MockProto {
    fn new() -> (Self, Receiver<Packet>, Sender<XferStart>) {
        let (out_tx, out_rx) = channel();
        let (in_tx, in_rx) = channel();
        (
            Self {
                tx: out_tx,
                rx: in_rx,
            },
            out_rx,
            in_tx,
        )
    }
}

impl MockTransfer {
    fn new() -> (Self, TransferHandle) {
        let (out_tx, out_rx) = channel();
        let (in_tx, in_rx) = channel();
        let done = Arc::new(Mutex::new(false));
        let (timeout_in_tx, timeout_in_rx) = sync_channel(0);
        (
            Self {
                tx: out_tx,
                rx: in_rx,
                timeout_rx: timeout_in_rx,
                done: done.clone(),
            },
            TransferHandle {
                rx: out_rx,
                tx: in_tx,
                timeout_tx: timeout_in_tx,
                done,
            },
        )
    }
}

fn create_server() -> (SocketAddr, Receiver<Packet>, Sender<XferStart>) {
    let (tx, rx) = channel();
    let (proto, start_rx, start_tx) = MockProto::new();

    thread::Builder::new()
        .name("server_thread".into())
        .spawn(move || {
            let mut cfg: ServerConfig = Default::default();
            cfg.addrs = vec![(IpAddr::from([127, 0, 0, 1]), 0)];
            let mut server = MockedServer::create(&cfg, proto).unwrap();
            let mut addrs = vec![];
            server.local_addresses(&mut addrs).unwrap();
            tx.send(addrs[0]).unwrap();

            if let Err(e) = server.run() {
                panic!("run ended with error: {:?}", e);
            }
        })
        .expect("cannot spawn server_thread");

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
