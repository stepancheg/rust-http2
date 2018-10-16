//! Implements all functionality related to encoding header blocks using
//! HPACK.
//!
//! Clients should use the `Encoder` struct as the API for performing HPACK
//! encoding.
use std::io;
use std::num::Wrapping;

use bytes::Bytes;

use super::HeaderTable;
use hpack::static_table::StaticTable;
use hpack::HeaderValueFound;

/// Encode an integer to the representation defined by HPACK, writing it into the provider
/// `io::Write` instance. Also allows the caller to specify the leading bits of the first
/// octet. Any bits that are already set within the last `prefix_size` bits will be cleared
/// and overwritten by the integer's representation (in other words, only the first
/// `8 - prefix_size` bits from the `leading_bits` octet are reflected in the first octet
/// emitted by the function.
pub fn encode_integer_into<W: io::Write>(
    mut value: usize,
    prefix_size: u8,
    leading_bits: u8,
    writer: &mut W,
) -> io::Result<()> {
    let Wrapping(mask) = if prefix_size >= 8 {
        Wrapping(0xFF)
    } else {
        Wrapping(1u8 << prefix_size) - Wrapping(1)
    };
    // Clear any bits within the last `prefix_size` bits of the provided `leading_bits`.
    // Failing to do so might lead to an incorrect encoding of the integer.
    let leading_bits = leading_bits & (!mask);
    let mask = mask as usize;
    if value < mask {
        writer.write_all(&[leading_bits | value as u8])?;
        return Ok(());
    }

    writer.write_all(&[leading_bits | mask as u8])?;
    value -= mask;
    while value >= 128 {
        writer.write_all(&[((value % 128) + 128) as u8])?;
        value = value / 128;
    }
    writer.write_all(&[value as u8])?;
    Ok(())
}

/// Encode an integer to the representation defined by HPACK.
///
/// Returns a newly allocated `Vec` containing the encoded bytes.
/// Only `prefix_size` lowest-order bits of the first byte in the
/// array are guaranteed to be used.
#[cfg(test)]
pub fn encode_integer(value: usize, prefix_size: u8) -> Vec<u8> {
    let mut res = Vec::new();
    encode_integer_into(value, prefix_size, 0, &mut res).unwrap();
    res
}

/// Represents an HPACK encoder. Allows clients to encode arbitrary header sets
/// and tracks the encoding context. That is, encoding subsequent header sets
/// will use the context built by previous encode calls.
///
/// This is the main API for performing HPACK encoding of headers.
pub struct Encoder {
    /// The header table represents the encoder's context
    header_table: HeaderTable,
}

impl Encoder {
    /// Creates a new `Encoder` with a default static table, as defined by the
    /// HPACK spec (Appendix A).
    pub fn new() -> Encoder {
        Encoder {
            header_table: HeaderTable::with_static_table(StaticTable::new()),
        }
    }

    /// Encodes the given headers using the HPACK rules and returns a newly
    /// allocated `Vec` containing the bytes representing the encoded header
    /// set.
    ///
    /// The encoder so far supports only a single, extremely simple encoding
    /// strategy, whereby each header is represented as an indexed header if
    /// already found in the header table and a literal otherwise. When a
    /// header isn't found in the table, it is added if the header name wasn't
    /// found either (i.e. there are never two header names with different
    /// values in the produced header table). Strings are always encoded as
    /// literals (Huffman encoding is not used).
    pub fn encode<'b, I>(&mut self, headers: I) -> Vec<u8>
    where
        I: IntoIterator<Item = (&'b [u8], &'b [u8])>,
    {
        let mut encoded: Vec<u8> = Vec::new();
        self.encode_into(headers, &mut encoded).unwrap();
        encoded
    }

    /// Encodes the given headers into the given `io::Write` instance. If the io::Write raises an
    /// Error at any point, this error is propagated out. Any changes to the internal state of the
    /// encoder will not be rolled back, though, so care should be taken to ensure that the paired
    /// decoder also ends up seeing the same state updates or that their pairing is cancelled.
    pub fn encode_into<'b, I, W>(&mut self, headers: I, writer: &mut W) -> io::Result<()>
    where
        I: IntoIterator<Item = (&'b [u8], &'b [u8])>,
        W: io::Write,
    {
        for header in headers {
            self.encode_header_into(header, writer)?;
        }
        Ok(())
    }

    /// Encodes a single given header into the given `io::Write` instance.
    ///
    /// Any errors are propagated, similarly to the `encode_into` method, and it is the callers
    /// responsiblity to make sure that the paired encoder sees them too.
    pub fn encode_header_into<W: io::Write>(
        &mut self,
        header: (&[u8], &[u8]),
        writer: &mut W,
    ) -> io::Result<()> {
        match self.header_table.find_header(header) {
            None => {
                // The name of the header is in no tables: need to encode
                // it with both a literal name and value.
                self.encode_literal(&header, true, writer)?;
                self.header_table
                    .add_header(Bytes::from(header.0), Bytes::from(header.1));
            }
            Some((index, HeaderValueFound::NameOnlyFound)) => {
                // The name of the header is at the given index, but the
                // value does not match the current one: need to encode
                // only the value as a literal.
                self.encode_indexed_name((index, header.1), false, writer)?;
            }
            Some((index, HeaderValueFound::Found)) => {
                // The full header was found in one of the tables, so we
                // just encode the index.
                self.encode_indexed(index, writer)?;
            }
        };
        Ok(())
    }

    /// Encodes a header as a literal (i.e. both the name and the value are
    /// encoded as a string literal) and places the result in the given buffer
    /// `buf`.
    ///
    /// # Parameters
    ///
    /// - `header` - the header to be encoded
    /// - `should_index` - indicates whether the given header should be indexed, i.e.
    ///                    inserted into the dynamic table
    /// - `buf` - The buffer into which the result is placed
    ///
    fn encode_literal<W: io::Write>(
        &mut self,
        header: &(&[u8], &[u8]),
        should_index: bool,
        buf: &mut W,
    ) -> io::Result<()> {
        let mask = if should_index { 0x40 } else { 0x0 };

        buf.write_all(&[mask])?;
        self.encode_string_literal(&header.0, buf)?;
        self.encode_string_literal(&header.1, buf)?;
        Ok(())
    }

    /// Encodes a string literal and places the result in the given buffer
    /// `buf`.
    ///
    /// The function does not consider Huffman encoding for now, but always
    /// produces a string literal representations, according to the HPACK spec
    /// section 5.2.
    fn encode_string_literal<W: io::Write>(
        &mut self,
        octet_str: &[u8],
        buf: &mut W,
    ) -> io::Result<()> {
        encode_integer_into(octet_str.len(), 7, 0, buf)?;
        buf.write_all(octet_str)?;
        Ok(())
    }

    /// Encodes a header whose name is indexed and places the result in the
    /// given buffer `buf`.
    fn encode_indexed_name<W: io::Write>(
        &mut self,
        header: (usize, &[u8]),
        should_index: bool,
        buf: &mut W,
    ) -> io::Result<()> {
        let (mask, prefix) = if should_index { (0x40, 6) } else { (0x0, 4) };

        encode_integer_into(header.0, prefix, mask, buf)?;
        // So far, we rely on just one strategy for encoding string literals.
        self.encode_string_literal(&header.1, buf)?;
        Ok(())
    }

    /// Encodes an indexed header (a header that is fully in the header table)
    /// and places the result in the given buffer `buf`.
    ///
    /// The encoding is according to the rules of the HPACK spec, section 6.1.
    fn encode_indexed<W: io::Write>(&self, index: usize, buf: &mut W) -> io::Result<()> {
        // We need to set the most significant bit, since the bit-pattern is
        // `1xxxxxxx` for indexed headers.
        encode_integer_into(index, 7, 0x80, buf)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::encode_integer;
    use super::Encoder;

    use super::super::Decoder;

    #[test]
    fn test_encode_integer() {
        assert_eq!(encode_integer(10, 5), [10]);
        assert_eq!(encode_integer(1337, 5), [31, 154, 10]);
        assert_eq!(encode_integer(127, 7), [127, 0]);
        assert_eq!(encode_integer(255, 8), [255, 0]);
        assert_eq!(encode_integer(254, 8), [254]);
        assert_eq!(encode_integer(1, 8), [1]);
        assert_eq!(encode_integer(0, 8), [0]);
        assert_eq!(encode_integer(255, 7), [127, 128, 1]);
    }

    /// A helper function that checks whether the given buffer can be decoded
    /// into a set of headers that corresponds to the given `headers` list.
    /// Relies on using the `hpack::decoder::Decoder`` struct for
    /// performing the decoding.
    ///
    /// # Returns
    ///
    /// A `bool` indicating whether such a decoding can be performed.
    fn is_decodable(buf: &Vec<u8>, headers: &[(Vec<u8>, Vec<u8>)]) -> bool {
        let mut decoder = Decoder::new();
        match decoder.decode(buf).ok() {
            Some(h) => {
                h == headers
                    .iter()
                    .map(|(k, v)| (Bytes::from(&(*k)[..]), Bytes::from(&(*v)[..])))
                    .collect::<Vec<_>>()
            }
            None => false,
        }
    }

    /// Tests that encoding only the `:method` header works.
    #[test]
    fn test_encode_only_method() {
        let mut encoder: Encoder = Encoder::new();
        let headers = vec![(b":method".to_vec(), b"GET".to_vec())];

        let result = encoder.encode(headers.iter().map(|h| (&h.0[..], &h.1[..])));

        debug!("{:?}", result);
        assert!(is_decodable(&result, &headers));
    }

    /// Tests that when a single custom header is sent it gets indexed by the
    /// coder.
    #[test]
    fn test_custom_header_gets_indexed() {
        let mut encoder: Encoder = Encoder::new();
        let headers = vec![(b"custom-key".to_vec(), b"custom-value".to_vec())];

        let result = encoder.encode(headers.iter().map(|h| (&h.0[..], &h.1[..])));
        assert!(is_decodable(&result, &headers));
        // The header is in the encoder's dynamic table.
        assert_eq!(encoder.header_table.dynamic_table.to_vec_of_vec(), headers);
        // ...but also indicated as such in the output.
        assert!(0x40 == (0x40 & result[0]));
        debug!("{:?}", result);
    }

    /// Tests that when a header gets added to the dynamic table, the encoder
    /// will use the index, instead of the literal representation on the next
    /// encoding of the same header.
    #[test]
    fn test_uses_index_on_second_iteration() {
        let mut encoder: Encoder = Encoder::new();
        let headers = vec![(b"custom-key".to_vec(), b"custom-value".to_vec())];
        // First encoding...
        let _ = encoder.encode(headers.iter().map(|h| (&h.0[..], &h.1[..])));

        // Encode the same headers again!
        let result = encoder.encode(headers.iter().map(|h| (&h.0[..], &h.1[..])));

        // The header is in the encoder's dynamic table.
        assert_eq!(encoder.header_table.dynamic_table.to_vec_of_vec(), headers);
        // The output is a single index byte?
        assert_eq!(result.len(), 1);
        // The index is correctly encoded:
        // - The most significant bit is set
        assert_eq!(0x80 & result[0], 0x80);
        // - The other 7 bits decode to an integer giving the index in the full
        //   header address space.
        assert_eq!(result[0] ^ 0x80, 62);
        // The header table actually contains the header at that index?
        assert_eq!(
            encoder.header_table.get_from_table_vec(62).unwrap(),
            headers[0]
        );
    }

    /// Tests that when a header name is indexed, but the value isn't, the
    /// header is represented by an index (for the name) and a literal (for
    /// the value).
    #[test]
    fn test_name_indexed_value_not() {
        {
            let mut encoder: Encoder = Encoder::new();
            // `:method` is in the static table, but only for GET and POST
            let headers = vec![(b":method", b"PUT")];

            let result = encoder.encode(headers.iter().map(|h| (&h.0[..], &h.1[..])));

            // The first byte represents the index in the header table: last
            // occurrence of `:method` is at index 3.
            assert_eq!(result[0], 3);
            // The rest of it correctly represents PUT?
            assert_eq!(&result[1..], &[3, b'P', b'U', b'T']);
        }
        {
            let mut encoder: Encoder = Encoder::new();
            // `:method` is in the static table, but only for GET and POST
            let headers = vec![(b":authority".to_vec(), b"example.com".to_vec())];

            let result = encoder.encode(headers.iter().map(|h| (&h.0[..], &h.1[..])));

            assert_eq!(result[0], 1);
            // The rest of it correctly represents PUT?
            assert_eq!(
                &result[1..],
                &[11, b'e', b'x', b'a', b'm', b'p', b'l', b'e', b'.', b'c', b'o', b'm']
            )
        }
    }

    /// Tests that multiple headers are correctly encoded (i.e. can be decoded
    /// back to their original representation).
    #[test]
    fn test_multiple_headers_encoded() {
        let mut encoder = Encoder::new();
        let headers = vec![
            (b"custom-key".to_vec(), b"custom-value".to_vec()),
            (b":method".to_vec(), b"GET".to_vec()),
            (b":path".to_vec(), b"/some/path".to_vec()),
        ];

        let result = encoder.encode(headers.iter().map(|h| (&h.0[..], &h.1[..])));

        assert!(is_decodable(&result, &headers));
    }
}
