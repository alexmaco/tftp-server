use packet::*;
use server::*;
use std::net::IpAddr;
use std::result;
use std::time::Duration;
use tftp_proto::{self, *};

struct MockTransfer;
struct MockProto;

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

    fn rx_initial(
        &mut self,
        _: Packet,
    ) -> (
        Option<Self::Transfer>,
        result::Result<Packet, tftp_proto::TftpError>,
    ) {
        (None, Ok(Packet::from(ErrorCode::NotDefined)))
    }

    fn new(_: IO, _: IOPolicyCfg) -> Self {
        MockProto
    }
}

type NopServer = TftpServerImpl<MockProto, FSAdapter>;

#[test]
fn needs_addresses() {
    let mut cfg: ServerConfig = Default::default();
    cfg.addrs = vec![];
    assert!(
        NopServer::with_cfg(&cfg).is_err(),
        "server creation succeeded without addresses"
    );
}

#[test]
fn binds_to_random_port() {
    let ip = IpAddr::from([0; 4]);

    let mut cfg: ServerConfig = Default::default();
    cfg.addrs = vec![(ip, 0)];
    let s = NopServer::with_cfg(&cfg).unwrap();

    let mut v = vec![];
    assert!(s.get_local_addrs(&mut v).is_ok());
    assert_matches!(v.as_slice(), &[addr] if addr.ip() == ip);
}
