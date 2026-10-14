use rmx::prelude::*;

#[salsa::input]
#[derive(Debug)]
pub struct Source {
    #[returns(ref)]
    pub text: String,
}

impl Source {
    /// The source's text.
    pub fn as_str<'db>(&self, db: &'db dyn crate::Db) -> &'db str {
        self.text(db).as_str()
    }
}
