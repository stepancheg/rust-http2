#[derive(PartialEq, Eq, Debug, Copy, Clone)]
pub enum RequestOrResponse {
    Request,
    Response,
}

impl RequestOrResponse {
    pub fn invert(&self) -> RequestOrResponse {
        match self {
            RequestOrResponse::Request => RequestOrResponse::Response,
            RequestOrResponse::Response => RequestOrResponse::Request,
        }
    }
}
