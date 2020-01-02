//! A module implementing HPACK functionality. Exposes a simple API for
//! performing the encoding and decoding of header sets, according to the
//! HPACK spec.

//#[cfg(feature="interop_tests")]
//extern crate rustc_serialize;

// Re-export the main HPACK API entry points.
pub use self::decoder::Decoder;
pub use self::encoder::Encoder;
use bytes::Bytes;
use crate::hpack::dynamic_table::DynamicTable;
use crate::hpack::static_table::StaticTable;

pub mod decoder;
mod dynamic_table;
pub mod encoder;
pub mod huffman;
mod static_table;

/// The struct represents the header table obtained by merging the static and
/// dynamic tables into a single index address space, as described in section
/// `2.3.3.` of the HPACK spec.
struct HeaderTable {
    static_table: StaticTable,
    dynamic_table: DynamicTable,
}

#[derive(Eq, PartialEq, Debug)]
enum HeaderValueFound {
    Found,
    NameOnlyFound,
}

impl HeaderTable {
    /// Creates a new header table where the static part is initialized with
    /// the given static table.
    pub fn with_static_table(static_table: StaticTable) -> HeaderTable {
        HeaderTable {
            static_table: static_table,
            dynamic_table: DynamicTable::new(),
        }
    }

    /// Returns an iterator through *all* headers stored in the header table,
    /// i.e. it includes both the ones found in the static table and the
    /// dynamic table, in the order of their indices in the single address
    /// space (first the headers in the static table, followed by headers in
    /// the dynamic table).
    ///
    /// The type yielded by the iterator is `(&[u8], &[u8])`, where the tuple
    /// corresponds to the header name, value pairs in the described order.
    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.static_table
            .iter()
            .map(|h| h)
            .chain(self.dynamic_table.iter())
    }

    /// Adds the given header to the table. Of course, this means that the new
    /// header is added to the dynamic part of the table.
    ///
    /// If the size of the new header is larger than the current maximum table
    /// size of the dynamic table, the effect will be that the dynamic table
    /// gets emptied and the new header does *not* get inserted into it.
    #[inline]
    pub fn add_header(&mut self, name: Bytes, value: Bytes) {
        self.dynamic_table.add_header(name, value);
    }

    #[cfg(test)]
    fn add_header_for_test(&mut self, name: impl Into<Bytes>, value: impl Into<Bytes>) {
        self.add_header(name.into(), value.into());
    }

    /// Returns a reference to the header (a `(name, value)` pair) with the
    /// given index in the table.
    ///
    /// The table is 1-indexed and constructed in such a way that the first
    /// entries belong to the static table, followed by entries in the dynamic
    /// table. They are merged into a single index address space, though.
    ///
    /// This is according to the [HPACK spec, section 2.3.3.]
    /// (http://http2.github.io/http2-spec/compression.html#index.address.space)
    pub fn get_from_table(&self, index: usize) -> Option<(Bytes, Bytes)> {
        // The IETF defined table indexing as 1-based.
        // So, before starting, make sure the given index is within the proper
        // bounds.
        let real_index = if index > 0 { index - 1 } else { return None };

        match self.static_table.get_by_index(real_index as u32) {
            Ok((k, v)) => Some((Bytes::from_static(k), Bytes::from_static(v))),
            Err(dynamic_index) => match self.dynamic_table.get(dynamic_index as usize) {
                Some(&(ref name, ref value)) => Some((name.clone(), value.clone())),
                None => None,
            },
        }
    }

    #[cfg(test)]
    pub fn get_from_table_vec(&self, index: usize) -> Option<(Vec<u8>, Vec<u8>)> {
        self.get_from_table(index)
            .map(|(k, v)| (k.to_vec(), v.to_vec()))
    }

    /// Finds the given header in the header table. Tries to match both the
    /// header name and value to one of the headers in the table. If no such
    /// header exists, then falls back to returning one that matched only the
    /// name.
    ///
    /// # Returns
    ///
    /// An `Option`, where `Some` corresponds to a tuple representing the index
    /// of the header in the header tables (the 1-based index that HPACK uses)
    /// and a `bool` indicating whether the value of the header also matched.
    pub fn find_header(&self, header: (&[u8], &[u8])) -> Option<(usize, HeaderValueFound)> {
        // Just does a simple scan of the entire table, searching for a header
        // that matches both the name and the value of the given header.
        // If no such header is found, then any one of the headers that had a
        // matching name is returned, with the appropriate return flag set.
        //
        // The tables are so small that it is unlikely that the linear scan
        // would be a major performance bottlneck. If it does prove to be,
        // though, a more efficient lookup/header representation method could
        // be devised.
        let mut matching_name: Option<usize> = None;
        for (i, h) in self.iter().enumerate() {
            if header.0 == h.0 {
                if header.1 == h.1 {
                    // Both name and value matched: returns it immediately
                    return Some((i + 1, HeaderValueFound::Found));
                }
                // If only the name was valid, we continue scanning, hoping to
                // find one where both the name and value match. We remember
                // this one, in case such a header isn't found after all.
                matching_name = Some(i + 1);
            }
        }

        // Finally, if there's no header with a matching name and value,
        // return one that matched only the name, if that *was* found.
        match matching_name {
            Some(i) => Some((i, HeaderValueFound::NameOnlyFound)),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::static_table::STATIC_TABLE;
    use super::HeaderTable;
    use crate::hpack::static_table::StaticTable;
    use crate::hpack::HeaderValueFound;

    /// Tests that indexing the header table with indices that correspond to
    /// entries found in the static table works.
    #[test]
    fn test_header_table_index_static() {
        let table = HeaderTable::with_static_table(StaticTable::new());

        for (index, (k, v)) in STATIC_TABLE.iter().enumerate() {
            assert_eq!(
                table.get_from_table(index + 1).unwrap(),
                (Bytes::from(*k), Bytes::from(*v))
            );
        }
    }

    /// Tests that when the given index is out of bounds, the `HeaderTable`
    /// returns a `None`
    #[test]
    fn test_header_table_index_out_of_bounds() {
        let table = HeaderTable::with_static_table(StaticTable::new());

        assert!(table.get_from_table(0).is_none());
        assert!(table.get_from_table(STATIC_TABLE.len() + 1).is_none());
    }

    /// Tests that adding entries to the dynamic table through the
    /// `HeaderTable` interface works.
    #[test]
    fn test_header_table_add_to_dynamic() {
        let mut table = HeaderTable::with_static_table(StaticTable::new());
        let header = (b"a".to_vec(), b"b".to_vec());

        table.add_header_for_test(header.0.clone(), header.1.clone());

        assert_eq!(table.dynamic_table.to_vec_of_vec(), vec![header]);
    }

    /// Tests that indexing the header table with indices that correspond to
    /// entries found in the dynamic table works.
    #[test]
    fn test_header_table_index_dynamic() {
        let mut table = HeaderTable::with_static_table(StaticTable::new());
        let header = (b"a".to_vec(), b"b".to_vec());

        table.add_header_for_test(header.0.clone(), header.1.clone());

        assert_eq!(
            table.get_from_table_vec(STATIC_TABLE.len() + 1).unwrap(),
            header
        );
    }

    /// Tests that the `iter` method of the `HeaderTable` returns an iterator
    /// through *all* the headers found in the header table (static and dynamic
    /// tables both included)
    #[test]
    fn test_header_table_iter() {
        let mut table = HeaderTable::with_static_table(StaticTable::new());
        let headers: [(&[u8], &[u8]); 2] = [(b"a", b"b"), (b"c", b"d")];
        for header in headers.iter() {
            table.add_header_for_test(header.0.to_vec(), header.1.to_vec());
        }

        let iterated: Vec<(&[u8], &[u8])> = table.iter().collect();

        assert_eq!(iterated.len(), headers.len() + STATIC_TABLE.len());
        // Part of the static table correctly iterated through
        for (h1, h2) in iterated.iter().zip(STATIC_TABLE.iter()) {
            assert_eq!(h1, h2);
        }
        // Part of the dynamic table correctly iterated through: the elements
        // are in reversed order of insertion in the dynamic table.
        for (h1, h2) in iterated
            .iter()
            .skip(STATIC_TABLE.len())
            .zip(headers.iter().rev())
        {
            assert_eq!(h1, h2);
        }
    }

    /// Tests that searching for an entry in the header table, which should be
    /// fully in the static table (both name and value), works correctly.
    #[test]
    fn test_find_header_static_full() {
        let table = HeaderTable::with_static_table(StaticTable::new());

        for (i, h) in STATIC_TABLE.iter().enumerate() {
            assert_eq!(
                table.find_header(*h).unwrap(),
                (i + 1, HeaderValueFound::Found)
            );
        }
    }

    /// Tests that searching for an entry in the header table, which should be
    /// only partially in the static table (only the name), works correctly.
    #[test]
    fn test_find_header_static_partial() {
        {
            let table = HeaderTable::with_static_table(StaticTable::new());
            let h: (&[u8], &[u8]) = (b":method", b"PUT");

            if let (index, HeaderValueFound::NameOnlyFound) = table.find_header(h).unwrap() {
                assert_eq!(h.0, STATIC_TABLE[index - 1].0);
                // The index is the last one with the corresponding name
                assert_eq!(3, index);
            } else {
                panic!("The header should have matched only partially");
            }
        }
        {
            let table = HeaderTable::with_static_table(StaticTable::new());
            let h: (&[u8], &[u8]) = (b":status", b"333");

            if let (index, HeaderValueFound::NameOnlyFound) = table.find_header(h).unwrap() {
                assert_eq!(h.0, STATIC_TABLE[index - 1].0);
                // The index is the last one with the corresponding name
                assert_eq!(14, index);
            } else {
                panic!("The header should have matched only partially");
            }
        }
        {
            let table = HeaderTable::with_static_table(StaticTable::new());
            let h: (&[u8], &[u8]) = (b":authority", b"example.com");

            if let (index, HeaderValueFound::NameOnlyFound) = table.find_header(h).unwrap() {
                assert_eq!(h.0, STATIC_TABLE[index - 1].0);
            } else {
                panic!("The header should have matched only partially");
            }
        }
        {
            let table = HeaderTable::with_static_table(StaticTable::new());
            let h: (&[u8], &[u8]) = (b"www-authenticate", b"asdf");

            if let (index, HeaderValueFound::NameOnlyFound) = table.find_header(h).unwrap() {
                assert_eq!(h.0, STATIC_TABLE[index - 1].0);
            } else {
                panic!("The header should have matched only partially");
            }
        }
    }

    /// Tests that searching for an entry in the header table, which should be
    /// fully in the dynamic table (both name and value), works correctly.
    #[test]
    fn test_find_header_dynamic_full() {
        let mut table = HeaderTable::with_static_table(StaticTable::new());
        let h: (&[u8], &[u8]) = (b":method", b"PUT");
        table.add_header_for_test(h.0.to_vec(), h.1.to_vec());

        if let (index, HeaderValueFound::Found) = table.find_header(h).unwrap() {
            assert_eq!(index, STATIC_TABLE.len() + 1);
        } else {
            panic!("The header should have matched fully");
        }
    }

    /// Tests that searching for an entry in the header table, which should be
    /// only partially in the dynamic table (only the name), works correctly.
    #[test]
    fn test_find_header_dynamic_partial() {
        let mut table = HeaderTable::with_static_table(StaticTable::new());
        // First add it to the dynamic table
        {
            let h = (b"X-Custom-Header", b"stuff");
            table.add_header_for_test(h.0.to_vec(), h.1.to_vec());
        }
        // Prepare a search
        let h: (&[u8], &[u8]) = (b"X-Custom-Header", b"different-stuff");

        // It must match only partially
        if let (index, HeaderValueFound::NameOnlyFound) = table.find_header(h).unwrap() {
            // The index must be the first one in the dynamic table
            // segment of the header table.
            assert_eq!(index, STATIC_TABLE.len() + 1);
        } else {
            panic!("The header should have matched only partially");
        }
    }
}
