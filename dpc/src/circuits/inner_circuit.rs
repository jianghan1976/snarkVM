// Copyright (C) 2019-2021 Aleo Systems Inc.
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

use crate::{ComputeKey, InnerPrivateVariables, InnerPublicVariables, Network, Payload};
use snarkvm_algorithms::traits::*;
use snarkvm_gadgets::{
    algorithms::merkle_tree::merkle_path::MerklePathGadget,
    bits::{Boolean, ToBytesGadget},
    integers::{int::Int64, uint::UInt8},
    traits::{
        algorithms::{CRHGadget, CommitmentGadget, EncryptionGadget, PRFGadget, SignatureGadget},
        alloc::AllocGadget,
        eq::{ConditionalEqGadget, EqGadget},
        integers::{add::Add, integer::Integer, sub::Sub},
    },
    ComparatorGadget,
    EvaluateLtGadget,
    ToConstraintFieldGadget,
};
use snarkvm_r1cs::{errors::SynthesisError, ConstraintSynthesizer, ConstraintSystem};
use snarkvm_utilities::{FromBytes, ToBytes};

use itertools::Itertools;

#[derive(Derivative)]
#[derivative(Clone(bound = "N: Network"))]
pub struct InnerCircuit<N: Network> {
    public: InnerPublicVariables<N>,
    private: InnerPrivateVariables<N>,
}

impl<N: Network> InnerCircuit<N> {
    pub fn blank() -> Self {
        Self {
            public: InnerPublicVariables::blank(),
            private: InnerPrivateVariables::blank(),
        }
    }

    pub fn new(public: InnerPublicVariables<N>, private: InnerPrivateVariables<N>) -> Self {
        Self { public, private }
    }
}

impl<N: Network> ConstraintSynthesizer<N::InnerScalarField> for InnerCircuit<N> {
    fn generate_constraints<CS: ConstraintSystem<N::InnerScalarField>>(
        &self,
        cs: &mut CS,
    ) -> Result<(), SynthesisError> {
        let public = &self.public;
        let private = &self.private;

        // In the inner circuit, this variable must be allocated as public input.
        debug_assert!(public.program_id.is_some());

        let (
            account_encryption_parameters,
            account_signature_parameters,
            record_commitment_parameters,
            ciphertext_id_crh,
            transition_id_crh,
            ledger_commitments_tree_crh,
            local_commitments_tree_crh,
            block_header_tree_parameters,
            block_hash_crh,
        ) = {
            let cs = &mut cs.ns(|| "Declare parameters");

            let account_encryption_parameters = N::AccountEncryptionGadget::alloc_constant(
                &mut cs.ns(|| "Declare account encryption parameters"),
                || Ok(N::account_encryption_scheme().clone()),
            )?;

            let account_signature_parameters = N::AccountSignatureGadget::alloc_constant(
                &mut cs.ns(|| "Declare account signature parameters"),
                || Ok(N::account_signature_scheme().clone()),
            )?;

            let record_commitment_parameters =
                N::CommitmentGadget::alloc_constant(&mut cs.ns(|| "Declare record commitment parameters"), || {
                    Ok(N::commitment_scheme().clone())
                })?;

            let ciphertext_id_crh_parameters = N::CiphertextIDCRHGadget::alloc_constant(
                &mut cs.ns(|| "Declare record ciphertext ID CRH parameters"),
                || Ok(N::ciphertext_id_crh().clone()),
            )?;

            let transition_id_crh = N::TransitionIDCRHGadget::alloc_constant(
                &mut cs.ns(|| "Declare transition ID CRH parameters"),
                || Ok(N::transition_id_crh().clone()),
            )?;

            let ledger_commitments_tree_crh = N::CommitmentsTreeCRHGadget::alloc_constant(
                &mut cs.ns(|| "Declare ledger commitments tree CRH parameters"),
                || Ok(N::commitments_tree_parameters().crh()),
            )?;

            let local_commitments_tree_crh = N::LocalCommitmentsTreeCRHGadget::alloc_constant(
                &mut cs.ns(|| "Declare local commitments tree CRH parameters"),
                || Ok(N::local_commitments_tree_parameters().crh()),
            )?;

            let block_header_tree_parameters = N::BlockHeaderTreeCRHGadget::alloc_constant(
                &mut cs.ns(|| "Declare block header tree CRH parameters"),
                || Ok(N::block_header_tree_parameters().crh()),
            )?;

            let block_hash_crh =
                N::BlockHashCRHGadget::alloc_constant(&mut cs.ns(|| "Declare block hash CRH parameters"), || {
                    Ok(N::block_hash_crh().clone())
                })?;

            (
                account_encryption_parameters,
                account_signature_parameters,
                record_commitment_parameters,
                ciphertext_id_crh_parameters,
                transition_id_crh,
                ledger_commitments_tree_crh,
                local_commitments_tree_crh,
                block_header_tree_parameters,
                block_hash_crh,
            )
        };

        // Declares a constant for a 0 value in a record.
        let zero_value = UInt8::constant_vec(&(0u64).to_bytes_le()?);
        // Declares a constant for an empty payload in a record.
        let empty_payload = UInt8::constant_vec(&Payload::default().to_bytes_le()?);
        // Declare the noop program ID as bytes.
        let noop_program_id_bytes = UInt8::constant_vec(&N::noop_program_id().to_bytes_le()?);

        let zero_value_field_elements =
            zero_value.to_constraint_field(&mut cs.ns(|| "convert zero value to field elements"))?;
        let empty_payload_field_elements =
            empty_payload.to_constraint_field(&mut cs.ns(|| "convert empty payload to field elements"))?;
        let noop_program_id_field_elements =
            noop_program_id_bytes.to_constraint_field(&mut cs.ns(|| "convert noop program ID to field elements"))?;

        let mut input_serial_numbers = Vec::with_capacity(N::NUM_INPUT_RECORDS);
        let mut input_serial_numbers_bytes = Vec::with_capacity(N::NUM_INPUT_RECORDS * 32); // Serial numbers are 32 bytes
        let mut input_commitments = Vec::with_capacity(N::NUM_INPUT_RECORDS);
        let mut input_commitments_bytes = Vec::with_capacity(N::NUM_INPUT_RECORDS);
        let mut input_owners = Vec::with_capacity(N::NUM_INPUT_RECORDS);
        let mut input_is_dummies = Vec::with_capacity(N::NUM_INPUT_RECORDS);
        let mut input_values = Vec::with_capacity(N::NUM_INPUT_RECORDS);
        let mut input_program_ids = Vec::with_capacity(N::NUM_INPUT_RECORDS);

        for (i, record) in private.input_records.iter().enumerate() {
            let cs = &mut cs.ns(|| format!("Process input record {}", i));

            // Declare record contents
            let (
                given_owner,
                given_is_dummy,
                given_value,
                given_payload,
                given_program_id,
                given_serial_number_nonce,
                given_commitment,
                given_commitment_randomness,
            ) = {
                let declare_cs = &mut cs.ns(|| "Declare input record");

                // No need to check that commitments, public keys and hashes are in
                // prime order subgroup because the commitment and CRH parameters
                // are trusted, and so when we recompute these, the newly computed
                // values will always be in correct subgroup. If the input cm, pk
                // or hash is incorrect, then it will not match the computed equivalent.

                let given_owner = <N::AccountSignatureGadget as SignatureGadget<
                    N::AccountSignatureScheme,
                    N::InnerScalarField,
                >>::PublicKeyGadget::alloc(
                    &mut declare_cs.ns(|| "given_record_owner"), || Ok(*record.owner())
                )?;

                let given_is_dummy = Boolean::alloc(&mut declare_cs.ns(|| "given_is_dummy"), || Ok(record.is_dummy()))?;

                let given_value = Int64::alloc(&mut declare_cs.ns(|| "given_value"), || Ok(record.value() as i64))?;

                let given_payload =
                    UInt8::alloc_vec(&mut declare_cs.ns(|| "given_payload"), &record.payload().to_bytes_le()?)?;

                let given_program_id = UInt8::alloc_vec(
                    &mut declare_cs.ns(|| "given_program_id"),
                    &record.program_id().to_bytes_le()?,
                )?;

                let given_serial_number_nonce =
                    <N::SerialNumberPRFGadget as PRFGadget<N::SerialNumberPRF, N::InnerScalarField>>::Input::alloc(
                        &mut declare_cs.ns(|| "given_serial_number_nonce"),
                        || Ok(vec![record.serial_number_nonce().clone()]),
                    )?;

                let given_commitment = <N::CommitmentGadget as CommitmentGadget<
                    N::CommitmentScheme,
                    N::InnerScalarField,
                >>::OutputGadget::alloc(
                    &mut declare_cs.ns(|| "given_commitment"), || Ok(record.commitment())
                )?;

                let given_commitment_randomness = <N::CommitmentGadget as CommitmentGadget<
                    N::CommitmentScheme,
                    N::InnerScalarField,
                >>::RandomnessGadget::alloc(
                    &mut declare_cs.ns(|| "given_commitment_randomness"),
                    || Ok(record.commitment_randomness()),
                )?;

                (
                    given_owner,
                    given_is_dummy,
                    given_value,
                    given_payload,
                    given_program_id,
                    given_serial_number_nonce,
                    given_commitment,
                    given_commitment_randomness,
                )
            };

            // ********************************************************************
            // Check that the serial number is derived correctly.
            // ********************************************************************
            {
                let sn_cs = &mut cs.ns(|| "Check that sn is derived correctly");

                // TODO (howardwu): CRITICAL - Review the translation from scalar to base field of `sk_prf`.
                // Allocate sk_prf.
                let sk_prf = {
                    let compute_key = ComputeKey::<N>::from_signature(&private.signature)
                        .expect("Failed to derive the compute key from signature");
                    FromBytes::read_le(&compute_key.sk_prf().to_bytes_le()?[..])?
                };

                let sk_prf =
                    <N::SerialNumberPRFGadget as PRFGadget<N::SerialNumberPRF, N::InnerScalarField>>::Seed::alloc(
                        &mut sn_cs.ns(|| "Declare sk_prf"),
                        || Ok(&sk_prf),
                    )?;

                let candidate_serial_number = <N::SerialNumberPRFGadget as PRFGadget<
                    N::SerialNumberPRF,
                    N::InnerScalarField,
                >>::check_evaluation_gadget(
                    &mut sn_cs.ns(|| "Compute serial number"),
                    &sk_prf,
                    &given_serial_number_nonce,
                )?;

                // Convert input serial numbers to bytes.
                let candidate_serial_number_bytes = candidate_serial_number
                    .to_bytes(&mut sn_cs.ns(|| format!("Convert {}-th serial number to bytes", i)))?;

                input_serial_numbers.push(candidate_serial_number);
                input_serial_numbers_bytes.extend_from_slice(&candidate_serial_number_bytes);
            };

            // *******************************************************************
            // Check that the record is well-formed.
            // *******************************************************************
            {
                let commitment_cs = &mut cs.ns(|| "Check that record is well-formed");

                let given_value_bytes =
                    given_value.to_bytes(&mut commitment_cs.ns(|| "Convert given_value to bytes"))?;

                // Perform noop safety checks.
                {
                    let given_value_field_elements = given_value_bytes
                        .to_constraint_field(&mut commitment_cs.ns(|| "convert given value to field elements"))?;
                    let given_payload_field_elements = given_payload
                        .to_constraint_field(&mut commitment_cs.ns(|| "convert given payload to field elements"))?;
                    let given_program_id_field_elements = given_program_id
                        .to_constraint_field(&mut commitment_cs.ns(|| "convert given program ID to field elements"))?;

                    given_value_field_elements.conditional_enforce_equal(
                        &mut commitment_cs
                            .ns(|| format!("If the input record {} is empty, enforce it has a value of 0", i)),
                        &zero_value_field_elements,
                        &given_is_dummy,
                    )?;
                    given_payload_field_elements.conditional_enforce_equal(
                        &mut commitment_cs
                            .ns(|| format!("If the input record {} is empty, enforce it has an empty payload", i)),
                        &empty_payload_field_elements,
                        &given_is_dummy,
                    )?;
                    given_program_id_field_elements.conditional_enforce_equal(
                        &mut commitment_cs
                            .ns(|| format!("If the input record {} is empty, enforce it has a noop program ID", i)),
                        &noop_program_id_field_elements,
                        &given_is_dummy,
                    )?;

                    input_program_ids.push(given_program_id_field_elements);
                }

                // Compute the record commitment and check that it matches the declared commitment.
                let given_owner_bytes =
                    given_owner.to_bytes(&mut commitment_cs.ns(|| "Convert record_owner to bytes"))?;
                let given_is_dummy_bytes =
                    given_is_dummy.to_bytes(&mut commitment_cs.ns(|| "Convert is_dummy to bytes"))?;
                let given_serial_number_nonce_bytes = given_serial_number_nonce
                    .to_bytes(&mut commitment_cs.ns(|| "Convert given_serial_number_nonce to bytes"))?;

                let mut commitment_input = Vec::new();
                commitment_input.extend_from_slice(&given_owner_bytes);
                commitment_input.extend_from_slice(&given_is_dummy_bytes);
                commitment_input.extend_from_slice(&given_value_bytes);
                commitment_input.extend_from_slice(&given_payload);
                commitment_input.extend_from_slice(&given_program_id);
                commitment_input.extend_from_slice(&given_serial_number_nonce_bytes);

                let candidate_commitment = record_commitment_parameters.check_commitment_gadget(
                    &mut commitment_cs.ns(|| "Compute commitment"),
                    &commitment_input,
                    &given_commitment_randomness,
                )?;

                candidate_commitment.enforce_equal(
                    &mut commitment_cs.ns(|| "Check that declared and computed commitments are equal"),
                    &given_commitment,
                )?;

                let candidate_commitment_bytes =
                    candidate_commitment.to_bytes(&mut commitment_cs.ns(|| "Convert candidate_commitment to bytes"))?;

                input_owners.push(given_owner);
                input_commitments.push(candidate_commitment);
                input_commitments_bytes.extend_from_slice(&candidate_commitment_bytes);
                input_is_dummies.push(given_is_dummy);
                input_values.push(given_value);
            }
        }

        // *******************************************************************
        // Check that the signature is valid.
        // *******************************************************************
        {
            let signature_cs = &mut cs.ns(|| "Check that the signature is valid");

            // TODO (howardwu): TEMPORARY - Enforce that the input owners are the same address.

            let signature_gadget = <N::AccountSignatureGadget as SignatureGadget<
                N::AccountSignatureScheme,
                N::InnerScalarField,
            >>::SignatureGadget::alloc(
                signature_cs.ns(|| "alloc_signature"), || Ok(&private.signature)
            )?;

            let mut signature_message = Vec::new();
            signature_message.extend_from_slice(&input_commitments_bytes);
            // signature_message.extend_from_slice(&inputs_digest);
            // signature_message.extend_from_slice(&fee);

            let signature_verification = account_signature_parameters.verify(
                signature_cs.ns(|| "signature_verify"),
                &input_owners[0],
                &signature_message,
                &signature_gadget,
            )?;

            signature_verification.enforce_equal(signature_cs.ns(|| "check_verification"), &Boolean::constant(true))?;
        }

        // **********************************************************************************
        // Check that the commitment appears on the ledger or prior transition,
        // i.e., the membership witness is valid with respect to the record commitment root.
        // **********************************************************************************
        let (candidate_block_hash_bytes, local_commitments_root_bytes) = {
            let ledger_cs = &mut cs.ns(|| "Check ledger proof");

            // Declare the ledger commitments root.
            let ledger_commitments_root = <N::CommitmentsTreeCRHGadget as CRHGadget<_, _>>::OutputGadget::alloc(
                &mut ledger_cs.ns(|| "Declare ledger commitments root"),
                || Ok(private.ledger_proof.commitments_root()),
            )?;

            // Declare the local commitments root.
            let local_commitments_root = <N::LocalCommitmentsTreeCRHGadget as CRHGadget<_, _>>::OutputGadget::alloc(
                &mut ledger_cs.ns(|| "Declare local commitments root"),
                || Ok(private.local_proof.local_commitments_root()),
            )?;

            // Ensure each commitment is either 1) in the ledger, 2) from a prior local transition, or 3) a dummy.
            for (i, (commitment, is_dummy)) in input_commitments
                .iter()
                .zip_eq(input_is_dummies.iter())
                .take(N::NUM_INPUT_RECORDS)
                .enumerate()
            {
                let inclusion_cs = &mut ledger_cs.ns(|| format!("Check commitment inclusion proof {}", i));

                let ledger_commitment_inclusion_proof = MerklePathGadget::<_, N::CommitmentsTreeCRHGadget, _>::alloc(
                    &mut inclusion_cs.ns(|| "Declare ledger commitment inclusion proof"),
                    || Ok(&private.ledger_proof.commitment_inclusion_proofs()[i]),
                )?;

                let local_commitment_inclusion_proof =
                    MerklePathGadget::<_, N::LocalCommitmentsTreeCRHGadget, _>::alloc(
                        &mut inclusion_cs.ns(|| "Declare local commitment inclusion proof"),
                        || Ok(&private.local_proof.commitment_inclusion_proofs()[i]),
                    )?;

                let candidate_ledger_commitments_root = ledger_commitment_inclusion_proof.calculate_root(
                    &mut inclusion_cs.ns(|| "calculate_root for candidate_ledger_commitments_root"),
                    &ledger_commitments_tree_crh,
                    &commitment,
                )?;

                let candidate_local_commitments_root = local_commitment_inclusion_proof.calculate_root(
                    &mut inclusion_cs.ns(|| "calculate_root for candidate_local_commitments_root"),
                    &local_commitments_tree_crh,
                    &commitment,
                )?;

                // First, check if the commitment is in the ledger commitments tree.
                let is_ledger = candidate_ledger_commitments_root.is_eq(
                    &mut inclusion_cs.ns(|| "ledger commitments root is_eq"),
                    &ledger_commitments_root,
                )?;

                // Second, check if the commitment is in the local commitments tree.
                let is_local = candidate_local_commitments_root.is_eq(
                    &mut inclusion_cs.ns(|| "local commitments root is_eq"),
                    &local_commitments_root,
                )?;

                // Determine if the commitment exists in either the ledger or local.
                let is_real_commitment = Boolean::or(
                    &mut inclusion_cs.ns(|| "Determine if the commitment exists in either the ledger or local"),
                    &is_ledger,
                    &is_local,
                )?;

                // Ensure it is consistent with the record's is_dummy flag.
                is_real_commitment.enforce_equal(&mut inclusion_cs.ns(|| "is_real_commitment"), &is_dummy.not())?;
            }

            // Ensure ledger commitments root exists in the claimed block.
            let candidate_block_hash_bytes = {
                // Declare the block header root.
                let header_root = <N::BlockHeaderTreeCRHGadget as CRHGadget<_, _>>::OutputGadget::alloc(
                    &mut ledger_cs.ns(|| "Declare block header root"),
                    || Ok(private.ledger_proof.header_root()),
                )?;

                // Ensure the header inclusion proof is valid.
                let header_inclusion_proof = MerklePathGadget::<_, N::BlockHeaderTreeCRHGadget, _>::alloc(
                    &mut ledger_cs.ns(|| "Declare block header inclusion proof"),
                    || Ok(private.ledger_proof.header_inclusion_proof()),
                )?;
                header_inclusion_proof.check_membership(
                    &mut ledger_cs.ns(|| "Perform block header inclusion proof check"),
                    &block_header_tree_parameters,
                    &header_root,
                    &ledger_commitments_root,
                )?;

                // Declare the previous block hash.
                let previous_block_hash = UInt8::alloc_vec(
                    &mut ledger_cs.ns(|| "Allocate network id"),
                    &private.ledger_proof.previous_block_hash().to_bytes_le()?,
                )?;

                // Construct the block hash preimage.
                let mut preimage = Vec::new();
                preimage.extend_from_slice(&previous_block_hash);
                preimage.extend_from_slice(&header_root.to_bytes(&mut ledger_cs.ns(|| "header_root"))?);

                let candidate_block_hash =
                    block_hash_crh.check_evaluation_gadget(&mut ledger_cs.ns(|| "Compute the block hash"), preimage)?;

                let given_block_hash =
                    <N::BlockHashCRHGadget as CRHGadget<N::BlockHashCRH, N::InnerScalarField>>::OutputGadget::alloc(
                        &mut ledger_cs.ns(|| "Allocate given block hash"),
                        || Ok(private.ledger_proof.block_hash()),
                    )?;

                // Ensure the block hash is valid.
                candidate_block_hash.enforce_equal(
                    &mut ledger_cs.ns(|| "Check that the block hash is valid"),
                    &given_block_hash,
                )?;

                candidate_block_hash.to_bytes(&mut ledger_cs.ns(|| "Convert block hash to bytes"))?
            };

            let local_commitments_root_bytes =
                local_commitments_root.to_bytes(&mut ledger_cs.ns(|| "Convert local commitments root to bytes"))?;

            (candidate_block_hash_bytes, local_commitments_root_bytes)
        };
        // ********************************************************************

        let mut output_commitments_bytes = Vec::with_capacity(N::NUM_OUTPUT_RECORDS * 32); // Commitments are 32 bytes
        let mut output_values = Vec::with_capacity(N::NUM_OUTPUT_RECORDS);
        let mut output_program_ids = Vec::with_capacity(N::NUM_OUTPUT_RECORDS);
        let mut ciphertext_ids_bytes = Vec::with_capacity(N::NUM_OUTPUT_RECORDS * 32);

        for (j, (record, encryption_randomness)) in private
            .output_records
            .iter()
            .zip_eq(&private.ciphertext_randomizers)
            .enumerate()
        {
            let cs = &mut cs.ns(|| format!("Process output record {}", j));

            let (
                given_owner,
                given_is_dummy,
                given_value,
                given_payload,
                given_program_id,
                given_serial_number_nonce,
                given_serial_number_nonce_bytes,
                given_commitment,
                given_commitment_randomness,
            ) = {
                let declare_cs = &mut cs.ns(|| "Declare output record");

                let given_owner = <N::AccountEncryptionGadget as EncryptionGadget<
                    N::AccountEncryptionScheme,
                    N::InnerScalarField,
                >>::PublicKeyGadget::alloc(
                    &mut declare_cs.ns(|| "given_record_owner"), || Ok(*record.owner())
                )?;

                let given_is_dummy = Boolean::alloc(&mut declare_cs.ns(|| "given_is_dummy"), || Ok(record.is_dummy()))?;

                let given_value = Int64::alloc(&mut declare_cs.ns(|| "given_value"), || Ok(record.value() as i64))?;

                let given_payload =
                    UInt8::alloc_vec(&mut declare_cs.ns(|| "given_payload"), &record.payload().to_bytes_le()?)?;

                let given_program_id = UInt8::alloc_vec(
                    &mut declare_cs.ns(|| "given_program_id"),
                    &record.program_id().to_bytes_le()?,
                )?;

                let given_serial_number_nonce =
                    <N::SerialNumberPRFGadget as PRFGadget<N::SerialNumberPRF, N::InnerScalarField>>::Output::alloc(
                        &mut declare_cs.ns(|| "given_serial_number_nonce"),
                        || Ok(record.serial_number_nonce()),
                    )?;

                let given_serial_number_nonce_bytes =
                    given_serial_number_nonce.to_bytes(&mut declare_cs.ns(|| "Convert sn nonce to bytes"))?;

                let given_commitment = <N::CommitmentGadget as CommitmentGadget<
                    N::CommitmentScheme,
                    N::InnerScalarField,
                >>::OutputGadget::alloc(
                    &mut declare_cs.ns(|| "record_commitment"), || Ok(record.commitment())
                )?;

                let given_commitment_randomness = <N::CommitmentGadget as CommitmentGadget<
                    N::CommitmentScheme,
                    N::InnerScalarField,
                >>::RandomnessGadget::alloc(
                    &mut declare_cs.ns(|| "given_commitment_randomness"),
                    || Ok(record.commitment_randomness()),
                )?;

                (
                    given_owner,
                    given_is_dummy,
                    given_value,
                    given_payload,
                    given_program_id,
                    given_serial_number_nonce,
                    given_serial_number_nonce_bytes,
                    given_commitment,
                    given_commitment_randomness,
                )
            };
            // ********************************************************************

            // *******************************************************************
            // Check that the serial number nonce is correct.
            // *******************************************************************
            {
                let sn_cs = &mut cs.ns(|| "Check that serial number nonce is correct");

                let candidate_serial_number_nonce = &input_serial_numbers[j];

                candidate_serial_number_nonce.enforce_equal(
                    &mut sn_cs.ns(|| "Check that computed nonce matches provided nonce"),
                    &given_serial_number_nonce,
                )?;
            }
            // *******************************************************************

            // *******************************************************************
            // Check that the record is well-formed.
            // *******************************************************************
            let given_value_bytes = {
                let commitment_cs = &mut cs.ns(|| "Check that record is well-formed");

                let given_value_bytes =
                    given_value.to_bytes(&mut commitment_cs.ns(|| "Convert given_value to bytes"))?;

                // Perform noop safety checks.
                {
                    let given_value_field_elements = given_value_bytes
                        .to_constraint_field(&mut commitment_cs.ns(|| "convert given value to field elements"))?;
                    let given_payload_field_elements = given_payload
                        .to_constraint_field(&mut commitment_cs.ns(|| "convert given payload to field elements"))?;
                    let given_program_id_field_elements = given_program_id
                        .to_constraint_field(&mut commitment_cs.ns(|| "convert given program ID to field elements"))?;

                    given_value_field_elements.conditional_enforce_equal(
                        &mut commitment_cs
                            .ns(|| format!("If the output record {} is empty, enforce it has a value of 0", j)),
                        &zero_value_field_elements,
                        &given_is_dummy,
                    )?;
                    given_payload_field_elements.conditional_enforce_equal(
                        &mut commitment_cs
                            .ns(|| format!("If the output record {} is empty, enforce it has an empty payload", j)),
                        &empty_payload_field_elements,
                        &given_is_dummy,
                    )?;
                    given_program_id_field_elements.conditional_enforce_equal(
                        &mut commitment_cs
                            .ns(|| format!("If the output record {} is empty, enforce it has a noop program ID", j)),
                        &noop_program_id_field_elements,
                        &given_is_dummy,
                    )?;

                    output_program_ids.push(given_program_id_field_elements);
                }

                // Compute the record commitment and check that it matches the declared commitment.
                let given_owner_bytes =
                    given_owner.to_bytes(&mut commitment_cs.ns(|| "Convert record_owner to bytes"))?;
                let given_is_dummy_bytes =
                    given_is_dummy.to_bytes(&mut commitment_cs.ns(|| "Convert is_dummy to bytes"))?;

                let mut commitment_input = Vec::new();
                commitment_input.extend_from_slice(&given_owner_bytes);
                commitment_input.extend_from_slice(&given_is_dummy_bytes);
                commitment_input.extend_from_slice(&given_value_bytes);
                commitment_input.extend_from_slice(&given_payload);
                commitment_input.extend_from_slice(&given_program_id);
                commitment_input.extend_from_slice(&given_serial_number_nonce_bytes);

                let candidate_commitment = record_commitment_parameters.check_commitment_gadget(
                    &mut commitment_cs.ns(|| "Compute record commitment"),
                    &commitment_input,
                    &given_commitment_randomness,
                )?;
                candidate_commitment.enforce_equal(
                    &mut commitment_cs.ns(|| "Check that computed commitment matches public input"),
                    &given_commitment,
                )?;

                output_commitments_bytes
                    .extend_from_slice(&candidate_commitment.to_bytes(&mut commitment_cs.ns(|| "commitment_bytes"))?);
                output_values.push(given_value);

                given_value_bytes
            };

            // *******************************************************************

            // *******************************************************************
            // Check that the record encryption is well-formed.
            // *******************************************************************
            {
                let encryption_cs = &mut cs.ns(|| "Check that record encryption is well-formed");

                // *******************************************************************
                // Convert program id, value, payload, serial number nonce, and commitment randomness into bits.

                let plaintext_bytes = {
                    // Commitment randomness
                    let given_commitment_randomness_bytes = given_commitment_randomness
                        .to_bytes(&mut encryption_cs.ns(|| "Convert commitment randomness to bytes"))?;

                    let mut res = vec![];
                    res.extend_from_slice(&given_value_bytes);
                    res.extend_from_slice(&given_payload);
                    res.extend_from_slice(&given_program_id);
                    res.extend_from_slice(&given_serial_number_nonce_bytes);
                    res.extend_from_slice(&given_commitment_randomness_bytes);
                    res
                };

                // *******************************************************************
                // Compute the record ciphertext and ciphertext ID.

                let encryption_randomness_gadget = <N::AccountEncryptionGadget as EncryptionGadget<
                    N::AccountEncryptionScheme,
                    N::InnerScalarField,
                >>::RandomnessGadget::alloc(
                    &mut encryption_cs.ns(|| format!("output record {} encryption_randomness", j)),
                    || Ok(encryption_randomness),
                )?;

                let candidate_encrypted_record_gadget = account_encryption_parameters.check_encryption_gadget(
                    &mut encryption_cs.ns(|| format!("output record {} check_encryption_gadget", j)),
                    &encryption_randomness_gadget,
                    &given_owner,
                    &plaintext_bytes,
                )?;

                let candidate_encrypted_record_id = ciphertext_id_crh.check_evaluation_gadget(
                    &mut encryption_cs.ns(|| format!("Compute encrypted record ID {}", j)),
                    candidate_encrypted_record_gadget,
                )?;

                ciphertext_ids_bytes.extend_from_slice(
                    &candidate_encrypted_record_id
                        .to_bytes(&mut encryption_cs.ns(|| "Convert ciphertext ID to bytes"))?,
                );
            }
        }
        // *******************************************************************

        // *******************************************************************
        // Check that program ID is declared by the input and output records.
        // *******************************************************************
        {
            let program_cs = &mut cs.ns(|| "Check that program ID is well-formed");

            // Allocate the program ID.
            let executable_program_id_field_elements = {
                let executable_program_id_bytes = UInt8::alloc_input_vec_le(
                    &mut program_cs.ns(|| "Allocate executable_program_id"),
                    &public.program_id.as_ref().unwrap().to_bytes_le()?,
                )?;
                executable_program_id_bytes
                    .to_constraint_field(&mut program_cs.ns(|| "convert executable program ID to field elements"))?
            };

            // Declare the required number of inputs for this function type.
            let number_of_inputs =
                &UInt8::alloc_vec(&mut program_cs.ns(|| "number_of_inputs for executable"), &[private
                    .function_type
                    .input_count()])?[0];
            {
                let number_of_input_records = UInt8::constant(N::NUM_INPUT_RECORDS as u8);
                let is_inputs_size_correct = number_of_inputs.less_than_or_equal(
                    &mut program_cs.ns(|| "Check number of inputs is less than or equal to input records size"),
                    &number_of_input_records,
                )?;
                is_inputs_size_correct.enforce_equal(
                    &mut program_cs.ns(|| "Enforce number of inputs is less than or equal to input records size"),
                    &Boolean::constant(true),
                )?;
            }

            // Declare the required number of outputs for this function type.
            let number_of_outputs =
                &UInt8::alloc_vec(&mut program_cs.ns(|| "number_of_outputs for executable"), &[private
                    .function_type
                    .output_count()])?[0];
            {
                let number_of_output_records = UInt8::constant(N::NUM_OUTPUT_RECORDS as u8);
                let is_outputs_size_correct = number_of_outputs.less_than_or_equal(
                    &mut program_cs.ns(|| "Check number of outputs is less than or equal to output records size"),
                    &number_of_output_records,
                )?;
                is_outputs_size_correct.enforce_equal(
                    &mut program_cs.ns(|| "Enforce number of outputs is less than or equal to output records size"),
                    &Boolean::constant(true),
                )?;
            }

            for (i, input_program_id_field_elements) in input_program_ids.iter().take(N::NUM_INPUT_RECORDS).enumerate()
            {
                let input_cs = &mut program_cs.ns(|| format!("Check input record {} on executable", i));

                let input_index = UInt8::constant(i as u8);

                let requires_check = input_index.less_than(
                    &mut input_cs.ns(|| format!("less than for input {}", i)),
                    &number_of_inputs,
                )?;

                input_program_id_field_elements.conditional_enforce_equal(
                    &mut input_cs.ns(|| format!("Check input program ID, if not dummy - {}", i)),
                    &executable_program_id_field_elements,
                    &requires_check,
                )?;

                input_program_id_field_elements.conditional_enforce_equal(
                    &mut input_cs
                        .ns(|| format!("If the input record {} is beyond, enforce it has a noop program ID", i)),
                    &noop_program_id_field_elements,
                    &requires_check.not(),
                )?;
            }

            for (j, output_program_id_field_elements) in
                output_program_ids.iter().take(N::NUM_OUTPUT_RECORDS).enumerate()
            {
                let output_cs = &mut program_cs.ns(|| format!("Check output record {} on executable", j));

                let output_index = UInt8::constant(j as u8);

                let requires_check = output_index.less_than(
                    &mut output_cs.ns(|| format!("less than for output {}", j)),
                    &number_of_outputs,
                )?;

                output_program_id_field_elements.conditional_enforce_equal(
                    &mut output_cs.ns(|| format!("Check output program ID, if not dummy - {}", j)),
                    &executable_program_id_field_elements,
                    &requires_check,
                )?;

                output_program_id_field_elements.conditional_enforce_equal(
                    &mut output_cs
                        .ns(|| format!("If the output record {} is beyond, enforce it has a noop program ID", j)),
                    &noop_program_id_field_elements,
                    &requires_check.not(),
                )?;
            }
        }
        // ********************************************************************

        // *******************************************************************
        // Check that the value balance is valid.
        // *******************************************************************
        let candidate_value_balance = {
            let mut cs = cs.ns(|| "Check that the value balance is valid.");

            let mut candidate_value_balance = Int64::zero();

            for (i, input_value) in input_values.iter().enumerate() {
                candidate_value_balance = candidate_value_balance
                    .add(cs.ns(|| format!("add input record {} value", i)), &input_value)
                    .unwrap();
            }

            for (j, output_value) in output_values.iter().enumerate() {
                candidate_value_balance = candidate_value_balance
                    .sub(cs.ns(|| format!("sub output record {} value", j)), &output_value)
                    .unwrap();
            }

            candidate_value_balance
        };

        // ********************************************************************
        // Check the transition ID is well-formed.
        // ********************************************************************
        {
            let mut cs = cs.ns(|| "Check that the transition ID is valid.");

            // Encode the preimage for the transition ID.
            let mut message = Vec::new();
            message.extend_from_slice(&candidate_block_hash_bytes);
            message.extend_from_slice(&local_commitments_root_bytes);
            message.extend_from_slice(&input_serial_numbers_bytes);
            message.extend_from_slice(&output_commitments_bytes);
            message.extend_from_slice(&ciphertext_ids_bytes);
            message.extend_from_slice(&candidate_value_balance.to_bytes(&mut cs.ns(|| "value_balance_bytes"))?);

            let candidate_transition_id = transition_id_crh
                .check_evaluation_gadget(&mut cs.ns(|| "Compute the transition ID"), message.clone())?;

            let given_transition_id = <N::TransitionIDCRHGadget as CRHGadget<
                N::TransitionIDCRH,
                N::InnerScalarField,
            >>::OutputGadget::alloc_input(
                &mut cs.ns(|| "Allocate given transition ID"),
                || Ok(public.transition_id()),
            )?;

            candidate_transition_id
                .enforce_equal(&mut cs.ns(|| "Check that transition ID is valid"), &given_transition_id)?;
        }

        Ok(())
    }
}