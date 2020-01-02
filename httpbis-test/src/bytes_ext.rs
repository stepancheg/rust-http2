use bytes::Bytes;
use bytes::BytesMut;

pub trait BytesExt {
    fn extend_from_slice(&mut self, slice: &[u8]);
}

impl BytesExt for Bytes {
    fn extend_from_slice(&mut self, slice: &[u8]) {
        let mut bytes = BytesMut::new();
        bytes.extend_from_slice(self.as_ref());
        bytes.extend_from_slice(slice);
        *self = bytes.freeze();
    }
}
