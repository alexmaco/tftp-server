use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::iter::Take;
use std::path::Path;
use packet::{ErrorCode, Packet, TftpOption};
use tftp_proto::*;
use std::time::Duration;

use tftp_proto::TftpResult::{Done, Repeat, Reply};
use packet::TransferMode::*;

#[test]
fn initial_ack_err() {
    let iof = TestIoFactory::new();
    let mut server = TftpServerProto::new(iof, Default::default());
    let (xfer, res) = server.rx_initial(Packet::ACK(0));
    assert_eq!(res, Err(TftpError::NotIniatingPacket));
    assert!(xfer.is_none());
}

#[test]
fn initial_data_err() {
    let iof = TestIoFactory::new();
    let mut server = TftpServerProto::new(iof, Default::default());
    let (xfer, res) = server.rx_initial(Packet::DATA {
        block_num: 1,
        data: vec![],
    });
    assert_eq!(res, Err(TftpError::NotIniatingPacket));
    assert!(xfer.is_none());
}

#[test]
fn rrq_no_file_gets_error() {
    let iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    let mut server = TftpServerProto::new(iof, Default::default());
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Octet,
        options: vec![],
    });
    assert_matches!(
        res,
        Ok(Packet::ERROR {
            code: ErrorCode::FileNotFound,
            ..
        })
    );
    assert!(xfer.is_none());
}

#[test]
fn rrq_mail_gets_error() {
    let (mut server, file, _) = rrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Mail,
        options: vec![],
    });
    assert_matches!(
        res,
        Ok(Packet::ERROR {
            code: ErrorCode::NoUser,
            ..
        })
    );
    assert!(xfer.is_none());
}

#[test]
fn rrq_netascii_gets_error() {
    let (mut server, file, _) = rrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Netascii,
        options: vec![],
    });
    assert_matches!(
        res,
        Ok(Packet::ERROR {
            code: ErrorCode::NotDefined,
            ..
        })
    );
    assert!(xfer.is_none());
}

#[test]
fn wrq_netascii_gets_error() {
    let (mut server, file, _) = wrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: Netascii,
        options: vec![],
    });
    assert_matches!(
        res,
        Ok(Packet::ERROR {
            code: ErrorCode::NotDefined,
            ..
        })
    );
    assert!(xfer.is_none());
}

fn rrq_fixture(file_size: usize) -> (TftpServerProto<TestIoFactory>, String, ByteGen) {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.possible_files.insert(file.clone(), file_size);
    iof.server_present_files.insert(file.clone());
    let serv = TftpServerProto::new(iof, Default::default());
    let file_bytes = ByteGen::new(&file);
    (serv, file, file_bytes)
}

#[test]
fn rrq_small_file_ack_end() {
    let (mut server, file, mut file_bytes) = rrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Octet,
        options: vec![],
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(132),
        })
    );
    let mut xfer = xfer.unwrap();
    assert!(!xfer.is_done());
    assert_eq!(xfer.timeout(), None);
    assert_eq!(xfer.rx(Packet::ACK(1)), Done(None));
    assert!(xfer.is_done());
    assert_eq!(xfer.rx(Packet::ACK(0)), Done(None));
}

#[test]
fn rrq_1_block_file() {
    let (mut server, file, mut file_bytes) = rrq_fixture(512);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Octet,
        options: vec![],
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::ACK(1)),
        Reply(Packet::DATA {
            block_num: 2,
            data: vec![],
        })
    );
    assert_eq!(xfer.rx(Packet::ACK(2)), Done(None));
}

#[test]
fn rrq_small_file_ack_wrong_block() {
    let (mut server, file, mut file_bytes) = rrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Octet,
        options: vec![],
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(132),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::ACK(2)),
        Done(Some(Packet::ERROR {
            code: ErrorCode::UnknownID,
            msg: "Incorrect block num in ACK".into(),
        }))
    );
    assert_eq!(xfer.rx(Packet::ACK(0)), Done(None));
}

#[test]
fn rrq_small_file_reply_with_data_illegal() {
    let (mut server, file, mut file_bytes) = rrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Octet,
        options: vec![],
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(132),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_matches!(
        xfer.rx(Packet::DATA {
            data: vec![],
            block_num: 1,
        }),
        Done(Some(Packet::ERROR {
            code: ErrorCode::IllegalTFTP,
            ..
        }))
    );
    assert_eq!(xfer.rx(Packet::ACK(0)), Done(None));
}

#[test]
fn double_rrq() {
    let (mut server, file, mut file_bytes) = rrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file.clone(),
        mode: Octet,
        options: vec![],
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(132),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::RRQ {
            filename: file,
            mode: Octet,
            options: vec![],
        }),
        TftpResult::Err(TftpError::TransferAlreadyRunning)
    );
}

#[test]
fn rrq_2_blocks_ok() {
    let (mut server, file, mut file_bytes) = rrq_fixture(612);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file.clone(),
        mode: Octet,
        options: vec![],
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::ACK(1)),
        Reply(Packet::DATA {
            block_num: 2,
            data: file_bytes.gen(100),
        })
    );
    assert_eq!(xfer.rx(Packet::ACK(2)), Done(None));
}

#[test]
fn rrq_2_blocks_second_lost_ack_repeat_ok() {
    let (mut server, file, mut file_bytes) = rrq_fixture(612);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file.clone(),
        mode: Octet,
        options: vec![],
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::ACK(1)),
        Reply(Packet::DATA {
            block_num: 2,
            data: file_bytes.gen(100),
        })
    );
    // assuming the second data got lost, and the client re-acks the first data
    assert_eq!(xfer.rx(Packet::ACK(1)), Repeat);
    assert_eq!(xfer.rx(Packet::ACK(2)), Done(None));
}

#[test]
fn rrq_large_file_blocknum_wraparound() {
    let size_bytes = 512 * 70_000 + 85;
    let (mut server, file, mut file_bytes) = rrq_fixture(size_bytes);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Octet,
        options: vec![],
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        })
    );
    let mut xfer = xfer.unwrap();

    let mut block = 1u16;
    // loop for one less because we already got the first DATA above
    for _ in 0..(size_bytes / 512) - 1 {
        let new_block = block.wrapping_add(1);
        assert_eq!(
            xfer.rx(Packet::ACK(block)),
            Reply(Packet::DATA {
                block_num: new_block,
                data: file_bytes.gen(512),
            })
        );
        block = new_block;
    }

    let new_block = block.wrapping_add(1);
    assert_eq!(
        xfer.rx(Packet::ACK(block)),
        Reply(Packet::DATA {
            block_num: new_block,
            data: file_bytes.gen(85),
        })
    );
    assert_eq!(xfer.rx(Packet::ACK(new_block)), Done(None));
}

#[test]
fn rrq_small_file_wrq_already_running() {
    let (mut server, file, mut file_bytes) = rrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file.clone(),
        mode: Octet,
        options: vec![],
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(132),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::WRQ {
            filename: file,
            mode: Octet,
            options: vec![],
        }),
        TftpResult::Err(TftpError::TransferAlreadyRunning)
    );
}

#[test]
fn rrq_small_file_err_kills_transfer() {
    let (mut server, file, mut file_bytes) = rrq_fixture(612);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file.clone(),
        mode: Octet,
        options: vec![],
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(xfer.rx(Packet::from(ErrorCode::DiskFull)), Done(None));
    assert!(xfer.is_done());
}

#[test]
fn wrq_already_exists_error() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.possible_files.insert(file.clone(), 132);
    iof.server_present_files.insert(file.clone());
    let mut server = TftpServerProto::new(iof, Default::default());
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: Octet,
        options: vec![],
    });
    assert_matches!(
        res,
        Ok(Packet::ERROR {
            code: ErrorCode::FileExists,
            ..
        })
    );
    assert!(xfer.is_none());
}

fn wrq_fixture(file_size: usize) -> (TftpServerProto<TestIoFactory>, String, ByteGen) {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.possible_files.insert(file.clone(), file_size);
    let serv = TftpServerProto::new(iof, Default::default());
    let file_bytes = ByteGen::new(&file);
    (serv, file, file_bytes)
}

fn wrq_fixture_early_termination(
    file_size: usize,
) -> (TftpServerProto<TestIoFactory>, String, ByteGen) {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.possible_files.insert(file.clone(), file_size);
    iof.enforce_full_write = false;
    let serv = TftpServerProto::new(iof, Default::default());
    let file_bytes = ByteGen::new(&file);
    (serv, file, file_bytes)
}

#[test]
fn wrq_mail_gets_error() {
    let (mut server, file, _) = wrq_fixture(200);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: Mail,
        options: vec![],
    });
    assert_matches!(
        res,
        Ok(Packet::ERROR {
            code: ErrorCode::NoUser,
            ..
        })
    );
    assert!(xfer.is_none());
}

#[test]
fn wrq_small_file_ack_end() {
    let (mut server, file, mut file_bytes) = wrq_fixture(132);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: Octet,
        options: vec![],
    });
    assert_eq!(res, Ok(Packet::ACK(0)));
    let mut xfer = xfer.unwrap();
    assert!(!xfer.is_done());
    assert_eq!(xfer.timeout(), None);
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(132),
        }),
        Done(Some(Packet::ACK(1)))
    );
    assert!(xfer.is_done());
}

#[test]
fn wrq_1_block_file() {
    let (mut server, file, mut file_bytes) = wrq_fixture(512);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: Octet,
        options: vec![],
    });
    assert_eq!(res, Ok(Packet::ACK(0)));
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        }),
        Reply(Packet::ACK(1))
    );
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 2,
            data: vec![],
        }),
        Done(Some(Packet::ACK(2)))
    );
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 2,
            data: vec![],
        }),
        Done(None)
    );
}

#[test]
fn wrq_small_file_reply_with_ack_illegal() {
    let (mut server, file, mut file_bytes) = wrq_fixture(512);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: Octet,
        options: vec![],
    });
    assert_eq!(res, Ok(Packet::ACK(0)));
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        }),
        Reply(Packet::ACK(1))
    );
    assert_matches!(
        xfer.rx(Packet::ACK(3)),
        Done(Some(Packet::ERROR {
            code: ErrorCode::IllegalTFTP,
            ..
        }))
    );
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 2,
            data: vec![],
        }),
        Done(None)
    );
}

#[test]
fn wrq_small_file_block_id_not_1_err() {
    let (mut server, file, mut file_bytes) = wrq_fixture_early_termination(132);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: Octet,
        options: vec![],
    });
    assert_eq!(res, Ok(Packet::ACK(0)));
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 2,
            data: file_bytes.gen(132),
        }),
        Done(Some(Packet::ERROR {
            code: ErrorCode::IllegalTFTP,
            msg: "Data packet lost".to_owned(),
        }))
    );
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 1,
            data: vec![],
        }),
        Done(None)
    );
}

#[test]
fn wrq_large_file_blocknum_wraparound() {
    let size_bytes = 512 * 70_000 + 85;
    let (mut server, file, mut file_bytes) = wrq_fixture(size_bytes);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: Octet,
        options: vec![],
    });
    assert_eq!(res, Ok(Packet::ACK(0)));
    let mut xfer = xfer.unwrap();

    let mut block_num = 1;
    for _ in 0..(size_bytes / 512) {
        assert_eq!(
            xfer.rx(Packet::DATA {
                block_num,
                data: file_bytes.gen(512),
            }),
            Reply(Packet::ACK(block_num))
        );
        block_num = block_num.wrapping_add(1);
    }
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num,
            data: file_bytes.gen(85),
        }),
        Done(Some(Packet::ACK(block_num)))
    );
}

#[test]
fn rrq_blocksize() {
    let (mut server, file, mut file_bytes) = rrq_fixture(1234 + 1233);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Octet,
        options: vec![TftpOption::Blocksize(1234)],
    });
    assert_eq!(
        res,
        Ok(Packet::OACK {
            options: vec![TftpOption::Blocksize(1234)],
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::ACK(0)),
        Reply(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(1234),
        })
    );
    assert_eq!(
        xfer.rx(Packet::ACK(1)),
        Reply(Packet::DATA {
            block_num: 2,
            data: file_bytes.gen(1233),
        })
    );
    assert_eq!(xfer.rx(Packet::ACK(2)), Done(None));
}

#[test]
fn wrq_blocksize() {
    let (mut server, file, mut file_bytes) = wrq_fixture(1234 + 1233);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: Octet,
        options: vec![TftpOption::Blocksize(1234)],
    });
    assert_eq!(
        res,
        Ok(Packet::OACK {
            options: vec![TftpOption::Blocksize(1234)],
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(1234),
        }),
        Reply(Packet::ACK(1))
    );
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 2,
            data: file_bytes.gen(1233),
        }),
        Done(Some(Packet::ACK(2)))
    );
}

#[derive(Debug)]
struct Failer {
    bytes: usize,
}
impl Read for Failer {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.bytes == 0 {
            Err(io::Error::new(io::ErrorKind::Other, "testing read fail"))
        } else {
            let amt = ::std::cmp::min(buf.len(), self.bytes);
            self.bytes -= amt;
            Ok(amt) // pretend we read stuff
        }
    }
}
impl Write for Failer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.bytes == 0 {
            Err(io::Error::new(io::ErrorKind::Other, "testing write fail"))
        } else {
            self.bytes = self.bytes.saturating_sub(buf.len());
            Ok(buf.len()) // pretend we wrote stuff
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct FailIO {
    bytes: usize,
}
impl IOAdapter for FailIO {
    type R = Failer;
    type W = Failer;
    fn open_read(&self, _: &Path) -> io::Result<(Self::R, Option<u64>)> {
        Ok((Failer { bytes: self.bytes }, None))
    }
    fn create_new(&mut self, _: &Path, _: Option<u64>) -> io::Result<Self::W> {
        Ok(Failer { bytes: self.bytes })
    }
}

#[test]
fn rrq_io_error() {
    let fio = FailIO { bytes: 0 };
    let mut server = TftpServerProto::new(fio, Default::default());
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: "".into(),
        mode: Octet,
        options: vec![],
    });
    assert_matches!(res, Ok(Packet::ERROR { .. }));
    assert_matches!(xfer, None);
}

#[test]
fn rrq_io_error_during() {
    let fio = FailIO { bytes: 520 };
    let mut server = TftpServerProto::new(fio, Default::default());
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: "".into(),
        mode: Octet,
        options: vec![],
    });
    assert_matches!(res, Ok(Packet::DATA { .. }));
    let mut xfer = xfer.unwrap();
    assert_matches!(xfer.rx(Packet::ACK(1)), Done(Some(Packet::ERROR { .. })));
    assert!(xfer.is_done());
}

#[test]
fn wrq_io_error() {
    let fio = FailIO { bytes: 0 };
    let mut server = TftpServerProto::new(fio, Default::default());
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: "".into(),
        mode: Octet,
        options: vec![],
    });
    assert_matches!(res, Ok(Packet::ACK(0)));
    let mut xfer = xfer.unwrap();
    assert_matches!(
        xfer.rx(Packet::DATA {
            block_num: 1,
            data: vec![0; 512],
        }),
        Done(Some(Packet::ERROR { .. }))
    );
    assert!(xfer.is_done());
}

#[test]
fn policy_readonly() {
    let mut iof = TestIoFactory::new();
    let amt = 100;
    let file_a = "file_a".to_owned();
    let file_b = "file_b".to_owned();
    iof.possible_files.insert(file_a.clone(), amt);
    iof.server_present_files.insert(file_a.clone());
    iof.possible_files.insert(file_b.clone(), amt);

    let proxy = IOPolicyProxy::new(
        iof,
        IOPolicyCfg {
            readonly: true,
            path: None,
        },
    );
    let mut v = vec![];
    assert_eq!(
        proxy
            .open_read(file_a.as_ref())
            .unwrap()
            .0
            .read_to_end(&mut v)
            .unwrap(),
        amt
    );
    assert_eq!(v.len(), amt);

    let mut proxy = proxy;
    if let Ok(_) = proxy.create_new(file_b.as_ref(), None) {
        panic!("create should not succeed");
    }
}

#[test]
fn policy_remap_directory() {
    // TODO: improve this test
    let mut iof = TestIoFactory::new();
    let amt = 100;
    let file_a = "the_new_path/file_a".to_owned();
    let file_b = "the_new_path/file_b".to_owned();
    iof.possible_files.insert(file_a.clone(), amt);
    iof.possible_files.insert(file_b.clone(), amt);
    iof.server_present_files.insert(file_a.clone());
    iof.enforce_full_write = false;

    let mut proxy = IOPolicyProxy::new(
        iof,
        IOPolicyCfg {
            readonly: false,
            path: Some("the_new_path".into()),
        },
    );
    assert!(proxy.open_read("the_new_path/file_a".as_ref()).is_err());
    assert!(proxy.open_read("file_a".as_ref()).is_ok());

    assert!(
        proxy
            .create_new("the_new_path/file_b".as_ref(), None)
            .is_err()
    );
    assert!(proxy.create_new("file_b".as_ref(), None).is_ok());
}

#[test]
fn policy_refuse_file_read_outside_cwd() {
    let mut iof = TestIoFactory::new();
    let amt = 100;
    let file_a = "./../file_a".to_owned();
    let file_b = "/foo/bar".to_owned();
    iof.possible_files.insert(file_a.clone(), amt);
    iof.server_present_files.insert(file_a.clone());
    iof.possible_files.insert(file_b.clone(), amt);
    iof.server_present_files.insert(file_b.clone());

    let proxy = IOPolicyProxy::new(
        iof,
        IOPolicyCfg {
            readonly: false,
            path: None,
        },
    );

    assert_matches!(
        proxy.open_read("./../file_a".as_ref()),
        Err(ref e) if e.kind() == io::ErrorKind::PermissionDenied
    );
    assert_matches!(
        proxy.open_read("/foo/bar".as_ref()),
        Err(ref e) if e.kind() == io::ErrorKind::PermissionDenied
    );
}

#[test]
fn policy_refuse_file_write_outside_cwd() {
    let mut iof = TestIoFactory::new();
    let amt = 100;

    let w_file_a = "./../w_file_a".to_owned();
    let w_file_b = "/boo/han".to_owned();
    iof.possible_files.insert(w_file_a.clone(), amt);
    iof.possible_files.insert(w_file_b.clone(), amt);

    let mut proxy = IOPolicyProxy::new(
        iof,
        IOPolicyCfg {
            readonly: false,
            path: None,
        },
    );

    assert_matches!(
        proxy.create_new("./../w_file_a".as_ref(), None),
        Err(ref e) if e.kind() == io::ErrorKind::PermissionDenied
    );
    assert_matches!(
        proxy.create_new("/boo/han".as_ref(), None),
        Err(ref e) if e.kind() == io::ErrorKind::PermissionDenied
    );
}

#[test]
fn option_timeout_rrq() {
    let (mut server, file, _) = rrq_fixture(1234);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Octet,
        options: vec![TftpOption::TimeoutSecs(4)],
    });
    assert_eq!(
        res,
        Ok(Packet::OACK {
            options: vec![TftpOption::TimeoutSecs(4)],
        })
    );
    let xfer = xfer.unwrap();
    assert_eq!(xfer.timeout(), Some(Duration::from_secs(4)));
}

#[test]
fn option_timeout_wrq() {
    let (mut server, file, _) = wrq_fixture_early_termination(1234);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: Octet,
        options: vec![TftpOption::TimeoutSecs(5)],
    });
    assert_eq!(
        res,
        Ok(Packet::OACK {
            options: vec![TftpOption::TimeoutSecs(5)],
        })
    );
    let xfer = xfer.unwrap();
    assert_eq!(xfer.timeout(), Some(Duration::from_secs(5)));
}

#[test]
fn option_tsize_rrq() {
    let (mut server, file, _) = rrq_fixture(1234);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Octet,
        options: vec![TftpOption::TransferSize(0)],
    });
    assert_eq!(
        res,
        Ok(Packet::OACK {
            options: vec![TftpOption::TransferSize(1234)],
        })
    );
    assert_matches!(xfer.as_ref().map(Transfer::is_done), Some(false));
}

#[test]
fn option_tsize_wrq() {
    // TODO: make test actually check that transfer size is passed down
    let (mut server, file, _) = wrq_fixture_early_termination(1234);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: Octet,
        options: vec![TftpOption::TransferSize(1234)],
    });
    assert_eq!(
        res,
        Ok(Packet::OACK {
            options: vec![TftpOption::TransferSize(1234)],
        })
    );
    assert_matches!(xfer.as_ref().map(Transfer::is_done), Some(false));
}

// TODO: maybe switch tests to use paths ?
struct TestIoFactory {
    server_present_files: HashSet<String>,
    possible_files: HashMap<String, usize>,
    enforce_full_write: bool,
}
impl TestIoFactory {
    fn new() -> Self {
        TestIoFactory {
            server_present_files: HashSet::new(),
            possible_files: HashMap::new(),
            enforce_full_write: true,
        }
    }
}
impl IOAdapter for TestIoFactory {
    type R = GeneratingReader;
    type W = ExpectingWriter;
    fn open_read(&self, file: &Path) -> io::Result<(Self::R, Option<u64>)> {
        let filename = file.to_str().expect("not a valid string");
        if self.server_present_files.contains(filename) {
            let size = *self.possible_files.get(filename).unwrap();
            Ok((GeneratingReader::new(filename, size), Some(size as u64)))
        } else if !self.possible_files.contains_key(filename) {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "unexpected file",
            ))
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "test file not found",
            ))
        }
    }
    fn create_new(&mut self, file: &Path, len: Option<u64>) -> io::Result<ExpectingWriter> {
        let filename = file.to_str().expect("not a valid string");
        if self.server_present_files.contains(filename) {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "test file already there",
            ))
        } else if !self.possible_files.contains_key(filename) {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "unexpected file",
            ))
        } else {
            self.server_present_files.insert(filename.into());
            let size = *self.possible_files.get(filename).unwrap();
            if let Some(l) = len {
                assert_eq!(l, size as u64, "given and expected sizes don't match");
            }
            Ok(ExpectingWriter::new(
                filename,
                size,
                self.enforce_full_write,
            ))
        }
    }
}

#[test]
fn rrq_timeout_repeat_end() {
    let (mut server, file, _) = rrq_fixture(512 * 3 + 123);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Octet,
        options: vec![],
    });
    assert_matches!(res, Ok(Packet::DATA { .. }));
    let mut xfer = xfer.unwrap();
    assert_eq!(xfer.timeout_expired(), Repeat);
    assert_eq!(xfer.timeout_expired(), Done(None));
    assert!(xfer.is_done());
}

#[test]
fn rrq_timeout_repeat_ack_repeat() {
    let (mut server, file, mut file_bytes) = rrq_fixture(512 * 3 + 123);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Octet,
        options: vec![],
    });
    assert_eq!(
        res,
        Ok(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        })
    );
    let mut xfer = xfer.unwrap();
    assert_eq!(xfer.timeout_expired(), Repeat);
    assert_eq!(
        xfer.rx(Packet::ACK(1)),
        Reply(Packet::DATA {
            block_num: 2,
            data: file_bytes.gen(512),
        })
    );
    assert_eq!(xfer.timeout_expired(), Repeat);
}

#[test]
fn wrq_timeout_repeat_ack_repeat() {
    let (mut server, file, mut file_bytes) = wrq_fixture_early_termination(512 * 3 + 123);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: Octet,
        options: vec![],
    });
    assert_eq!(res, Ok(Packet::ACK(0)));
    let mut xfer = xfer.unwrap();
    assert_eq!(xfer.timeout_expired(), Repeat);
    assert_eq!(
        xfer.rx(Packet::DATA {
            block_num: 1,
            data: file_bytes.gen(512),
        }),
        Reply(Packet::ACK(1))
    );
    assert_eq!(xfer.timeout_expired(), Repeat);
}

macro_rules! assert_packets {
    ( $e:expr => $pat:pat => $code:expr => [ $($value:expr,)* ] ) => {
        if let $pat = $e {
            $( assert_eq!($code, Some($value)); )*
            assert_eq!($code, None);
        } else {
            panic!("assertion failed: `{:?}` does not match `{} if {}`",
                $e, stringify!($pat), stringify!($cond))
        }
    };
}

#[test]
fn rrq_windowsize_2_ok() {
    let (mut server, file, mut file_bytes) = rrq_fixture(512 * 3 + 123 /*4 blocks*/);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Octet,
        options: vec![TftpOption::WindowSize(2)],
    });
    assert_eq!(
        res,
        Ok(Packet::OACK {
            options: vec![TftpOption::WindowSize(2)],
        })
    );
    let mut xfer = xfer.unwrap();

    assert_packets!(
        xfer.rx2(Packet::ACK(0)) => Ok(mut packs) => packs.next() => [
            ResponseItem::Packet(Packet::DATA { block_num: 1, data: file_bytes.gen(512), }),
            ResponseItem::Packet(Packet::DATA { block_num: 2, data: file_bytes.gen(512), }),
        ]
    );

    assert_packets!(
        xfer.rx2(Packet::ACK(2)) => Ok(mut packs) => packs.next() => [
            ResponseItem::Packet(Packet::DATA { block_num: 3, data: file_bytes.gen(512), }),
            ResponseItem::Packet(Packet::DATA { block_num: 4, data: file_bytes.gen(123), }),
        ]
    );

    assert_packets!(
        xfer.rx2(Packet::ACK(4)) => Ok(mut packs) => packs.next() => [
            ResponseItem::Done,
        ]
    );
    assert!(xfer.is_done());
}

#[test]
fn rrq_windowsize_2_ok_incomplete_window() {
    let (mut server, file, mut file_bytes) = rrq_fixture(123 /*1 block*/);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Octet,
        options: vec![TftpOption::WindowSize(2)],
    });
    assert_eq!(
        res,
        Ok(Packet::OACK {
            options: vec![TftpOption::WindowSize(2)],
        })
    );
    let mut xfer = xfer.unwrap();

    assert_packets!(
        xfer.rx2(Packet::ACK(0)) => Ok(mut packs) => packs.next() => [
            ResponseItem::Packet(Packet::DATA { block_num: 1, data: file_bytes.gen(123), }),
        ]
    );
    assert_packets!(
        xfer.rx2(Packet::ACK(1)) => Ok(mut packs) => packs.next() => [
            ResponseItem::Done,
        ]
    );
}

#[test]
fn rrq_windowsize_partial_resume() {
    let (mut server, file, mut file_bytes) = rrq_fixture(512 * 3 + 123 /*4 blocks*/);
    let (xfer, res) = server.rx_initial(Packet::RRQ {
        filename: file,
        mode: Octet,
        options: vec![TftpOption::WindowSize(3)],
    });
    assert_eq!(
        res,
        Ok(Packet::OACK {
            options: vec![TftpOption::WindowSize(3)],
        })
    );
    let mut xfer = xfer.unwrap();

    assert_packets!(
        xfer.rx2(Packet::ACK(0)) => Ok(mut packs) => packs.next() => [
            ResponseItem::Packet(Packet::DATA { block_num: 1, data: file_bytes.gen(512), }),
            ResponseItem::Packet(Packet::DATA { block_num: 2, data: file_bytes.gen(512), }),
            ResponseItem::Packet(Packet::DATA { block_num: 3, data: file_bytes.gen(512), }),
        ]
    );

    // assuming 2 and 3 got lost
    assert_packets!(
        xfer.rx2(Packet::ACK(1)) => Ok(mut packs) => packs.next() => [
            ResponseItem::RepeatLast(2),
            ResponseItem::Packet(Packet::DATA { block_num: 4, data: file_bytes.gen(123), }),
        ]
    );

    assert_packets!(
        xfer.rx2(Packet::ACK(4)) => Ok(mut packs) => packs.next() => [
            ResponseItem::Done,
        ]
    );
    assert!(xfer.is_done());
}

#[test]
fn wrq_windowsize_2_ok() {
    let (mut server, file, mut file_bytes) = wrq_fixture(512 * 3 + 123);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: Octet,
        options: vec![TftpOption::WindowSize(2)],
    });
    assert_eq!(
        res,
        Ok(Packet::OACK {
            options: vec![TftpOption::WindowSize(2)],
        })
    );
    let mut xfer = xfer.unwrap();

    assert_packets!(
        xfer.rx2(Packet::DATA { block_num: 1, data: file_bytes.gen(512), })
            => Ok(mut packs) => packs.next() => []
    );

    assert_packets!(
        xfer.rx2(Packet::DATA { block_num: 2, data: file_bytes.gen(512), })
            => Ok(mut packs) => packs.next() =>
        [
            ResponseItem::Packet(Packet::ACK(2)),
        ]
    );

    assert_packets!(
        xfer.rx2(Packet::DATA { block_num: 3, data: file_bytes.gen(512), })
            => Ok(mut packs) => packs.next() => []
    );
    assert_packets!(
        xfer.rx2(Packet::DATA { block_num: 4, data: file_bytes.gen(123), })
            => Ok(mut packs) => packs.next() =>
        [
            ResponseItem::Packet(Packet::ACK(4)),
            ResponseItem::Done,
        ]
    );
}

#[test]
fn wrq_windowsize_2_ok_incomplete_window() {
    let (mut server, file, mut file_bytes) = wrq_fixture(123);
    let (xfer, res) = server.rx_initial(Packet::WRQ {
        filename: file,
        mode: Octet,
        options: vec![TftpOption::WindowSize(2)],
    });
    assert_eq!(
        res,
        Ok(Packet::OACK {
            options: vec![TftpOption::WindowSize(2)],
        })
    );
    let mut xfer = xfer.unwrap();

    assert_packets!(
        xfer.rx2(Packet::DATA { block_num: 1, data: file_bytes.gen(123), })
            => Ok(mut packs) => packs.next() =>
        [
            ResponseItem::Packet(Packet::ACK(1)),
            ResponseItem::Done,
        ]
    );
}

#[derive(Debug)]
struct ByteGen {
    crt: u8,
    count: u8,
}
impl ByteGen {
    // pseudorandom, just so we get different values for each file
    fn new(s: &str) -> Self {
        let mut v = 0;
        for b in s.bytes() {
            v ^= b;
        }
        ByteGen { crt: v, count: 0 }
    }
    fn gen(&mut self, n: usize) -> Vec<u8> {
        self.take(n).collect()
    }
}
impl Iterator for ByteGen {
    type Item = u8;
    fn next(&mut self) -> Option<u8> {
        self.crt = self.crt.wrapping_add(1).wrapping_mul(self.count);
        self.count = self.count.wrapping_add(1);
        Some(self.crt)
    }
}

#[derive(Debug)]
struct GeneratingReader {
    gen: Take<ByteGen>,
}
impl GeneratingReader {
    fn new(s: &str, amt: usize) -> Self {
        Self {
            gen: ByteGen::new(s).take(amt),
        }
    }
}
impl Read for GeneratingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // TODO: write this more concisely
        let mut read = 0;
        for e in buf {
            if let Some(v) = self.gen.next() {
                *e = v;
                read += 1;
            } else {
                break;
            }
        }
        Ok(read)
    }
}

#[derive(Debug)]
struct ExpectingWriter {
    gen: Take<ByteGen>,
    enforce_full_write: bool,
}
impl ExpectingWriter {
    fn new(s: &str, amt: usize, enforce_full_write: bool) -> Self {
        Self {
            gen: ByteGen::new(s).take(amt),
            enforce_full_write,
        }
    }
}
impl Write for ExpectingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // TODO: write this more concisely
        let mut wrote = 0;
        for b in buf {
            if let Some(v) = self.gen.next() {
                assert_eq!(*b, v);
                wrote += 1;
            } else {
                panic!("wrote more than expected");
            }
        }
        Ok(wrote)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Drop for ExpectingWriter {
    fn drop(&mut self) {
        if self.enforce_full_write && !::std::thread::panicking() {
            let (_, sup) = self.gen.size_hint();
            assert_eq!(
                sup,
                Some(0),
                "writer destroyed before all bytes were written"
            );
        }
    }
}
