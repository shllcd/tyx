use std::fmt;

const MAX_VARINT_BYTES: usize = 10;

#[inline]
fn pack_varint_groups(groups: u64) -> u64 {
    let pairs = (groups & 0x7f00_7f00_7f00_7f00) >> 1 | groups & 0x007f_007f_007f_007f;
    let quads = (pairs & 0x3fff_0000_3fff_0000) >> 2 | pairs & 0x0000_3fff_0000_3fff;
    (quads & 0x0fff_ffff_0000_0000) >> 4 | quads & 0x0000_0000_0fff_ffff
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Eof,
    VarintOverflow,
    InvalidBool(u8),
    InvalidUtf8,
    InvalidTimestamp,
    UnknownVariant { ty: &'static str, index: u32 },
    UnexpectedPayload { ty: &'static str, index: u32 },
    MissingPayload { ty: &'static str, index: u32 },
    TrailingBytes,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eof => f.write_str("unexpected end of input"),
            Self::VarintOverflow => f.write_str("varint does not fit its type"),
            Self::InvalidBool(byte) => write!(f, "invalid bool byte {byte:#04x}"),
            Self::InvalidUtf8 => f.write_str("invalid utf-8 in string"),
            Self::InvalidTimestamp => f.write_str("timestamp out of range"),
            Self::UnknownVariant { ty, index } => write!(f, "unknown variant {index} of `{ty}`"),
            Self::UnexpectedPayload { ty, index } => write!(f, "unit variant {index} of `{ty}` carries a payload"),
            Self::MissingPayload { ty, index } => write!(f, "variant {index} of `{ty}` lacks its payload"),
            Self::TrailingBytes => f.write_str("trailing bytes after the value"),
        }
    }
}

impl std::error::Error for Error {}

impl From<Vec<u8>> for Writer {
    fn from(buf: Vec<u8>) -> Self {
        Self { buf }
    }
}

#[derive(Debug, Default)]
pub struct Writer {
    buf: Vec<u8>,
}

#[must_use = "a frame must be closed with Writer::end"]
pub struct Frame(usize);

impl Writer {
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(256)
    }

    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self { buf: Vec::with_capacity(capacity) }
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }

    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }

    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }

    #[inline]
    pub fn byte(&mut self, byte: u8) {
        self.buf.push(byte);
    }

    #[inline]
    pub fn bytes(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    #[inline]
    pub fn varint(&mut self, mut value: u64) {
        while value >= 0x80 {
            self.buf.push(value as u8 | 0x80);
            value >>= 7;
        }
        self.buf.push(value as u8);
    }

    #[inline]
    pub fn zigzag(&mut self, value: i64) {
        self.varint(((value << 1) ^ (value >> 63)) as u64);
    }

    #[inline]
    pub fn len_prefixed(&mut self, bytes: &[u8]) {
        self.varint(bytes.len() as u64);
        self.bytes(bytes);
    }

    #[inline]
    pub fn tag(&mut self, index: u32, has_payload: bool) {
        self.varint(u64::from(index) << 1 | u64::from(has_payload));
    }

    #[inline]
    pub fn begin(&mut self, mask: u64) -> Frame {
        let start = self.buf.len();
        self.buf.push(u8::from(mask != 0));
        if mask != 0 {
            self.varint(mask);
        }
        Frame(start)
    }

    #[inline]
    pub fn end(&mut self, Frame(start): Frame) {
        let len = (self.buf.len() - start - 1) as u64;
        let header = len << 1 | u64::from(self.buf[start]);
        if header < 0x80 {
            self.buf[start] = header as u8;
        } else {
            self.widen_header(start, header);
        }
    }

    #[cold]
    fn widen_header(&mut self, start: usize, mut header: u64) {
        let mut encoded = [0u8; MAX_VARINT_BYTES];
        let mut width = 0;
        while header >= 0x80 {
            encoded[width] = header as u8 | 0x80;
            header >>= 7;
            width += 1;
        }
        encoded[width] = header as u8;
        self.buf[start] = encoded[0];
        self.buf.splice(start + 1..start + 1, encoded[1..=width].iter().copied());
    }
}

#[derive(Debug, Clone)]
pub struct Reader<'de> {
    buf: &'de [u8],
}

impl<'de> Reader<'de> {
    #[must_use]
    pub fn new(buf: &'de [u8]) -> Self {
        Self { buf }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    #[must_use]
    pub fn remaining(&self) -> usize {
        self.buf.len()
    }

    #[inline]
    pub fn byte(&mut self) -> Result<u8, Error> {
        let (&byte, rest) = self.buf.split_first().ok_or(Error::Eof)?;
        self.buf = rest;
        Ok(byte)
    }

    #[inline]
    pub fn take(&mut self, len: usize) -> Result<&'de [u8], Error> {
        let (head, rest) = self.buf.split_at_checked(len).ok_or(Error::Eof)?;
        self.buf = rest;
        Ok(head)
    }

    #[inline]
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let (head, rest) = self.buf.split_first_chunk::<N>().ok_or(Error::Eof)?;
        self.buf = rest;
        Ok(*head)
    }

    #[inline]
    pub fn varint(&mut self) -> Result<u64, Error> {
        if let Some((&byte, rest)) = self.buf.split_first()
            && byte < 0x80
        {
            self.buf = rest;
            return Ok(u64::from(byte));
        }
        self.varint_multi_byte()
    }

    #[inline(never)]
    fn varint_multi_byte(&mut self) -> Result<u64, Error> {
        if let Some(chunk) = self.buf.first_chunk::<8>() {
            let word = u64::from_le_bytes(*chunk);
            let stops = !word & 0x8080_8080_8080_8080;
            if stops != 0 {
                let len = stops.trailing_zeros() as usize / 8 + 1;
                self.buf = &self.buf[len..];
                return Ok(pack_varint_groups(word & (stops ^ (stops - 1)) & 0x7f7f_7f7f_7f7f_7f7f));
            }
        }
        let mut value = 0u64;
        for shift in (0..MAX_VARINT_BYTES as u32 * 7).step_by(7) {
            let byte = self.byte()?;
            let bits = u64::from(byte & 0x7f);
            if shift == 63 && bits > 1 {
                return Err(Error::VarintOverflow);
            }
            value |= bits << shift;
            if byte < 0x80 {
                return Ok(value);
            }
        }
        Err(Error::VarintOverflow)
    }

    #[inline]
    pub fn varint_u32(&mut self) -> Result<u32, Error> {
        u32::try_from(self.varint()?).map_err(|_| Error::VarintOverflow)
    }

    #[inline]
    pub fn length(&mut self) -> Result<usize, Error> {
        let len = usize::try_from(self.varint()?).map_err(|_| Error::VarintOverflow)?;
        if len > self.buf.len() {
            return Err(Error::Eof);
        }
        Ok(len)
    }

    #[inline]
    pub fn count(&mut self) -> Result<usize, Error> {
        usize::try_from(self.varint()?).map_err(|_| Error::VarintOverflow)
    }

    #[inline]
    pub fn zigzag(&mut self) -> Result<i64, Error> {
        let raw = self.varint()?;
        Ok((raw >> 1) as i64 ^ -((raw & 1) as i64))
    }

    #[inline]
    pub fn len_prefixed(&mut self) -> Result<&'de [u8], Error> {
        let len = self.length()?;
        self.take(len)
    }

    #[inline]
    pub fn tag(&mut self) -> Result<(u32, bool), Error> {
        let raw = self.varint()?;
        let index = u32::try_from(raw >> 1).map_err(|_| Error::VarintOverflow)?;
        Ok((index, raw & 1 == 1))
    }

    /// Splits off the next frame and returns its body reader with the presence mask.
    #[inline]
    pub fn frame(&mut self) -> Result<(Self, u64), Error> {
        let header = self.varint()?;
        let len = usize::try_from(header >> 1).map_err(|_| Error::VarintOverflow)?;
        let mut body = Self::new(self.take(len)?);
        let mask = if header & 1 == 1 { body.varint()? } else { 0 };
        Ok((body, mask))
    }

    pub fn skip_frame(&mut self) -> Result<(), Error> {
        let header = self.varint()?;
        let len = usize::try_from(header >> 1).map_err(|_| Error::VarintOverflow)?;
        self.take(len).map(drop)
    }
}
