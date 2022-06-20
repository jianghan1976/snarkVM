// Copyright (C) 2019-2022 Aleo Systems Inc.
// This file is part of the snarkVM library.

// The snarkVM library is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// The snarkVM library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with the snarkVM library. If not, see <https://www.gnu.org/licenses/>.

mod ciphertext;
pub use ciphertext::Ciphertext;

mod entry;
pub use entry::Entry;

mod identifier;
pub use identifier::Identifier;

mod literal;
pub use literal::Literal;

mod plaintext;
pub use plaintext::Plaintext;

mod decrypt;
mod encrypt;
mod to_bits;
mod to_id;

use crate::{FromFields, ToFields};
use snarkvm_console_account::{Address, ViewKey};
use snarkvm_console_network::Network;
use snarkvm_curves::{AffineCurve, ProjectiveCurve};
use snarkvm_utilities::{FromBits, ToBits};

use anyhow::{bail, Result};
use core::ops::Deref;

pub trait Visibility<N: Network>: ToBits + FromBits + ToFields + FromFields {
    /// Returns the number of field elements to encode `self`.
    fn size_in_fields(&self) -> Result<u16>;
}

/// A value stored in program data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Data<N: Network, Private: Visibility<N>>(Vec<(Identifier<N>, Entry<N, Private>)>);

impl<N: Network, Private: Visibility<N>> From<Vec<(Identifier<N>, Entry<N, Private>)>> for Data<N, Private> {
    /// Initializes a new `Data` value from a vector of `(Identifier, Entry)` pairs.
    fn from(entries: Vec<(Identifier<N>, Entry<N, Private>)>) -> Self {
        Self(entries)
    }
}

impl<N: Network, Private: Visibility<N>> Deref for Data<N, Private> {
    type Target = [(Identifier<N>, Entry<N, Private>)];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}