// SPDX-License-Identifier: AGPL-3.0-or-later

//! The steps that turn remote services into a list: [`source`] fetches, [`map`] aligns field names,
//! [`normalize`] applies the rules of each kind of item, [`filter`] selects, [`sort`] orders.
//! [`view`] holds what to select and how to order as data, [`viewmodel`] prepares what the UIs draw
//! and [`engine`] is the one interface the rest of the application talks to.

pub mod cache;
pub mod config;
pub mod credentials;
pub mod discover;
pub mod engine;
pub mod filter;
pub mod item;
pub mod map;
pub mod normalize;
pub mod notify;
pub mod paths;
pub mod sort;
pub mod source;
pub mod view;
pub mod viewmodel;
pub mod when;
