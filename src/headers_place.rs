#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum HeadersPlace {
    // Initial headers including continuation
    Initial,
    Trailing,
}
