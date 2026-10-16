//! Why an uploaded file can't be imported: a code the client words the
//! refusal by, the English for any other caller, and the values the wording
//! takes. The upload and a sheet change answer 400 with all three
//! (`{error, code, args}`).

use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub code: &'static str,
    pub message: String,
    pub args: Map<String, Value>,
}

impl Refused {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            args: Map::new(),
        }
    }

    /// The refusal with `value` for its wording's `name`.
    pub fn arg(mut self, name: &str, value: impl Into<Value>) -> Self {
        self.args.insert(name.to_string(), value.into());
        self
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Refused {}
