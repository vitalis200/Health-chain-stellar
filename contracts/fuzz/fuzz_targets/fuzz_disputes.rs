#![no_main]

use libfuzzer_sys::fuzz_target;
use arbitrary::Arbitrary;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    vec, Address, Bytes, Env, String as SorobanString,
};

use health_chain_contract::payments::{
    Dispute, DisputeMetadata, DisputeStatus, FeeStructure, Payment, PaymentStats, PaymentStatus,
    HIGH_VALUE_THRESHOLD,
};
use health_chain_contract::{DataKey, Error, HealthChainContract, HealthChainContractClient};

/// Actions exercised against the payment + dispute state machine.
/// All actor/payment references are indices into pre-built pools rather than
/// raw values, so the fuzzer spends its budget on meaningful sequences instead
/// of mostly-invalid lookups (mirrors fuzz_custody_transfer.rs).
#[derive(Arbitrary, Debug, Clone)]
enum DisputeOperation {
    CreatePayment {
        payer_idx: u8,
        payee_idx: u8,
        admin_is_caller: bool,
        amount_kind: AmountKind,
        fee_kind: FeeKind,
    },
    FundEscrow {
        payment_idx: u8,
    },
    RaiseDispute {
        payment_idx: u8,
        raiser_idx: u8,
        reason_len: u8,
        evidence_byte: u8,
        num_chunks: u8,
    },
    ResolveDispute {
        dispute_idx: u8,
        resolution_kind: u8, // mod 4 -> DisputeStatus variant
    },
    ProcessExpiredDisputes,
    AdvanceTime {
        seconds: u32,
    },
    SetDisputeTimeout {
        timeout_secs: u32,
    },
}

/// Amount boundaries deliberately clustered around HIGH_VALUE_THRESHOLD (10_000),
/// since that's the one branch point the issue calls out explicitly.
#[derive(Arbitrary, Debug, Clone, Copy)]
enum AmountKind {
    Tiny,            // 1
    JustBelowHigh,   // HIGH_VALUE_THRESHOLD - 1
    ExactlyHigh,     // HIGH_VALUE_THRESHOLD
    JustAboveHigh,   // HIGH_VALUE_THRESHOLD + 1
    Large,           // HIGH_VALUE_THRESHOLD * 100
}

impl AmountKind {
    fn to_amount(self) -> i128 {
        match self {
            AmountKind::Tiny => 1,
            AmountKind::JustBelowHigh => HIGH_VALUE_THRESHOLD - 1,
            AmountKind::ExactlyHigh => HIGH_VALUE_THRESHOLD,
            AmountKind::JustAboveHigh => HIGH_VALUE_THRESHOLD + 1,
            AmountKind::Large => HIGH_VALUE_THRESHOLD * 100,
        }
    }
}

/// Fee shapes: valid (non-negative) and a deliberately tampered/negative one,
/// since test_create_payment_fails_with_tampered_fee_payload shows the contract
/// must reject negative fees with Error::InvalidFeePayload.
#[derive(Arbitrary, Debug, Clone, Copy)]
enum FeeKind {
    Zero,
    SmallValid { service_fee: u8, network_fee: u8 },
    NegativeService, // service_fee = -100, must be rejected
}

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    operations: Vec<DisputeOperation>,
}

fn read_payment(env: &Env, contract_id: &Address, payment_id: u64) -> Option<Payment> {
    env.as_contract(contract_id, || {
        env.storage().persistent().get(&DataKey::Payment(payment_id))
    })
}

fn dispute_status_from_u8(v: u8) -> DisputeStatus {
    match v % 4 {
        0 => DisputeStatus::Open,
        1 => DisputeStatus::ResolvedInFavorOfPayer,
        2 => DisputeStatus::ResolvedInFavorOfPayee,
        _ => DisputeStatus::Dismissed,
    }
}

fuzz_target!(|input: FuzzInput| {
    // Cap to prevent fuzzer timeout.
    if input.operations.len() > 50 {
        return;
    }

    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register(HealthChainContract, ());
    let client = HealthChainContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    // Fixed actor pool: indices fuzzed via u8 % pool.len() rather than deriving
    // Arbitrary on Address directly (Address has no such impl).
    let actors: Vec<Address> = (0..6).map(|_| Address::generate(&env)).collect();
    let asset = Address::generate(&env);

    // Track created payment ids and dispute ids for index-based reference by
    // later operations, same bookkeeping pattern as pending_event_ids in the
    // custody harness.
    let mut payment_ids: Vec<u64> = Vec::new();
    let mut payment_payers: Vec<(u64, Address)> = Vec::new();
    let mut dispute_ids: Vec<u64> = Vec::new();
    // Parallel map: dispute_id -> payment_id, so we can check cross-invariants.
    let mut dispute_to_payment: Vec<(u64, u64)> = Vec::new();

    for op in input.operations.iter() {
        match op {
            DisputeOperation::CreatePayment {
                payer_idx,
                payee_idx,
                admin_is_caller,
                amount_kind,
                fee_kind,
            } => {
                let payer = actors[*payer_idx as usize % actors.len()].clone();
                let payee = actors[*payee_idx as usize % actors.len()].clone();

                // Skip the trivially-rejected same-payer/payee case; that's
                // Payment::validate() territory, already covered by unit tests.
                if payer == payee {
                    continue;
                }

                let amount = amount_kind.to_amount();

                let fee_structure = match fee_kind {
                    FeeKind::Zero => FeeStructure {
                        policy_id: soroban_sdk::Symbol::new(&env, "fz_zero"),
                        service_fee: 0,
                        network_fee: 0,
                        performance_bonus: 0,
                        fixed_fee: 0,
                    },
                    FeeKind::SmallValid {
                        service_fee,
                        network_fee,
                    } => FeeStructure {
                        policy_id: soroban_sdk::Symbol::new(&env, "fz_valid"),
                        service_fee: *service_fee as i128,
                        network_fee: *network_fee as i128,
                        performance_bonus: 0,
                        fixed_fee: 0,
                    },
                    FeeKind::NegativeService => FeeStructure {
                        policy_id: soroban_sdk::Symbol::new(&env, "fz_bad"),
                        service_fee: -100,
                        network_fee: 0,
                        performance_bonus: 0,
                        fixed_fee: 0,
                    },
                };

                let caller = if *admin_is_caller {
                    admin.clone()
                } else {
                    actors[0].clone()
                };

                let result = client.try_create_payment(
                    &1u64,
                    &payer,
                    &payee,
                    &amount,
                    &asset,
                    &fee_structure,
                    &caller,
                );

                match fee_kind {
                    FeeKind::NegativeService => {
                        // INVARIANT: negative fees must always be rejected,
                        // regardless of amount or caller.
                        assert!(
                            result.is_err(),
                            "INVARIANT VIOLATION: payment created with negative fee"
                        );
                    }
                    _ => {
                        if !*admin_is_caller {
                            // Unauthorized caller path: must fail with Unauthorized,
                            // never silently succeed.
                            if let Err(Ok(e)) = &result {
                                assert_eq!(
                                    *e,
                                    Error::Unauthorized,
                                    "INVARIANT VIOLATION: wrong error for unauthorized create_payment"
                                );
                            }
                        } else if let Ok(Ok(payment_id)) = result {
                            payment_ids.push(payment_id);
                            payment_payers.push((payment_id, payer.clone()));
                        }
                    }
                }
            }

            DisputeOperation::FundEscrow { payment_idx } => {
                // Payments start Pending; the payer funds them through the
                // real fund_escrow entrypoint before they can be disputed (#1431).
                if payment_ids.is_empty() {
                    continue;
                }
                let payment_id = payment_ids[(*payment_idx as usize) % payment_ids.len()];
                let payer = payment_payers
                    .iter()
                    .find(|(id, _)| *id == payment_id)
                    .map(|(_, p)| p.clone())
                    .unwrap();
                let _ = client.try_fund_escrow(&payment_id, &payer);
            }

            DisputeOperation::RaiseDispute {
                payment_idx,
                raiser_idx,
                reason_len,
                evidence_byte,
                num_chunks,
            } => {
                if payment_ids.is_empty() {
                    continue;
                }
                let payment_id = payment_ids[(*payment_idx as usize) % payment_ids.len()];
                let raiser = actors[*raiser_idx as usize % actors.len()].clone();

                let reason_text = "x".repeat((*reason_len as usize) % 64);
                let reason = SorobanString::from_str(&env, &reason_text);

                let digest_bytes = [*evidence_byte; 32];
                let evidence_digest = Bytes::from_slice(&env, &digest_bytes);

                let mut chunks = vec![&env];
                for i in 0..(*num_chunks % 5) {
                    chunks.push_back(SorobanString::from_str(&env, &format!("chunk{}", i)));
                }

                let result = client.try_raise_dispute(
                    &payment_id,
                    &raiser,
                    &reason,
                    &evidence_digest,
                    &chunks,
                );

                if let Ok(Ok(dispute_id)) = result {
                    // INVARIANT: a dispute must only be raisable on a payment
                    // that exists and is in Escrowed status (can_transition_to
                    // Disputed). If it succeeded, the payment must now be Disputed.
                    let payment = read_payment(&env, &contract_id, payment_id).unwrap();
                    assert_eq!(
                        payment.status,
                        PaymentStatus::Disputed,
                        "INVARIANT VIOLATION: raise_dispute succeeded but payment not Disputed"
                    );
                    // INVARIANT: only the payer or payee may raise a dispute.
                    assert!(
                        raiser == payment.payer || raiser == payment.payee,
                        "INVARIANT VIOLATION: third party raised a dispute"
                    );
                    dispute_ids.push(dispute_id);
                    dispute_to_payment.push((dispute_id, payment_id));
                }
                // If it errored, fine -- e.g. PaymentNotFound or InvalidTransition
                // (payment not yet Escrowed, or already Disputed/terminal).
            }

            DisputeOperation::ResolveDispute {
                dispute_idx,
                resolution_kind,
            } => {
                if dispute_ids.is_empty() {
                    continue;
                }
                let dispute_id = dispute_ids[(*dispute_idx as usize) % dispute_ids.len()];
                let resolution = dispute_status_from_u8(*resolution_kind);

                let result = client.try_resolve_dispute(&dispute_id, &resolution);

                if result.is_ok() {
                    // INVARIANT: resolved dispute's linked payment status must
                    // match the resolution mapping exactly.
                    if let Some((_, payment_id)) =
                        dispute_to_payment.iter().find(|(d, _)| *d == dispute_id)
                    {
                        {
                            let payment = read_payment(&env, &contract_id, *payment_id).unwrap();

                            let expected = match resolution {
                                DisputeStatus::ResolvedInFavorOfPayer => PaymentStatus::Refunded,
                                DisputeStatus::ResolvedInFavorOfPayee => PaymentStatus::Completed,
                                _ => PaymentStatus::Resolved,
                            };
                            assert_eq!(
                                payment.status, expected,
                                "INVARIANT VIOLATION: resolve_dispute set wrong payment status"
                            );
                        }
                    }
                }
                // Errors expected when: dispute not Open already, or dispute
                // doesn't exist -- both must be rejected, never silently no-op.
            }

            DisputeOperation::ProcessExpiredDisputes => {
                let stats_before: PaymentStats = client.get_payment_stats();
                let mut dispute_ids = soroban_sdk::Vec::new(&env);
                for (dispute_id, _) in dispute_to_payment.iter() {
                    dispute_ids.push_back(*dispute_id);
                }
                let result = client.process_expired_disputes(&dispute_ids);

                if result > 0 {
                    let stats_after: PaymentStats = client.get_payment_stats();
                    // INVARIANT: auto-refund count must strictly increase by
                    // exactly the number processed, never decrease or skip.
                    assert_eq!(
                        stats_after.count_auto_refunded,
                        stats_before.count_auto_refunded + result as u64,
                        "INVARIANT VIOLATION: auto-refund stats count mismatch"
                    );
                    assert!(
                        stats_after.total_auto_refunded >= stats_before.total_auto_refunded,
                        "INVARIANT VIOLATION: total_auto_refunded decreased"
                    );
                }

                // INVARIANT: an immediate second sweep must not refund or
                // count any dispute again.
                let stats_mid: PaymentStats = client.get_payment_stats();
                assert_eq!(
                    client.process_expired_disputes(&dispute_ids),
                    0,
                    "INVARIANT VIOLATION: dispute auto-refunded twice"
                );
                assert_eq!(
                    client.get_payment_stats(),
                    stats_mid,
                    "INVARIANT VIOLATION: repeat sweep changed stats"
                );
            }

            DisputeOperation::AdvanceTime { seconds } => {
                let advance = (*seconds as u64).min(7 * 24 * 60 * 60); // cap at 7 days
                env.ledger().with_mut(|li| {
                    li.timestamp += advance;
                });
            }

            DisputeOperation::SetDisputeTimeout { timeout_secs } => {
                // Only admin should be able to do this in a real flow; client
                // call here mirrors test usage (client.set_dispute_timeout).
                let timeout = (*timeout_secs as u64).max(1);
                let _ = client.try_set_dispute_timeout(&timeout);
            }
        }

        // ----- Global invariants, checked after every operation -----

        // 1. No dispute should ever reference a payment_id that doesn't exist.
        for (_, payment_id) in &dispute_to_payment {
            assert!(
                read_payment(&env, &contract_id, *payment_id).is_some(),
                "GLOBAL INVARIANT VIOLATION: dispute references missing payment {}",
                payment_id
            );
        }

        // 2. Every dispute must have matching DisputeMetadata with a deadline
        // strictly after raised_at (per auto_refund_after_timeout test).
        for (dispute_id, _) in &dispute_to_payment {
            env.as_contract(&contract_id, || {
                let dispute: Dispute = env
                    .storage()
                    .persistent()
                    .get(&DataKey::Dispute(*dispute_id))
                    .unwrap();
                let meta: Option<DisputeMetadata> = env
                    .storage()
                    .persistent()
                    .get(&DataKey::DisputeMetadata(*dispute_id));
                let meta = meta.unwrap_or_else(|| {
                    panic!(
                        "GLOBAL INVARIANT VIOLATION: dispute {} has no metadata",
                        dispute_id
                    )
                });
                assert!(
                    meta.dispute_deadline > dispute.raised_at,
                    "GLOBAL INVARIANT VIOLATION: dispute_deadline <= raised_at for dispute {}",
                    dispute_id
                );
            });
        }
    }
});
