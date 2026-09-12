//! Byte-preserving output construction with infallible UTF-8 text appends.

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub(crate) struct ByteBuffer(Vec<u8>);

impl ByteBuffer {
    pub(crate) fn new() -> Self {
        Self::default()
    }
    pub(crate) fn push(&mut self, value: char) {
        self.0
            .extend_from_slice(value.encode_utf8(&mut [0; 4]).as_bytes());
    }
    pub(crate) fn push_str(&mut self, value: &str) {
        self.0.extend_from_slice(value.as_bytes());
    }
    pub(crate) fn push_bytes(&mut self, value: &[u8]) {
        self.0.extend_from_slice(value);
    }
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Write for ByteBuffer {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.push_str(value);
        Ok(())
    }
}

impl AsRef<[u8]> for ByteBuffer {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl PartialEq<&str> for ByteBuffer {
    fn eq(&self, other: &&str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}
