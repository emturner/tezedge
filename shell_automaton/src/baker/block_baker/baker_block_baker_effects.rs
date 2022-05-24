// Copyright (c) SimpleStaking, Viable Systems and Tezedge Contributors
// SPDX-License-Identifier: MIT

use tezos_api::ffi::ComputePathRequest;
use tezos_encoding::enc::BinWriter;
use tezos_messages::p2p::encoding::operations_for_blocks::{
    OperationsForBlock, OperationsForBlocksMessage,
};

use crate::block_applier::BlockApplierEnqueueBlockAction;
use crate::rights::rights_actions::RightsGetAction;
use crate::rights::RightsKey;
use crate::service::protocol_runner_service::ProtocolRunnerResult;
use crate::service::storage_service::StorageRequestPayload;
use crate::service::ProtocolRunnerService;
use crate::storage::request::{StorageRequestCreateAction, StorageRequestor};
use crate::{Action, ActionWithMeta, Service, Store};

use super::{
    BakerBlockBakerBakeNextLevelAction, BakerBlockBakerComputeOperationsPathsPendingAction,
    BakerBlockBakerComputeOperationsPathsSuccessAction, BakerBlockBakerPreapplyInitAction,
    BakerBlockBakerRightsGetCurrentLevelSuccessAction, BakerBlockBakerRightsGetInitAction,
    BakerBlockBakerRightsGetNextLevelSuccessAction, BakerBlockBakerRightsGetPendingAction,
    BakerBlockBakerRightsGetSuccessAction, BakerBlockBakerRightsNoRightsAction,
    BakerBlockBakerState, BakerBlockBakerTimeoutPendingAction,
};

pub fn baker_block_baker_effects<S>(store: &mut Store<S>, action: &ActionWithMeta)
where
    S: Service,
{
    match &action.action {
        Action::CurrentHeadRehydrated(_) | Action::CurrentHeadUpdate(_) => {
            store.dispatch(BakerBlockBakerRightsGetInitAction {});
        }
        Action::BakerBlockBakerRightsGetInit(_) => {
            let (head_level, head_hash) = match store.state().current_head.get() {
                Some(v) => (v.header.level(), v.hash.clone()),
                None => return,
            };

            let bakers = store.state().baker_keys_iter().cloned().collect::<Vec<_>>();
            for baker in bakers {
                store.dispatch(BakerBlockBakerRightsGetPendingAction { baker });
            }

            // TODO(zura): use baking rights instead.
            store.dispatch(RightsGetAction {
                key: RightsKey::endorsing(head_hash, Some(head_level + 1)),
            });
        }
        Action::RightsEndorsingReady(content) => {
            let head = match store.state().current_head.get() {
                Some(v) => v,
                None => return,
            };
            let is_level_current = content
                .key
                .level()
                .map_or(true, |level| level == head.header.level());
            let is_level_next = content
                .key
                .level()
                .map_or(false, |level| level == head.header.level() + 1);
            if content.key.block() != &head.hash || (!is_level_current && !is_level_next) {
                return;
            }
            let rights_level = if is_level_current {
                head.header.level()
            } else if is_level_next {
                head.header.level() + 1
            } else {
                return;
            };

            let rights = store.state().rights.cache.endorsing.get(&rights_level);
            let rights = match rights {
                Some((_, rights)) => rights,
                None => return,
            };
            let bakers_slots = store
                .state()
                .baker_keys_iter()
                .cloned()
                .map(|baker| {
                    let baker_key = baker.clone();
                    rights
                        .delegates
                        .get(&baker)
                        .map(|(first_slot, _)| (baker, vec![*first_slot]))
                        .unwrap_or((baker_key, vec![]))
                })
                .collect::<Vec<_>>();
            for (baker, slots) in bakers_slots {
                if is_level_current {
                    store.dispatch(BakerBlockBakerRightsGetCurrentLevelSuccessAction {
                        baker,
                        slots,
                    });
                } else {
                    store.dispatch(BakerBlockBakerRightsGetNextLevelSuccessAction { baker, slots });
                }
            }
        }
        Action::BakerBlockBakerRightsGetCurrentLevelSuccess(content) => {
            store.dispatch(BakerBlockBakerRightsNoRightsAction {
                baker: content.baker.clone(),
            });
            store.dispatch(BakerBlockBakerRightsGetSuccessAction {
                baker: content.baker.clone(),
            });
        }
        Action::BakerBlockBakerRightsGetNextLevelSuccess(content) => {
            store.dispatch(BakerBlockBakerRightsNoRightsAction {
                baker: content.baker.clone(),
            });
            store.dispatch(BakerBlockBakerRightsGetSuccessAction {
                baker: content.baker.clone(),
            });
        }
        Action::BakerBlockBakerRightsGetSuccess(content) => {
            store.dispatch(BakerBlockBakerTimeoutPendingAction {
                baker: content.baker.clone(),
            });
        }
        Action::MempoolQuorumReached(_) => {
            let bakers = store.state().baker_keys_iter().cloned().collect::<Vec<_>>();
            for baker in bakers {
                store.dispatch(BakerBlockBakerBakeNextLevelAction { baker });
            }
        }
        Action::BakerBlockBakerBakeNextLevel(content) => {
            store.dispatch(BakerBlockBakerPreapplyInitAction {
                baker: content.baker.clone(),
            });
        }
        Action::BakerBlockBakerBakeNextRound(content) => {
            store.dispatch(BakerBlockBakerPreapplyInitAction {
                baker: content.baker.clone(),
            });
        }
        Action::BakerBlockBakerPreapplyPending(content) => {
            let mut encoded_req = vec![];
            match content.request.bin_write(&mut encoded_req) {
                Ok(_) => {}
                Err(_) => return,
            }
        }
        Action::BakerBlockBakerComputeOperationsPathsInit(content) => {
            let baker_state = match store.state().bakers.get(&content.baker) {
                Some(v) => v,
                None => return,
            };
            let req = match &baker_state.block_baker {
                BakerBlockBakerState::ComputeOperationsPathsSuccess { operations, .. } => {
                    match ComputePathRequest::try_from(operations) {
                        Ok(v) => v,
                        Err(_) => return,
                    }
                }
                _ => return,
            };
            let token = store
                .service
                .protocol_runner()
                .compute_operations_paths(req);
            store.dispatch(BakerBlockBakerComputeOperationsPathsPendingAction {
                baker: content.baker.clone(),
                protocol_req_id: token,
            });
        }
        Action::BakerBlockBakerComputeOperationsPathsSuccess(content) => {
            let chain_id = store.state().config.chain_id.clone();
            let baker_state = match store.state().bakers.get(&content.baker) {
                Some(v) => v,
                None => return,
            };
            let (block, operations) = match &baker_state.block_baker {
                BakerBlockBakerState::ComputeOperationsPathsSuccess {
                    block, operations, ..
                } => (block.clone(), operations.clone()),
                _ => return,
            };
            let block_hash = block.hash.clone();
            store.dispatch(StorageRequestCreateAction {
                payload: StorageRequestPayload::BlockHeaderPut(chain_id, block),
                requestor: StorageRequestor::BakerBlockBaker(content.baker.clone()),
            });

            for (i, operations) in operations.iter().enumerate() {
                let ops = OperationsForBlocksMessage::new(
                    OperationsForBlock::new(block_hash.clone(), i as i8),
                    content.operations_paths[i].clone(),
                    operations.clone(),
                );
                store.dispatch(StorageRequestCreateAction {
                    payload: StorageRequestPayload::BlockOperationsPut(ops),
                    requestor: StorageRequestor::BakerBlockBaker(content.baker.clone()),
                });
            }

            store.dispatch(BlockApplierEnqueueBlockAction {
                block_hash: block_hash.into(),
                injector_rpc_id: None,
            });
        }
        Action::ProtocolRunnerResponse(content) => match &content.result {
            ProtocolRunnerResult::ComputeOperationsPaths((token, result)) => {
                let mut bakers_iter = store.state().bakers.iter();
                let baker = match bakers_iter
                    .find(|(_, b)| match &b.block_baker {
                        BakerBlockBakerState::ComputeOperationsPathsPending {
                            protocol_req_id,
                            ..
                        } => protocol_req_id == token,
                        _ => false,
                    })
                    .map(|(baker, _)| baker.clone())
                {
                    Some(v) => v,
                    None => return,
                };

                match result {
                    Ok(resp) => {
                        store.dispatch(BakerBlockBakerComputeOperationsPathsSuccessAction {
                            baker,
                            operations_paths: resp.operations_hashes_path.clone(),
                        });
                    }
                    Err(_) => {
                        todo!();
                    }
                }
            }
            _ => return,
        },
        _ => {}
    }
}
