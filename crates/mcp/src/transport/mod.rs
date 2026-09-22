//! Getting JSON-RPC messages to and from [`crate::Mcp`].
//!
//! Both transports are thin by design: framing, auth and status codes live
//! here, and every decision about what a method *means* lives in the
//! server module, so the two cannot drift apart.

pub mod http;
pub mod stdio;
