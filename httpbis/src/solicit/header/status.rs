use bytes::Bytes;

use crate::HeaderValue;

pub const STATUS_200: HeaderValue =
    unsafe { HeaderValue::from_bytes_unchecked(Bytes::from_static(b"200")) };
pub const STATUS_404: HeaderValue =
    unsafe { HeaderValue::from_bytes_unchecked(Bytes::from_static(b"404")) };
pub const STATUS_500: HeaderValue =
    unsafe { HeaderValue::from_bytes_unchecked(Bytes::from_static(b"500")) };

pub fn status_to_header_value(code: u32) -> HeaderValue {
    match code {
        200 => STATUS_200,
        404 => STATUS_404,
        500 => STATUS_500,
        s => HeaderValue::from_bytes(Bytes::from(format!("{}", s)))
            .map_err(|(_, e)| e)
            .unwrap(),
    }
}
