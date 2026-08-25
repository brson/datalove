use rmx::prelude::*;

#[salsa::input]
#[derive(Debug)]
pub struct Source {
    #[returns(ref)]
    pub text: String,
}
