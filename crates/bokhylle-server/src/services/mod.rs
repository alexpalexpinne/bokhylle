//! Application services shared by the HTTP routes and the MCP tools. These
//! own the product decisions (profile rules, existing-copy reuse, request
//! approval) so no interface re-implements them.

pub mod books;
pub mod delivery;
pub mod reader;
pub mod requests;
