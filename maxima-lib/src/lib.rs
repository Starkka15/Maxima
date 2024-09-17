#![feature(type_ascription)]
#![feature(slice_pattern)]
#![feature(string_remove_matches)]
#![feature(trait_alias)]
#![feature(type_alias_impl_trait)]

pub mod content;
pub mod core;
pub mod lsx;
pub mod ooa;
pub mod rtm;
pub mod util;

#[cfg(unix)]
pub mod unix;

#[cfg(not(target_arch = "x86_64"))]
compile_error!("Only x86_64 is supported at the moment");
