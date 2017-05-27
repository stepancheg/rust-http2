use std::mem;

use bytes::*;

pub fn bytes_extend_with_slice(target: &mut Bytes, append: &[u8]) {
    let mut bytes_mut = BytesMut::from(mem::replace(target, Bytes::new()));
    bytes_mut_extend_with_slice(&mut bytes_mut, append);
    mem::replace(target, bytes_mut.freeze());
}

pub fn bytes_extend_with(target: &mut Bytes, append: Bytes) {
    // TODO: optimize
    bytes_extend_with_slice(target, &append[..]);
}

// https://github.com/carllerche/bytes/pull/112
pub fn bytes_mut_extend_with_slice(target: &mut BytesMut, append: &[u8]) {
    target.reserve(append.len());
    target.put_slice(append);
}

