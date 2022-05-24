// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

use serde::{Deserialize, Serialize};

use crypto::hash::{BlockPayloadHash, NonceHash};
use storage::BlockHeaderWithHash;
use tezos_encoding::enc::BinWriter;
use tezos_encoding::types::SizedBytes;
use tezos_messages::p2p::encoding::operation::Operation;
use tezos_messages::p2p::encoding::operations_for_blocks::Path;
use tezos_messages::Timestamp;

use crate::protocol_runner::ProtocolRunnerToken;

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct BakingSlot {
    pub round: u32,
    pub timeout: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum BakerBlockBakerState {
    Idle {
        time: u64,
    },
    RightsGetPending {
        time: u64,
        /// Slots for current level.
        slots: Option<Vec<u16>>,
        /// Slots for next level.
        next_slots: Option<Vec<u16>>,
    },
    RightsGetSuccess {
        time: u64,
        /// Slots for current level.
        slots: Vec<u16>,
        /// Slots for next level.
        next_slots: Vec<u16>,
    },
    NoRights {
        time: u64,
    },
    /// Waiting until current level/round times out and until it's time
    /// for us to bake a block.
    TimeoutPending {
        time: u64,
        /// Slot for current level's next round that we can bake.
        next_round: Option<BakingSlot>,
        /// Slots for next level's next round that we can bake.
        next_level: Option<BakingSlot>,
    },
    /// Previous round didn't reach the quorum, or we aren't baker of
    /// the next level and we haven't seen next level block yet, so
    /// it's time to bake next round.
    BakeNextRound {
        time: u64,
        round: u32,
        block_timestamp: Timestamp,
    },
    /// Previous round did reach the quorum, so bake the next level.
    BakeNextLevel {
        time: u64,
        round: u32,
        block_timestamp: Timestamp,
    },
    PreapplyPending {
        time: u64,
        request: BlockPreapplyRequest,
    },
    PreapplySuccess {
        time: u64,
        block: BlockHeaderWithHash,
        operations: Vec<Vec<Operation>>,
    },
    ComputeOperationsPathsPending {
        time: u64,
        protocol_req_id: ProtocolRunnerToken,
        block: BlockHeaderWithHash,
        operations: Vec<Vec<Operation>>,
    },
    ComputeOperationsPathsSuccess {
        time: u64,
        block: BlockHeaderWithHash,
        operations: Vec<Vec<Operation>>,
        operations_paths: Vec<Path>,
    },
}

impl BakerBlockBakerState {
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle { .. })
    }
}

#[derive(BinWriter, Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "fuzzing", derive(fuzzcheck::DefaultMutator))]
pub struct BlockPreapplyRequest {
    pub payload_hash: BlockPayloadHash,
    pub payload_round: i32,
    #[cfg_attr(feature = "fuzzing", field_mutator(SizedBytesMutator<8>))]
    pub proof_of_work_nonce: SizedBytes<8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed_nonce_hash: Option<NonceHash>,
    pub liquidity_baking_escape_vote: bool,
    // skip with bin_write.
    pub timestamp: i64,
    // add dummy signature in bin_write.
    pub operations: Vec<Vec<Operation>>,
}
