use bytes::Bytes;
use std::collections::VecDeque;
use std::fmt;

/// A struct representing the dynamic table that needs to be maintained by the
/// coder.
///
/// The dynamic table contains a number of recently used headers. The size of
/// the table is constrained to a certain number of octets. If on insertion of
/// a new header into the table, the table would exceed the maximum size,
/// headers are evicted in a FIFO fashion until there is enough room for the
/// new header to be inserted. (Therefore, it is possible that though all
/// elements end up being evicted, there is still not enough space for the new
/// header: when the size of this individual header exceeds the maximum size of
/// the table.)
///
/// The current size of the table is calculated, based on the IETF definition,
/// as the sum of sizes of each header stored within the table, where the size
/// of an individual header is
/// `len_in_octets(header_name) + len_in_octets(header_value) + 32`.
///
/// Note: the maximum size of the dynamic table does not have to be equal to
/// the maximum header table size as defined by a "higher level" protocol
/// (such as the `SETTINGS_HEADER_TABLE_SIZE` setting in HTTP/2), since HPACK
/// can choose to modify the dynamic table size on the fly (as long as it keeps
/// it below the maximum value set by the protocol). So, the `DynamicTable`
/// only cares about the maximum size as set by the HPACK {en,de}coder and lets
/// *it* worry about making certain that the changes are valid according to
/// the (current) constraints of the protocol.
pub(crate) struct DynamicTable {
    table: VecDeque<(Bytes, Bytes)>,
    size: usize,
    max_size: usize,
}

impl DynamicTable {
    /// Creates a new empty dynamic table with a default size.
    pub fn new() -> DynamicTable {
        // The default maximum size corresponds to the default HTTP/2
        // setting
        DynamicTable::with_size(4096)
    }

    /// Creates a new empty dynamic table with the given maximum size.
    fn with_size(max_size: usize) -> DynamicTable {
        DynamicTable {
            table: VecDeque::new(),
            size: 0,
            max_size: max_size,
        }
    }

    /// Returns the current size of the table in octets, as defined by the IETF
    /// HPACK spec.
    pub fn get_size(&self) -> usize {
        self.size
    }

    /// Returns an `Iterator` through the headers stored in the `DynamicTable`.
    ///
    /// The iterator will yield elements of type `(&[u8], &[u8])`,
    /// corresponding to a single header name and value. The name and value
    /// slices are borrowed from their representations in the `DynamicTable`
    /// internal implementation, which means that it is possible only to
    /// iterate through the headers, not mutate them.
    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.table.iter().map(|(k, v)| (&k[..], &v[..]))
    }

    /// Sets the new maximum table size.
    ///
    /// If the current size of the table is larger than the new maximum size,
    /// existing headers are evicted in a FIFO fashion until the size drops
    /// below the new maximum.
    pub fn set_max_table_size(&mut self, new_max_size: usize) {
        self.max_size = new_max_size;
        // Make the table size fit within the new constraints.
        self.consolidate_table();
    }

    /// Returns the maximum size of the table in octets.
    #[cfg(test)]
    pub fn get_max_table_size(&self) -> usize {
        self.max_size
    }

    /// Add a new header to the dynamic table.
    ///
    /// The table automatically gets resized, if necessary.
    ///
    /// Do note that, under the HPACK rules, it is possible the given header
    /// is not found in the dynamic table after this operation finishes, in
    /// case the total size of the given header exceeds the maximum size of the
    /// dynamic table.
    pub fn add_header(&mut self, name: Bytes, value: Bytes) {
        // This is how the HPACK spec makes us calculate the size.  The 32 is
        // a magic number determined by them (under reasonable assumptions of
        // how the table is stored).
        self.size += name.len() + value.len() + 32;
        ndc_debug!("New dynamic table size {}", self.size);
        // Now add it to the internal buffer
        self.table.push_front((name, value));
        // ...and make sure we're not over the maximum size.
        self.consolidate_table();
        ndc_debug!("After consolidation dynamic table size {}", self.size);
    }

    #[cfg(test)]
    fn add_header_for_test(&mut self, k: impl Into<Bytes>, v: impl Into<Bytes>) {
        self.add_header(k.into(), v.into());
    }

    /// Consolidates the table entries so that the table size is below the
    /// maximum allowed size, by evicting headers from the table in a FIFO
    /// fashion.
    fn consolidate_table(&mut self) {
        while self.size > self.max_size {
            {
                let last_header = match self.table.back() {
                    Some(x) => x,
                    None => {
                        // Can never happen as the size of the table must reach
                        // 0 by the time we've exhausted all elements.
                        panic!("Size of table != 0, but no headers left!");
                    }
                };
                self.size -= last_header.0.len() + last_header.1.len() + 32;
            }
            self.table.pop_back();
        }
    }

    /// Converts the current state of the table to a `Vec`
    #[cfg(test)]
    fn to_vec_of_bytes(&self) -> Vec<(Bytes, Bytes)> {
        let mut ret = Vec::new();
        for elem in &self.table {
            ret.push(elem.clone());
        }

        ret
    }

    #[cfg(test)]
    pub fn to_vec_of_vec(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.to_vec_of_bytes()
            .into_iter()
            .map(|(k, v)| (k.to_vec(), v.to_vec()))
            .collect()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.table.len()
    }

    /// Returns a reference to the header at the given index, if found in the
    /// dynamic table.
    pub fn get(&self, index: usize) -> Option<&(Bytes, Bytes)> {
        self.table.get(index)
    }
}

impl fmt::Debug for DynamicTable {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "{:?}", self.table)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_dynamic_table_size_calculation_simple() {
        let mut table = DynamicTable::new();
        // Sanity check
        assert_eq!(0, table.get_size());

        table.add_header_for_test(b"a".to_vec(), b"b".to_vec());

        assert_eq!(32 + 2, table.get_size());
    }

    #[test]
    fn test_dynamic_table_size_calculation() {
        let mut table = DynamicTable::new();

        table.add_header_for_test(b"a".to_vec(), b"b".to_vec());
        table.add_header_for_test(b"123".to_vec(), b"456".to_vec());
        table.add_header_for_test(b"a".to_vec(), b"b".to_vec());

        assert_eq!(3 * 32 + 2 + 6 + 2, table.get_size());
    }

    /// Tests that the `DynamicTable` gets correctly resized (by evicting old
    /// headers) if it exceeds the maximum size on an insertion.
    #[test]
    fn test_dynamic_table_auto_resize() {
        let mut table = DynamicTable::with_size(38);
        table.add_header_for_test(b"a".to_vec(), b"b".to_vec());
        assert_eq!(32 + 2, table.get_size());

        table.add_header_for_test(b"123".to_vec(), b"456".to_vec());

        // Resized?
        assert_eq!(32 + 6, table.get_size());
        // Only has the second header?
        assert_eq!(
            table.to_vec_of_vec(),
            vec![(b"123".to_vec(), b"456".to_vec())]
        );
    }

    /// Tests that when inserting a new header whose size is larger than the
    /// size of the entire table, the table is fully emptied.
    #[test]
    fn test_dynamic_table_auto_resize_into_empty() {
        let mut table = DynamicTable::with_size(38);
        table.add_header_for_test(b"a".to_vec(), b"b".to_vec());
        assert_eq!(32 + 2, table.get_size());

        table.add_header_for_test(b"123".to_vec(), b"4567".to_vec());

        // Resized and empty?
        assert_eq!(0, table.get_size());
        assert_eq!(0, table.to_vec_of_bytes().len());
    }

    /// Tests that when changing the maximum size of the `DynamicTable`, the
    /// headers are correctly evicted in order to keep its size below the new
    /// max.
    #[test]
    fn test_dynamic_table_change_max_size() {
        let mut table = DynamicTable::new();
        table.add_header_for_test(b"a".to_vec(), b"b".to_vec());
        table.add_header_for_test(b"123".to_vec(), b"456".to_vec());
        table.add_header_for_test(b"c".to_vec(), b"d".to_vec());
        assert_eq!(3 * 32 + 2 + 6 + 2, table.get_size());

        table.set_max_table_size(38);

        assert_eq!(32 + 2, table.get_size());
        assert_eq!(table.to_vec_of_vec(), vec![(b"c".to_vec(), b"d".to_vec())]);
    }

    /// Tests that setting the maximum table size to 0 clears the dynamic
    /// table.
    #[test]
    fn test_dynamic_table_clear() {
        let mut table = DynamicTable::new();
        table.add_header_for_test(b"a".to_vec(), b"b".to_vec());
        table.add_header_for_test(b"123".to_vec(), b"456".to_vec());
        table.add_header_for_test(b"c".to_vec(), b"d".to_vec());
        assert_eq!(3 * 32 + 2 + 6 + 2, table.get_size());

        table.set_max_table_size(0);

        assert_eq!(0, table.len());
        assert_eq!(0, table.to_vec_of_bytes().len());
        assert_eq!(0, table.get_size());
        assert_eq!(0, table.get_max_table_size());
    }

    /// Tests that when the initial max size of the table is 0, nothing
    /// can be added to the table.
    #[test]
    fn test_dynamic_table_max_size_zero() {
        let mut table = DynamicTable::with_size(0);

        table.add_header_for_test(b"a".to_vec(), b"b".to_vec());

        assert_eq!(0, table.len());
        assert_eq!(0, table.to_vec_of_bytes().len());
        assert_eq!(0, table.get_size());
        assert_eq!(0, table.get_max_table_size());
    }

    /// Tests that the iterator through the `DynamicTable` works when there are
    /// some elements in the dynamic table.
    #[test]
    fn test_dynamic_table_iter_with_elems() {
        let mut table = DynamicTable::new();
        table.add_header_for_test(b"a".to_vec(), b"b".to_vec());
        table.add_header_for_test(b"123".to_vec(), b"456".to_vec());
        table.add_header_for_test(b"c".to_vec(), b"d".to_vec());

        let iter_res: Vec<(&[u8], &[u8])> = table.iter().collect();

        let expected: Vec<(&[u8], &[u8])> = vec![(b"c", b"d"), (b"123", b"456"), (b"a", b"b")];
        assert_eq!(iter_res, expected);
    }

    /// Tests that the iterator through the `DynamicTable` works when there are
    /// no elements in the dynamic table.
    #[test]
    fn test_dynamic_table_iter_no_elems() {
        let table = DynamicTable::new();

        let iter_res: Vec<(&[u8], &[u8])> = table.iter().collect();

        let expected = vec![];
        assert_eq!(iter_res, expected);
    }
}
