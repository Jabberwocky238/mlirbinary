use alloc::string::String;

use mlirformat::{Error, Operation};

/// SPIR-V words ⇒ a `spirv.module` in generic form
pub struct SpirvReader;

impl SpirvReader {
    pub fn read(_bytes: &[u8]) -> Result<Operation<'static>, Error> {
        Err(Error(String::from("SPIR-V reading is not written yet")))
    }
}
