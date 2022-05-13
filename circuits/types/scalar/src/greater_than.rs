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

use super::*;

impl<E: Environment> GreaterThan<Scalar<E>> for Scalar<E> {
    type Output = Boolean<E>;

    /// Returns `true` if `self` is greater than `other`.
    fn is_greater_than(&self, other: &Self) -> Self::Output {
        other.is_less_than(self)
    }
}

impl<E: Environment> Metadata<dyn GreaterThan<Scalar<E>, Output = Boolean<E>>> for Scalar<E> {
    type Case = (CircuitType<Self>, CircuitType<Self>);
    type OutputType = CircuitType<Boolean<E>>;

    fn count(case: &Self::Case) -> Count {
        let (left, right) = case.clone();
        count!(Self, LessThan<Self, Output = Boolean<E>>, &(right, left))
    }

    fn output_type(case: Self::Case) -> Self::OutputType {
        let (left, right) = case;
        output_type!(Self, LessThan<Self, Output = Boolean<E>>, (right, left))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snarkvm_circuits_environment::Circuit;
    use snarkvm_utilities::{test_rng, UniformRand};

    const ITERATIONS: u64 = 100;

    fn check_is_greater_than(mode_a: Mode, mode_b: Mode) {
        for i in 0..ITERATIONS {
            // Sample a random element `a`.
            let expected_a: <Circuit as Environment>::ScalarField = UniformRand::rand(&mut test_rng());
            let candidate_a = Scalar::<Circuit>::new(mode_a, expected_a);

            // Sample a random element `b`.
            let expected_b: <Circuit as Environment>::ScalarField = UniformRand::rand(&mut test_rng());
            let candidate_b = Scalar::<Circuit>::new(mode_b, expected_b);

            // Perform the less than comparison.
            Circuit::scope(&format!("{} {} {}", mode_a, mode_b, i), || {
                let candidate = candidate_a.is_greater_than(&candidate_b);
                assert_eq!(expected_a > expected_b, candidate.eject_value());

                let case = (CircuitType::from(candidate_a), CircuitType::from(candidate_b));
                assert_count!(GreaterThan(Scalar, Scalar) => Boolean, &case);
                assert_output_type!(GreaterThan(Scalar, Scalar) => Boolean, case, candidate);
            });
            Circuit::reset();
        }
    }

    #[test]
    fn test_constant_is_greater_than_constant() {
        check_is_greater_than(Mode::Constant, Mode::Constant);
    }

    #[test]
    fn test_constant_is_greater_than_public() {
        check_is_greater_than(Mode::Constant, Mode::Public);
    }

    #[test]
    fn test_constant_is_greater_than_private() {
        check_is_greater_than(Mode::Constant, Mode::Private);
    }

    #[test]
    fn test_public_is_greater_than_constant() {
        check_is_greater_than(Mode::Public, Mode::Constant);
    }

    #[test]
    fn test_public_is_greater_than_public() {
        check_is_greater_than(Mode::Public, Mode::Public);
    }

    #[test]
    fn test_public_is_greater_than_private() {
        check_is_greater_than(Mode::Public, Mode::Private);
    }

    #[test]
    fn test_private_is_greater_than_constant() {
        check_is_greater_than(Mode::Private, Mode::Constant);
    }

    #[test]
    fn test_private_is_greater_than_public() {
        check_is_greater_than(Mode::Private, Mode::Public);
    }

    #[test]
    fn test_private_is_greater_than_private() {
        check_is_greater_than(Mode::Private, Mode::Private);
    }
}