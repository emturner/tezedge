// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

use std::collections::BTreeSet;

use crate::baker::{
    BakerState, ElectedBlock, CONSENSUS_COMMITTEE_SIZE, DELAY_INCREMENT_PER_ROUND,
    MINIMAL_BLOCK_DELAY,
};
use crate::block_applier::BlockApplierApplyState;
use crate::mempool::{MempoolState, OperationKind};
use crate::{Action, ActionWithMeta, State};

use super::{BakerBlockBakerState, BakingSlot};

fn set_elected_block_operations(baker: &mut BakerState, mempool: &MempoolState) {
    baker.elected_block = baker.elected_block.take().and_then(|mut block| {
        if !block.operations.is_empty() {
            return Some(block);
        }
        let applied = &mempool.validated_operations.applied;
        let applied = applied
            .iter()
            .map(|v| v.hash.clone())
            .collect::<BTreeSet<_>>();

        let ops = &mempool.validated_operations.ops;

        let empty_operations = vec![vec![], vec![], vec![], vec![]];
        let operations = applied
            .into_iter()
            .filter_map(|hash| Some((ops.get(&hash)?, hash)))
            .fold(empty_operations, |mut r, (op, hash)| {
                let container = match OperationKind::from_operation_content_raw(op.data().as_ref())
                {
                    OperationKind::Unknown
                    | OperationKind::Preendorsement
                    | OperationKind::FailingNoop
                    | OperationKind::EndorsementWithSlot => return r,
                    OperationKind::Endorsement => &mut r[0],
                    OperationKind::Proposals | OperationKind::Ballot => &mut r[1],
                    OperationKind::SeedNonceRevelation
                    | OperationKind::DoublePreendorsementEvidence
                    | OperationKind::DoubleEndorsementEvidence
                    | OperationKind::DoubleBakingEvidence
                    | OperationKind::ActivateAccount => {
                        // TODO(zura): do we need it???
                        // if op.signature.is_none() {
                        //     op.signature = Some(Signature(vec![0; 64]));
                        // }
                        &mut r[2]
                    }
                    OperationKind::Reveal
                    | OperationKind::Transaction
                    | OperationKind::Origination
                    | OperationKind::Delegation
                    | OperationKind::RegisterGlobalConstant
                    | OperationKind::SetDepositsLimit => &mut r[3],
                };
                container.push((op, hash));
                r
            });

        block.operations = operations
            .iter()
            .map(|ops| ops.into_iter().map(|(op, _)| (*op).clone()).collect())
            .collect();
        block.non_consensus_op_hashes = operations
            .into_iter()
            .skip(1)
            .flatten()
            .map(|(_, hash)| hash)
            .collect();

        Some(block)
    });
}

pub fn baker_block_baker_reducer(state: &mut State, action: &ActionWithMeta) {
    match &action.action {
        Action::BlockApplierApplySuccess(_) => {
            let new_block = match &state.block_applier.current {
                BlockApplierApplyState::Success { block, .. } => &*block,
                _ => return,
            };
            let mempool = &state.mempool;
            for (_, baker) in state.bakers.iter_mut() {
                if let Some(elected_block) = baker.elected_block.as_ref() {
                    if elected_block.header().level() < new_block.header.level() {
                        baker.elected_block = None;
                        continue;
                    }
                    set_elected_block_operations(baker, mempool);
                }
            }
        }
        Action::CurrentHeadRehydrated(_) | Action::CurrentHeadUpdate(_) => {
            for (_, baker) in state.bakers.iter_mut() {
                baker.block_baker = BakerBlockBakerState::Idle {
                    time: action.time_as_nanos(),
                };
                let head = state.current_head.get();
                baker.elected_block = baker.elected_block.take().and_then(|block| {
                    if block.header().level() < head?.header.level() {
                        None
                    } else {
                        Some(block)
                    }
                });
            }
        }
        Action::BakerBlockBakerRightsGetPending(content) => {
            if let Some(baker) = state.bakers.get_mut(&content.baker) {
                baker.block_baker = BakerBlockBakerState::RightsGetPending {
                    time: action.time_as_nanos(),
                    slots: None,
                    next_slots: None,
                };
            }
        }
        Action::BakerBlockBakerRightsGetCurrentLevelSuccess(content) => {
            if let Some(baker) = state.bakers.get_mut(&content.baker) {
                match &mut baker.block_baker {
                    BakerBlockBakerState::RightsGetPending { slots, .. } => {
                        *slots = Some(content.slots.clone());
                    }
                    _ => {}
                }
            }
        }
        Action::BakerBlockBakerRightsGetNextLevelSuccess(content) => {
            if let Some(baker) = state.bakers.get_mut(&content.baker) {
                match &mut baker.block_baker {
                    BakerBlockBakerState::RightsGetPending { next_slots, .. } => {
                        *next_slots = Some(content.slots.clone());
                    }
                    _ => {}
                }
            }
        }
        Action::BakerBlockBakerRightsGetSuccess(content) => {
            if let Some(baker) = state.bakers.get_mut(&content.baker) {
                match &mut baker.block_baker {
                    BakerBlockBakerState::RightsGetPending {
                        slots, next_slots, ..
                    } => match (slots, next_slots) {
                        (Some(slots), Some(next_slots)) => {
                            baker.block_baker = BakerBlockBakerState::RightsGetSuccess {
                                time: action.time_as_nanos(),
                                slots: std::mem::take(slots),
                                next_slots: std::mem::take(next_slots),
                            };
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
        Action::BakerBlockBakerRightsNoRights(content) => {
            if let Some(baker) = state.bakers.get_mut(&content.baker) {
                let time = action.time_as_nanos();
                baker.block_baker = BakerBlockBakerState::NoRights { time };
            }
        }
        Action::BakerBlockBakerTimeoutPending(content) => {
            if let Some(baker) = state.bakers.get_mut(&content.baker) {
                match &baker.block_baker {
                    BakerBlockBakerState::RightsGetSuccess {
                        slots, next_slots, ..
                    } => {
                        let round = match state.current_head.round() {
                            Some(v) => v,
                            None => return,
                        };
                        let current_slot = (round as u32 % CONSENSUS_COMMITTEE_SIZE) as u16;
                        let next_round = slots
                            .into_iter()
                            .map(|slot| *slot)
                            .find(|slot| *slot > current_slot)
                            .and_then(|slot| {
                                let pred = state.current_head.get()?;
                                let timestamp = pred.header.timestamp().as_u64();
                                let timestamp = timestamp * 1_000_000_000;

                                let rounds_left = slot.checked_sub(current_slot)? as i32;
                                let target_round = round + rounds_left;
                                let time_left =
                                    calc_time_until_round(round as u64, target_round as u64);
                                let timeout = timestamp + time_left;
                                Some(BakingSlot {
                                    round: target_round as u32,
                                    timeout,
                                })
                            });
                        let next_level = next_slots.get(0).cloned().and_then(|slot| {
                            let pred = baker
                                .elected_block_header_with_hash()
                                .or_else(|| state.current_head.get())?;
                            let timestamp = pred.header.timestamp().as_u64();
                            let timestamp = timestamp * 1_000_000_000;

                            let time_left = calc_time_until_round(round as u64, (round + 1) as u64)
                                + calc_time_until_round(0, slot as u64);
                            let timeout = timestamp + time_left;
                            Some(BakingSlot {
                                round: slot as u32,
                                timeout,
                            })
                        });

                        baker.block_baker = BakerBlockBakerState::TimeoutPending {
                            time: action.time_as_nanos(),
                            next_round,
                            next_level,
                        };
                    }
                    _ => {}
                }
            }
        }
        Action::BakerBlockBakerBakeNextRound(content) => {
            if let Some(baker) = state.bakers.get_mut(&content.baker) {
                match &baker.block_baker {
                    BakerBlockBakerState::TimeoutPending { next_round, .. } => {
                        let next_round = match next_round {
                            Some(v) => v,
                            None => return,
                        };
                        let timestamp = next_round.timeout / 1_000_000_000;
                        baker.block_baker = BakerBlockBakerState::BakeNextRound {
                            time: action.time_as_nanos(),
                            round: next_round.round,
                            block_timestamp: (timestamp as i64).into(),
                        };
                    }
                    _ => {}
                }
            }
        }
        Action::MempoolQuorumReached(_) => {
            let head = state.current_head.get();
            let payload_hash = state.current_head.payload_hash();
            state
                .bakers
                .iter_mut()
                .filter(|(_, baker_state)| baker_state.elected_block.is_none())
                .fold(None, |_, (_, baker_state)| {
                    let block = head?.clone();
                    let round = block.header.fitness().round()?;

                    baker_state.elected_block = Some(ElectedBlock {
                        block,
                        round,
                        payload_hash: payload_hash?.clone(),
                        operations: vec![],
                        non_consensus_op_hashes: vec![],
                    });
                    Some(())
                });
        }
        Action::BakerBlockBakerBakeNextLevel(content) => {
            if let Some(baker) = state.bakers.get_mut(&content.baker) {
                match &baker.block_baker {
                    BakerBlockBakerState::TimeoutPending { next_level, .. } => {
                        let next_level = match next_level {
                            Some(v) => v,
                            None => return,
                        };
                        let timestamp = next_level.timeout / 1_000_000_000;
                        baker.block_baker = BakerBlockBakerState::BakeNextLevel {
                            time: action.time_as_nanos(),
                            round: next_level.round,
                            block_timestamp: (timestamp as i64).into(),
                        };
                    }
                    _ => {}
                }
            }
        }
        Action::BakerBlockBakerPreapplySuccess(content) => {
            if let Some(baker) = state.bakers.get_mut(&content.baker) {
                match &baker.block_baker {
                    BakerBlockBakerState::PreapplyPending { .. } => {
                        baker.block_baker = BakerBlockBakerState::PreapplySuccess {
                            time: action.time_as_nanos(),
                            block: content.block.clone(),
                            operations: content.operations.clone(),
                        };
                    }
                    _ => {}
                }
            }
        }
        Action::BakerBlockBakerComputeOperationsPathsPending(content) => {
            if let Some(baker) = state.bakers.get_mut(&content.baker) {
                match &baker.block_baker {
                    BakerBlockBakerState::PreapplySuccess {
                        block, operations, ..
                    } => {
                        baker.block_baker = BakerBlockBakerState::ComputeOperationsPathsPending {
                            time: action.time_as_nanos(),
                            protocol_req_id: content.protocol_req_id,
                            block: block.clone(),
                            operations: operations.clone(),
                        };
                    }
                    _ => {}
                }
            }
        }
        Action::BakerBlockBakerComputeOperationsPathsSuccess(content) => {
            if let Some(baker) = state.bakers.get_mut(&content.baker) {
                match &baker.block_baker {
                    BakerBlockBakerState::PreapplySuccess {
                        block, operations, ..
                    } => {
                        baker.block_baker = BakerBlockBakerState::ComputeOperationsPathsSuccess {
                            time: action.time_as_nanos(),
                            block: block.clone(),
                            operations: operations.clone(),
                            operations_paths: content.operations_paths.clone(),
                        };
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn calc_seconds_until_round(current_round: u64, target_round: u64) -> u64 {
    // let current_slot = (current_round as u32 % CONSENSUS_COMMITTEE_SIZE) as u16;
    let rounds_left = target_round.saturating_sub(current_round);
    MINIMAL_BLOCK_DELAY * rounds_left
        + DELAY_INCREMENT_PER_ROUND * rounds_left * (current_round + target_round).saturating_sub(1)
            / 2
}

fn calc_time_until_round(current_round: u64, target_round: u64) -> u64 {
    calc_seconds_until_round(current_round, target_round) * 1_000_000_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calc_seconds_until_round() {
        assert_eq!(calc_seconds_until_round(0, 0), 0);
        assert_eq!(calc_seconds_until_round(0, 1), 15);
        assert_eq!(calc_seconds_until_round(0, 2), 35);
        assert_eq!(calc_seconds_until_round(0, 3), 60);
        assert_eq!(calc_seconds_until_round(0, 4), 90);
        assert_eq!(calc_seconds_until_round(0, 5), 125);

        assert_eq!(calc_seconds_until_round(1, 2), 20);
        assert_eq!(calc_seconds_until_round(1, 3), 45);
        assert_eq!(calc_seconds_until_round(1, 4), 75);
        assert_eq!(calc_seconds_until_round(1, 5), 110);

        assert_eq!(calc_seconds_until_round(2, 3), 25);
        assert_eq!(calc_seconds_until_round(2, 4), 55);
        assert_eq!(calc_seconds_until_round(2, 5), 90);

        assert_eq!(calc_seconds_until_round(3, 4), 30);
        assert_eq!(calc_seconds_until_round(3, 5), 65);

        assert_eq!(calc_seconds_until_round(4, 5), 35);
    }
}
