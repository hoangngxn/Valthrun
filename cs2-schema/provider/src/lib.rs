mod provider;
pub use provider::*;

mod resolver;
pub use resolver::{
    resolve_offset,
    try_resolve_offset,
};
