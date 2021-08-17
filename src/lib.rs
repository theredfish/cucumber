// Copyright (c) 2018-2021  Brendan Molloy <brendan@bbqsrc.net>,
//                          Ilya Solovyiov <ilya.solovyiov@gmail.com>,
//                          Kai Ren <tyranron@gmail.com>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![doc = include_str!("../README.md")]
#![deny(
    nonstandard_style,
    rust_2018_idioms,
    rustdoc::broken_intra_doc_links,
    rustdoc::private_intra_doc_links,
    trivial_casts,
    trivial_numeric_casts
)]
#![forbid(non_ascii_idents, unsafe_code)]
#![warn(
    deprecated_in_future,
    missing_copy_implementations,
    missing_debug_implementations,
    missing_docs,
    unreachable_pub,
    unused_import_braces,
    unused_labels,
    unused_qualifications,
    unused_results
)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod cucumber;
pub mod event;
pub mod feature;
pub mod parser;
pub mod runner;
pub mod step;
pub mod writer;

#[cfg(feature = "macros")]
#[doc(hidden)]
pub mod private;

use std::error::Error as StdError;

use async_trait::async_trait;

pub use gherkin;

#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
#[doc(inline)]
pub use self::private::WorldInit;
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
#[doc(inline)]
pub use cucumber_rust_codegen::{given, then, when, WorldInit};

#[doc(inline)]
pub use self::{
    cucumber::Cucumber,
    parser::Parser,
    runner::Runner,
    step::Step,
    writer::{Ext as WriterExt, Writer},
};

/// Represents a shared user-defined state for a [Cucumber] run.
/// It lives on per-[scenario] basis.
///
/// [Cucumber]: https://cucumber.io
/// [scenario]: https://cucumber.io/docs/gherkin/reference/#descriptions
#[async_trait(?Send)]
pub trait World: Sized + 'static {
    /// Error of creating a new [`World`] instance.
    type Error: StdError;

    /// Creates a new [`World`] instance.
    async fn new() -> Result<Self, Self::Error>;
}
