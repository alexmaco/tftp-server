use std::io::{self, Write};

pub const MAX_BLOCKSIZE: u16 = 65_464;

/// A TFTP protocol option, proposed via RRQ/WRQ and acknowledged via OACK
#[derive(PartialEq, Clone, Debug)]
pub enum TftpOption {
    Blocksize(u16),
    TransferSize(u64),
    TimeoutSecs(u8),
    WindowSize(u16),
}

impl TftpOption {
    pub fn write_to(&self, buf: &mut Write) -> io::Result<()> {
        use self::TftpOption::*;
        match *self {
            Blocksize(size) => {
                write!(buf, "blksize\0{}\0", size)?;
            }
            TransferSize(size) => {
                write!(buf, "tsize\0{}\0", size)?;
            }
            TimeoutSecs(t) => {
                write!(buf, "timeout\0{}\0", t)?;
            }
            WindowSize(t) => {
                write!(buf, "windowsize\0{}\0", t)?;
            }
        };
        Ok(())
    }

    pub fn try_from(name: &str, value: &str) -> Option<Self> {
        if "blksize".eq_ignore_ascii_case(name) {
            let val = value.parse::<u16>().ok()?;
            if val >= 8 && val <= MAX_BLOCKSIZE {
                return Some(TftpOption::Blocksize(val));
            }
        } else if "timeout".eq_ignore_ascii_case(name) {
            let val = value.parse().ok()?;
            if val > 0 {
                return Some(TftpOption::TimeoutSecs(val));
            }
        } else if "tsize".eq_ignore_ascii_case(name) {
            let val = value.parse().ok()?;
            return Some(TftpOption::TransferSize(val));
        } else if "windowsize".eq_ignore_ascii_case(name) {
            let val = value.parse().ok()?;
            if val > 0 {
                return Some(TftpOption::WindowSize(val));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocksize_parse() {
        assert_eq!(
            TftpOption::try_from("blksize", "512"),
            Some(TftpOption::Blocksize(512))
        );
        assert_eq!(
            TftpOption::try_from("bLkSIzE", "512"),
            Some(TftpOption::Blocksize(512))
        );
        assert_eq!(TftpOption::try_from("blksize", "cat"), None);
        assert_eq!(TftpOption::try_from("blocksize", "512"), None);
    }

    #[test]
    fn blocksize_bounds() {
        assert_eq!(TftpOption::try_from("blksize", "7"), None);
        assert_eq!(
            TftpOption::try_from("blksize", "8"),
            Some(TftpOption::Blocksize(8))
        );
        assert_eq!(MAX_BLOCKSIZE, 65_464);
        assert_eq!(
            TftpOption::try_from("blksize", "65464"),
            Some(TftpOption::Blocksize(65_464))
        );
        assert_eq!(TftpOption::try_from("blksize", "65465"), None);
    }

    #[test]
    fn blocksize_write() {
        let mut v = vec![];
        TftpOption::Blocksize(78).write_to(&mut v).unwrap();
        assert_eq!(v, b"blksize\078\0");
    }

    #[test]
    fn transfer_size_parse() {
        assert_eq!(
            TftpOption::try_from("tsize", "56246"),
            Some(TftpOption::TransferSize(56246))
        );
        assert_eq!(
            TftpOption::try_from("tSiZE", "0"),
            Some(TftpOption::TransferSize(0))
        );
    }

    #[test]
    fn transfer_size_write() {
        let mut v = vec![];
        TftpOption::TransferSize(54).write_to(&mut v).unwrap();
        assert_eq!(v, b"tsize\054\0");
    }

    #[test]
    fn timeout_parse() {
        assert_eq!(
            TftpOption::try_from("timeout", "8"),
            Some(TftpOption::TimeoutSecs(8))
        );
        assert_eq!(
            TftpOption::try_from("TIMEOUT", "3"),
            Some(TftpOption::TimeoutSecs(3))
        );
    }

    #[test]
    fn timeout_bounds() {
        assert_eq!(
            TftpOption::try_from("timeout", "255"),
            Some(TftpOption::TimeoutSecs(255))
        );
        assert_eq!(TftpOption::try_from("TIMEOUT", "256"), None);
        assert_eq!(
            TftpOption::try_from("timeout", "1"),
            Some(TftpOption::TimeoutSecs(1))
        );
        assert_eq!(TftpOption::try_from("TIMEOUT", "0"), None);
    }

    #[test]
    fn timeout_write() {
        let mut v = vec![];
        TftpOption::TimeoutSecs(4).write_to(&mut v).unwrap();
        assert_eq!(v, b"timeout\04\0");
    }

    #[test]
    fn windowsize_parse() {
        assert_eq!(
            TftpOption::try_from("windowsize", "8"),
            Some(TftpOption::WindowSize(8))
        );
        assert_eq!(
            TftpOption::try_from("WINDOWSIZE", "3"),
            Some(TftpOption::WindowSize(3))
        );
    }

    #[test]
    fn windowsize_bounds() {
        assert_eq!(
            TftpOption::try_from("windowsize", "65535"),
            Some(TftpOption::WindowSize(65_535))
        );
        assert_eq!(TftpOption::try_from("WINDOWSIZE", "65536"), None);
        assert_eq!(
            TftpOption::try_from("windowsize", "1"),
            Some(TftpOption::WindowSize(1))
        );
        assert_eq!(TftpOption::try_from("WINDOWSIZE", "0"), None);
    }

    #[test]
    fn windowsize_write() {
        let mut v = vec![];
        TftpOption::WindowSize(4).write_to(&mut v).unwrap();
        assert_eq!(v, b"windowsize\04\0");
    }
}
