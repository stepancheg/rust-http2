use ErrorCode;

#[derive(Eq, PartialEq, Debug)]
pub enum GoAwayState {
    None,
    NeedToSend(ErrorCode),
    Sending,
    Sent,
}

impl Default for GoAwayState {
    fn default() -> Self {
        GoAwayState::None
    }
}
