#![cfg(test)]

use mio::*;
use std::borrow::BorrowMut;
use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::iter::Take;
use packet::{ErrorCode, Packet};
use server::IOAdapter;
use tftp_proto::*;

#[test]
fn no_answer_ack_no_xfer() {
    let iof = TestIoFactory::new();
    let mut serv = TftpServerProto::new(iof);
    assert_eq!(
        serv.recv(Token(1), Packet::ACK(0)),
        TftpResult::Err(TftpError::InvalidTransferToken)
    );
}

#[test]
fn rrq_no_file_gets_error() {
    let iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    let mut serv = TftpServerProto::new(iof);
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::RRQ {
                filename: file,
                mode: "octet".to_owned(),
            },
        ),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::FileNotFound,
            msg: "".to_owned(),
        }))
    );
}

#[test]
fn rrq_mail_gets_error() {
    let iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    let mut serv = TftpServerProto::new(iof);
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::RRQ {
                filename: file,
                mode: "mail".to_owned(),
            },
        ),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::NoUser,
            msg: "".to_owned(),
        }))
    );
}

#[test]
fn rrq_small_file_ack_end() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.files.insert(file.clone(), 132);
    iof.server_files.insert(file.clone());
    let mut serv = TftpServerProto::new(iof);
    let byte_gen = ByteGen::new(&file);
    assert_eq!(
        serv.recv(
            Token(2),
            Packet::RRQ {
                filename: file.clone(),
                mode: "octet".to_owned(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.take(132).collect(),
        })
    );
    assert_eq!(serv.recv(Token(2), Packet::ACK(1)), TftpResult::Done(None));
    assert_eq!(
        serv.recv(Token(2), Packet::ACK(0)),
        TftpResult::Err(TftpError::InvalidTransferToken)
    );
}

#[test]
fn rrq_1_block_file() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.files.insert(file.clone(), 512);
    iof.server_files.insert(file.clone());
    let mut serv = TftpServerProto::new(iof);
    let byte_gen = ByteGen::new(&file);
    assert_eq!(
        serv.recv(
            Token(2),
            Packet::RRQ {
                filename: file.clone(),
                mode: "octet".to_owned(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.take(512).collect(),
        })
    );
    assert_eq!(
        serv.recv(Token(2), Packet::ACK(1)),
        TftpResult::Reply(Packet::DATA {
            block_num: 2,
            data: Vec::new(),
        })
    );
    assert_eq!(serv.recv(Token(2), Packet::ACK(2)), TftpResult::Done(None));
}

#[test]
fn rrq_small_file_ack_timeout_err() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.files.insert(file.clone(), 132);
    iof.server_files.insert(file.clone());
    let mut serv = TftpServerProto::new(iof);
    let byte_gen = ByteGen::new(&file);
    assert_eq!(
        serv.recv(
            Token(2),
            Packet::RRQ {
                filename: file.clone(),
                mode: "octet".to_owned(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.take(132).collect(),
        })
    );
    serv.timeout(Token(2));
    assert_eq!(
        serv.recv(Token(2), Packet::ACK(1)),
        TftpResult::Err(TftpError::InvalidTransferToken)
    );
}

#[test]
fn rrq_small_file_ack_wrong_block() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.files.insert(file.clone(), 132);
    iof.server_files.insert(file.clone());
    let mut serv = TftpServerProto::new(iof);
    let byte_gen = ByteGen::new(&file);
    assert_eq!(
        serv.recv(
            Token(2),
            Packet::RRQ {
                filename: file.clone(),
                mode: "octet".to_owned(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.take(132).collect(),
        })
    );
    assert_eq!(
        serv.recv(Token(2), Packet::ACK(2)),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::UnknownID,
            msg: "Incorrect block num in ACK".to_owned(),
        }))
    );
    assert_eq!(
        serv.recv(Token(2), Packet::ACK(0)),
        TftpResult::Err(TftpError::InvalidTransferToken)
    );
}

#[test]
fn rrq_small_file_reply_with_data_illegal() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.files.insert(file.clone(), 132);
    iof.server_files.insert(file.clone());
    let mut serv = TftpServerProto::new(iof);
    let byte_gen = ByteGen::new(&file);
    assert_eq!(
        serv.recv(
            Token(2),
            Packet::RRQ {
                filename: file.clone(),
                mode: "octet".to_owned(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.take(132).collect(),
        })
    );
    assert_eq!(
        serv.recv(
            Token(2),
            Packet::DATA {
                data: Vec::new(),
                block_num: 1,
            },
        ),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::IllegalTFTP,
            msg: "".to_owned(),
        }))
    );
    assert_eq!(
        serv.recv(Token(2), Packet::ACK(0)),
        TftpResult::Err(TftpError::InvalidTransferToken)
    );
}

#[test]
fn double_rrq() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.files.insert(file.clone(), 132);
    iof.server_files.insert(file.clone());
    let mut serv = TftpServerProto::new(iof);
    let byte_gen = ByteGen::new(&file);
    assert_eq!(
        serv.recv(
            Token(2),
            Packet::RRQ {
                filename: file.clone(),
                mode: "octet".to_owned(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.take(132).collect(),
        })
    );
    assert_eq!(
        serv.recv(
            Token(2),
            Packet::RRQ {
                filename: file.clone(),
                mode: "octet".to_owned(),
            },
        ),
        TftpResult::Err(TftpError::TransferAlreadyRunning)
    );
}

#[test]
fn rrq_2_blocks_ok() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.files.insert(file.clone(), 612);
    iof.server_files.insert(file.clone());
    let mut serv = TftpServerProto::new(iof);
    let mut byte_gen = ByteGen::new(&file);
    assert_eq!(
        serv.recv(
            Token(5),
            Packet::RRQ {
                filename: file.clone(),
                mode: "octet".to_owned(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.borrow_mut().take(512).collect(),
        })
    );
    assert_eq!(
        serv.recv(Token(5), Packet::ACK(1)),
        TftpResult::Reply(Packet::DATA {
            block_num: 2,
            data: byte_gen.take(100).collect(),
        })
    );
    assert_eq!(serv.recv(Token(5), Packet::ACK(2)), TftpResult::Done(None));
}

#[test]
fn rrq_2_blocks_second_lost_ack_repeat_ok() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.files.insert(file.clone(), 612);
    iof.server_files.insert(file.clone());
    let mut serv = TftpServerProto::new(iof);
    let mut byte_gen = ByteGen::new(&file);
    assert_eq!(
        serv.recv(
            Token(5),
            Packet::RRQ {
                filename: file.clone(),
                mode: "octet".to_owned(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.borrow_mut().take(512).collect(),
        })
    );
    assert_eq!(
        serv.recv(Token(5), Packet::ACK(1)),
        TftpResult::Reply(Packet::DATA {
            block_num: 2,
            data: byte_gen.take(100).collect(),
        })
    );
    // assuming the second data got lost, and the client re-acks the first data
    assert_eq!(serv.recv(Token(5), Packet::ACK(1)), TftpResult::Repeat);
    assert_eq!(serv.recv(Token(5), Packet::ACK(2)), TftpResult::Done(None));
}

#[test]
fn rrq_large_file_blocknum_wraparound() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    let size_bytes = 512 * 70_000 + 85;
    iof.files.insert(file.clone(), size_bytes);
    iof.server_files.insert(file.clone());
    let mut serv = TftpServerProto::new(iof);
    let mut byte_gen = ByteGen::new(&file);
    assert_eq!(
        serv.recv(
            Token(5),
            Packet::RRQ {
                filename: file.clone(),
                mode: "octet".to_owned(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.borrow_mut().take(512).collect(),
        })
    );

    let mut block = 1u16;
    for _ in 0..(size_bytes / 512) - 1
    //one less because we already got the first DATA above
    {
        let new_block = block.wrapping_add(1);
        assert_eq!(
            serv.recv(Token(5), Packet::ACK(block)),
            TftpResult::Reply(Packet::DATA {
                block_num: new_block,
                data: byte_gen.borrow_mut().take(512).collect(),
            })
        );
        block = new_block;
    }

    let new_block = block.wrapping_add(1);
    assert_eq!(
        serv.recv(Token(5), Packet::ACK(block)),
        TftpResult::Reply(Packet::DATA {
            block_num: new_block,
            data: byte_gen.take(85).collect(),
        })
    );
    assert_eq!(
        serv.recv(Token(5), Packet::ACK(new_block)),
        TftpResult::Done(None)
    );
}

#[test]
fn rrq_small_file_wrq_already_running() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.files.insert(file.clone(), 132);
    iof.server_files.insert(file.clone());
    let mut serv = TftpServerProto::new(iof);
    let byte_gen = ByteGen::new(&file);
    assert_eq!(
        serv.recv(
            Token(2),
            Packet::RRQ {
                filename: file.clone(),
                mode: "octet".to_owned(),
            },
        ),
        TftpResult::Reply(Packet::DATA {
            block_num: 1,
            data: byte_gen.take(132).collect(),
        })
    );
    assert_eq!(
        serv.recv(
            Token(2),
            Packet::WRQ {
                filename: file.clone(),
                mode: "octet".to_owned(),
            },
        ),
        TftpResult::Err(TftpError::TransferAlreadyRunning)
    );
}

#[test]
fn wrq_already_exists_error() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.files.insert(file.clone(), 132);
    iof.server_files.insert(file.clone());
    let mut serv = TftpServerProto::new(iof);
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::WRQ {
                filename: file,
                mode: "octet".to_owned(),
            },
        ),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::FileExists,
            msg: "".to_owned(),
        }))
    );
}

#[test]
fn wrq_mail_gets_error() {
    let iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    let mut serv = TftpServerProto::new(iof);
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::WRQ {
                filename: file,
                mode: "mail".to_owned(),
            },
        ),
        TftpResult::Done(Some(Packet::ERROR {
            code: ErrorCode::NoUser,
            msg: "".to_owned(),
        }))
    );
}

#[test]
fn wrq_small_file_ack_end() {
    let mut iof = TestIoFactory::new();
    let file = "textfile".to_owned();
    iof.files.insert(file.clone(), 132);
    let mut serv = TftpServerProto::new(iof);
    let byte_gen = ByteGen::new(&file);
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::WRQ {
                filename: file,
                mode: "octet".to_owned(),
            },
        ),
        TftpResult::Reply(Packet::ACK(0))
    );
    assert_eq!(
        serv.recv(
            Token(1),
            Packet::DATA {
                block_num: 1,
                data: byte_gen.take(132).collect(),
            },
        ),
        TftpResult::Done(Some(Packet::ACK(1)))
    );
}

struct TestIoFactory {
    server_files: HashSet<String>,
    files: HashMap<String, usize>,
}
impl TestIoFactory {
    fn new() -> Self {
        TestIoFactory {
            server_files: HashSet::new(),
            files: HashMap::new(),
        }
    }
}
impl IOAdapter for TestIoFactory {
    type R = GeneratingReader;
    type W = ExpectingWriter;
    fn open_read(&self, filename: String) -> io::Result<Self::R> {
        if self.server_files.contains(&filename) {
            let size = *self.files.get(&filename).unwrap();
            Ok(GeneratingReader::new(&filename, size))
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "test file not found",
            ))
        }
    }
    fn create_new(&mut self, filename: String) -> io::Result<ExpectingWriter> {
        if self.server_files.contains(&filename) {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "test file already there",
            ))
        } else {
            self.server_files.insert(filename.clone());
            let size = *self.files.get(&filename).unwrap();
            Ok(ExpectingWriter::new(&filename, size))
        }
    }
}

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
}
impl Iterator for ByteGen {
    type Item = u8;
    fn next(&mut self) -> Option<u8> {
        self.crt = self.crt.wrapping_add(1).wrapping_mul(self.count);
        self.count = self.count.wrapping_add(1);
        Some(self.crt)
    }
}

struct GeneratingReader {
    gen: Take<ByteGen>,
}
impl GeneratingReader {
    fn new(s: &str, amt: usize) -> Self {
        Self { gen: ByteGen::new(s).take(amt) }
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

struct ExpectingWriter {
    gen: Take<ByteGen>,
}
impl ExpectingWriter {
    fn new(s: &str, amt: usize) -> Self {
        Self { gen: ByteGen::new(s).take(amt) }
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
        let (_, sup) = self.gen.size_hint();
        assert_eq!(sup, Some(0), "writer destroyed before all bytes were written");
    }
}
