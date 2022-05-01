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

impl<E: Environment, I: IntegerType, M: Magnitude> Shr<Integer<E, M>> for Integer<E, I> {
    type Output = Self;

    fn shr(self, rhs: Integer<E, M>) -> Self::Output {
        self >> &rhs
    }
}

impl<E: Environment, I: IntegerType, M: Magnitude> Shr<Integer<E, M>> for &Integer<E, I> {
    type Output = Integer<E, I>;

    fn shr(self, rhs: Integer<E, M>) -> Self::Output {
        self >> &rhs
    }
}

impl<E: Environment, I: IntegerType, M: Magnitude> Shr<&Integer<E, M>> for Integer<E, I> {
    type Output = Self;

    fn shr(self, rhs: &Integer<E, M>) -> Self::Output {
        &self >> rhs
    }
}

impl<E: Environment, I: IntegerType, M: Magnitude> Shr<&Integer<E, M>> for &Integer<E, I> {
    type Output = Integer<E, I>;

    fn shr(self, rhs: &Integer<E, M>) -> Self::Output {
        let mut output = self.clone();
        output >>= rhs;
        output
    }
}

impl<E: Environment, I: IntegerType, M: Magnitude> ShrAssign<Integer<E, M>> for Integer<E, I> {
    fn shr_assign(&mut self, rhs: Integer<E, M>) {
        *self >>= &rhs
    }
}

impl<E: Environment, I: IntegerType, M: Magnitude> ShrAssign<&Integer<E, M>> for Integer<E, I> {
    fn shr_assign(&mut self, rhs: &Integer<E, M>) {
        // Stores the result of `self` >> `other` in `self`.
        *self = self.shr_checked(rhs);
    }
}

impl<E: Environment, I: IntegerType, M: Magnitude> ShrChecked<Integer<E, M>> for Integer<E, I> {
    type Output = Self;

    #[inline]
    fn shr_checked(&self, rhs: &Integer<E, M>) -> Self::Output {
        // Determine the variable mode.
        if self.is_constant() && rhs.is_constant() {
            // This cast is safe since `Magnitude`s can only be `u8`, `u16`, or `u32`.
            match self.eject_value().checked_shr(rhs.eject_value().to_u32().unwrap()) {
                Some(value) => Integer::new(Mode::Constant, value),
                None => E::halt("Constant shifted by constant exceeds the allowed bitwidth."),
            }
        } else {
            // Index of the first upper bit of rhs that must be zero.
            // This is a safe case as I::BITS = 8, 16, 32, 64, or 128.
            // Therefore there is at least one trailing zero.
            let first_upper_bit_index = I::BITS.trailing_zeros() as usize;

            let upper_bits_are_nonzero =
                rhs.bits_le[first_upper_bit_index..].iter().fold(Boolean::constant(false), |a, b| a | b);

            // Halt if upper bits of rhs are constant and nonzero.
            if upper_bits_are_nonzero.is_constant() && upper_bits_are_nonzero.eject_value() {
                E::halt("Integer shifted by constant exceeds the allowed bitwidth.")
            }
            // Enforce that the appropriate number of upper bits in rhs are zero.
            E::assert_eq(upper_bits_are_nonzero, E::zero());

            // Perform a wrapping shift right.
            self.shr_wrapped(rhs)
        }
    }
}

impl<E: Environment, I: IntegerType, M: Magnitude> Metrics<dyn ShrChecked<Integer<E, M>, Output = Integer<E, I>>>
    for Integer<E, I>
{
    type Case = (Mode, Mode);

    fn count(case: &Self::Case) -> Count {
        // A quick hack that matches `(u8 -> 0, u16 -> 1, u32 -> 2, u64 -> 3, u128 -> 4)`.
        let index = |num_bits: u64| match [8, 16, 32, 64, 128].iter().position(|&bits| bits == num_bits) {
            Some(index) => index as u64,
            None => E::halt(format!("Integer of {num_bits} bits is not supported")),
        };

        match (case.0, case.1) {
            (Mode::Constant, Mode::Constant) => Count::is(I::BITS, 0, 0, 0),
            (_, Mode::Constant) => Count::is(0, 0, 0, 0),
            (Mode::Constant, _) | (_, _) => {
                let wrapped_count = count!(Integer<E, I>, ShrWrapped<Integer<E, M>, Output=Integer<E, I>>, case);
                wrapped_count + Count::is(0, 0, M::BITS - 4 - index(I::BITS), M::BITS - 3 - index(I::BITS))
            }
        }
    }
}

impl<E: Environment, I: IntegerType, M: Magnitude> OutputMode<dyn ShrChecked<Integer<E, M>, Output = Integer<E, I>>>
    for Integer<E, I>
{
    type Case = (Mode, Mode);

    fn output_mode(case: &Self::Case) -> Mode {
        match (case.0, case.1) {
            (Mode::Constant, Mode::Constant) => Mode::Constant,
            (mode_a, Mode::Constant) => mode_a,
            (_, _) => Mode::Private,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use snarkvm_circuits_environment::Circuit;
    use snarkvm_utilities::{test_rng, UniformRand};
    use test_utilities::*;

    use core::{ops::RangeInclusive, panic::RefUnwindSafe};

    const ITERATIONS: usize = 32;

    fn check_shr<I: IntegerType + RefUnwindSafe, M: Magnitude + RefUnwindSafe>(
        name: &str,
        first: I,
        second: M,
        mode_a: Mode,
        mode_b: Mode,
    ) {
        let a = Integer::<Circuit, I>::new(mode_a, first);
        let b = Integer::<Circuit, M>::new(mode_b, second);
        match first.checked_shr(second.to_u32().unwrap()) {
            Some(expected) => Circuit::scope(name, || {
                let candidate = a.shr_checked(&b);
                assert_eq!(expected, candidate.eject_value());
                assert_count!(Integer<Circuit, I>, ShrChecked<Integer<Circuit, M>, Output=Integer<Circuit, I>>, &(mode_a, mode_b));
                assert_output_mode!(candidate, Integer<Circuit, I>, ShrChecked<Integer<Circuit, M>, Output=Integer<Circuit, I>>, &(mode_a, mode_b));
            }),
            None => match (mode_a, mode_b) {
                (_, Mode::Constant) => check_operation_halts(&a, &b, Integer::shr_checked),
                _ => Circuit::scope(name, || {
                    let _candidate = a.shr_checked(&b);
                    assert_count_fails!(Integer<Circuit, I>, ShrChecked<Integer<Circuit, M>, Output=Integer<Circuit, I>>, &(mode_a, mode_b));
                }),
            },
        };
        Circuit::reset();
    }

    fn run_test<I: IntegerType + RefUnwindSafe, M: Magnitude + RefUnwindSafe>(mode_a: Mode, mode_b: Mode) {
        for i in 0..ITERATIONS {
            let first: I = UniformRand::rand(&mut test_rng());
            let second: M = UniformRand::rand(&mut test_rng());

            let name = format!("Shr: {} >> {} {}", mode_a, mode_b, i);
            check_shr(&name, first, second, mode_a, mode_b);

            // Check that shift right by one is computed correctly.
            let name = format!("Half: {} >> {} {}", mode_a, mode_b, i);
            check_shr(&name, first, M::one(), mode_a, mode_b);
        }
    }

    fn run_exhaustive_test<I: IntegerType + RefUnwindSafe, M: Magnitude + RefUnwindSafe>(mode_a: Mode, mode_b: Mode)
    where
        RangeInclusive<I>: Iterator<Item = I>,
        RangeInclusive<M>: Iterator<Item = M>,
    {
        for first in I::MIN..=I::MAX {
            for second in M::MIN..=M::MAX {
                let name = format!("Shr: ({} >> {})", first, second);
                check_shr(&name, first, second, mode_a, mode_b);
            }
        }
    }

    test_integer_binary!(run_test, i8, u8, shr);
    test_integer_binary!(run_test, i8, u16, shr);
    test_integer_binary!(run_test, i8, u32, shr);

    test_integer_binary!(run_test, i16, u8, shr);
    test_integer_binary!(run_test, i16, u16, shr);
    test_integer_binary!(run_test, i16, u32, shr);

    test_integer_binary!(run_test, i32, u8, shr);
    test_integer_binary!(run_test, i32, u16, shr);
    test_integer_binary!(run_test, i32, u32, shr);

    test_integer_binary!(run_test, i64, u8, shr);
    test_integer_binary!(run_test, i64, u16, shr);
    test_integer_binary!(run_test, i64, u32, shr);

    test_integer_binary!(run_test, i128, u8, shr);
    test_integer_binary!(run_test, i128, u16, shr);
    test_integer_binary!(run_test, i128, u32, shr);

    test_integer_binary!(run_test, u8, u8, shr);
    test_integer_binary!(run_test, u8, u16, shr);
    test_integer_binary!(run_test, u8, u32, shr);

    test_integer_binary!(run_test, u16, u8, shr);
    test_integer_binary!(run_test, u16, u16, shr);
    test_integer_binary!(run_test, u16, u32, shr);

    test_integer_binary!(run_test, u32, u8, shr);
    test_integer_binary!(run_test, u32, u16, shr);
    test_integer_binary!(run_test, u32, u32, shr);

    test_integer_binary!(run_test, u64, u8, shr);
    test_integer_binary!(run_test, u64, u16, shr);
    test_integer_binary!(run_test, u64, u32, shr);

    test_integer_binary!(run_test, u128, u8, shr);
    test_integer_binary!(run_test, u128, u16, shr);
    test_integer_binary!(run_test, u128, u32, shr);

    test_integer_binary!(#[ignore], run_exhaustive_test, u8, u8, shr, exhaustive);
    test_integer_binary!(#[ignore], run_exhaustive_test, i8, u8, shr, exhaustive);
}
