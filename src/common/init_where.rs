/// Who initiated a stream: locally or by peer
#[derive(Eq, PartialEq, Debug)]
pub enum InitWhere {
    Locally,
    Peer,
}
