// SPDX-License-Identifier: AGPL-3.0-only

//! The steps that turn remote services into a list: [`source`] fetches, [`map`] aligns field names,
//! [`filter`] selects, [`sort`] orders. [`view`] composes them.

pub mod filter;
pub mod map;
pub mod sort;
pub mod source;
pub mod view;
