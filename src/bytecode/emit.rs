use alloc::vec::Vec;

/// A section's id, the first byte of the section (`bytecode::Section::ID`)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Section {
    String = 0,
    Dialect = 1,
    AttrType = 2,
    AttrTypeOffset = 3,
    Ir = 4,
    Resource = 5,
    ResourceOffset = 6,
    Properties = 8,
}

/// Bytes being written, with the alignment their start needs (`EncodingEmitter`)
#[derive(Default)]
pub(super) struct Emitter {
    pub bytes: Vec<u8>,
    align: u64,
}

impl Emitter {
    /// What pads a section or a blob to its alignment
    const PADDING: u8 = 0xCB;
    /// The high bit of a section's id: an alignment follows its length
    const ALIGNED: u8 = 0x80;
    /// The value bits a one-byte varint holds
    const SHORT_BITS: u32 = 7;
    /// The most bytes a prefix varint takes before it falls back to a marker and eight full bytes
    const LONG_BYTES: usize = 9;

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn byte(&mut self, b: u8) {
        self.bytes.push(b);
    }

    pub fn raw(&mut self, b: &[u8]) {
        self.bytes.extend_from_slice(b);
    }

    /// A prefix varint: the low bits of the first byte count the bytes that follow
    pub fn var(&mut self, v: u64) {
        if v >> Self::SHORT_BITS == 0 {
            return self.byte(((v << 1) | 1) as u8);
        }
        for n in 2..Self::LONG_BYTES {
            if v >> (Self::SHORT_BITS * n as u32) == 0 {
                let enc = ((v << 1) | 1) << (n - 1);
                return self.raw(&enc.to_le_bytes()[..n]);
            }
        }
        self.byte(0);
        self.raw(&v.to_le_bytes());
    }

    /// A zigzag varint
    pub fn svar(&mut self, v: i64) {
        self.var(((v as u64) << 1) ^ ((v >> 63) as u64));
    }

    pub fn var_flag(&mut self, v: u64, flag: bool) {
        self.var((v << 1) | u64::from(flag));
    }

    pub fn cstr(&mut self, s: &[u8]) {
        self.raw(s);
        self.byte(0);
    }

    pub fn align_to(&mut self, align: u64) {
        if align < 2 {
            return;
        }
        while self.bytes.len() as u64 % align != 0 {
            self.byte(Self::PADDING);
        }
        self.align = self.align.max(align);
    }

    /// `inner` as a section of id `id`
    pub fn section(&mut self, id: Section, inner: Emitter) {
        let at = self.bytes.len();
        self.byte(id as u8);
        self.var(inner.bytes.len() as u64);
        if inner.align > 1 {
            if self.bytes.len() as u64 % inner.align != 0 {
                self.var(inner.align);
                self.align_to(inner.align);
                self.bytes[at] |= Self::ALIGNED;
            } else {
                self.align = self.align.max(inner.align);
            }
        }
        self.raw(&inner.bytes);
    }
}
