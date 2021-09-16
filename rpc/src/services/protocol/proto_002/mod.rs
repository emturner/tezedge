// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

use serde::Deserialize;
use getset::CopyGetters;
use num::BigInt;

mod helpers;
pub(crate) mod rights_service;

use super::{vec_string_to_int, string_to_int};

#[derive(Debug, Deserialize, Clone, CopyGetters)]
pub(crate) struct ProtocolConstants {
    proof_of_work_nonce_size: u8,
    nonce_length: u8,
    max_revelations_per_block: u8,
    max_operation_data_length: i32,
    preserved_cycles: u8,

    #[get_copy = "pub(crate)"]
    blocks_per_cycle: i32,

    blocks_per_commitment: i32,
    blocks_per_roll_snapshot: i32,
    blocks_per_voting_period: i32,

    #[serde(with = "vec_string_to_int")]
    time_between_blocks: Vec<i64>,
    
    endorsers_per_block: u16,

    #[serde(with = "string_to_int")]
    origination_burn: BigInt,

    #[serde(with = "string_to_int")]
    hard_gas_limit_per_operation: BigInt,

    #[serde(with = "string_to_int")]
    hard_gas_limit_per_block: BigInt,

    #[serde(with = "string_to_int")]
    proof_of_work_threshold: BigInt,

    #[serde(with = "string_to_int")]
    tokens_per_roll: BigInt,
    michelson_maximum_type_size: u16,

    #[serde(with = "string_to_int")]
    seed_nonce_revelation_tip: BigInt,

    #[serde(with = "string_to_int")]
    block_security_deposit: BigInt,

    #[serde(with = "string_to_int")]
    endorsement_security_deposit: BigInt,

    #[serde(with = "string_to_int")]
    block_reward: BigInt,

    #[serde(with = "string_to_int")]
    endorsement_reward: BigInt,

    #[serde(with = "string_to_int")]
    cost_per_byte: BigInt,

    #[serde(with = "string_to_int")]
    hard_storage_limit_per_operation: BigInt,
}
