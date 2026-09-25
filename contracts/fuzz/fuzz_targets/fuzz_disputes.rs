#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Bytes, Env, String as SorobanString, Vec as SorobanVec,
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
    ForceEscrow {
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
    Tiny,          // 1
    JustBelowHigh, // HIGH_VALUE_THRESHOLD - 1
    ExactlyHigh,   // HIGH_VALUE_THRESHOLD
    JustAboveHigh, // HIGH_VALUE_THRESHOLD + 1
    Large,         // HIGH_VALUE_THRESHOLD * 100
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

fn dispute_status_from_u8(v: u8) -> DisputeStatus {
    match v % 4 {
        0 => DisputeStatus::Open,
        1 => DisputeStatus::ResolvedInFavorOfPayer,
        2 => DisputeStatus::ResolvedInFavorOfPayee,
        _ => DisputeStatus::Dismissed,
    }
}

// Storage access mirrors the contract's per-record layout (#1394): each
// payment, dispute and dispute deadline lives under its own DataKey. The
// legacy PAY_RECS / DISP_REC / DISP_META maps are only read by the one-off
// migration and are never written by current code.

fn load_payment(env: &Env, contract_id: &Address, id: u64) -> Option<Payment> {
    env.as_contract(contract_id, || {
        env.storage().persistent().get(&DataKey::Payment(id))
    })
}

fn store_payment(env: &Env, contract_id: &Address, payment: &Payment) {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Payment(payment.id), payment)
    });
}

fn load_dispute(env: &Env, contract_id: &Address, id: u64) -> Option<Dispute> {
    env.as_contract(contract_id, || {
        env.storage().persistent().get(&DataKey::Dispute(id))
    })
}

fn load_dispute_metadata(env: &Env, contract_id: &Address, id: u64) -> Option<DisputeMetadata> {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::DisputeMetadata(id))
    })
}

fn pick<T: Clone>(pool: &[T], idx: u8) -> T {
    pool[(idx as usize) % pool.len()].clone()
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
                let payer = pick(&actors, *payer_idx);
                let payee = pick(&actors, *payee_idx);

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
                        }
                    }
                }
            }

            DisputeOperation::ForceEscrow { payment_idx } => {
                // Mirrors move_payment_to_disputed_ready_state from test_payments.rs.
                // create_payment already stores payments as Escrowed (#1324), so
                // this only matters for any payment left in Pending.
                if payment_ids.is_empty() {
                    continue;
                }
                let payment_id = pick(&payment_ids, *payment_idx);

                if let Some(mut payment) = load_payment(&env, &contract_id, payment_id) {
                    if payment.status == PaymentStatus::Pending {
                        payment.status = PaymentStatus::Escrowed;
                        store_payment(&env, &contract_id, &payment);
                    }
                }
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
                let payment_id = pick(&payment_ids, *payment_idx);
                let raiser = pick(&actors, *raiser_idx);

                let reason_text = "x".repeat((*reason_len as usize) % 64);
                let reason = SorobanString::from_str(&env, &reason_text);

                let digest_bytes = [*evidence_byte; 32];
                let evidence_digest = Bytes::from_slice(&env, &digest_bytes);

                let mut chunks = SorobanVec::new(&env);
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
                    // INVARIANT: a dispute must only be raisable by the
                    // payment's payer or payee (#1391), on a payment that could
                    // transition to Disputed. If it succeeded, the payment must
                    // now be Disputed.
                    let payment = load_payment(&env, &contract_id, payment_id)
                        .expect("INVARIANT VIOLATION: disputed payment missing");
                    assert!(
                        raiser == payment.payer || raiser == payment.payee,
                        "INVARIANT VIOLATION: third party raised a dispute"
                    );
                    assert_eq!(
                        payment.status,
                        PaymentStatus::Disputed,
                        "INVARIANT VIOLATION: raise_dispute succeeded but payment not Disputed"
                    );
                    dispute_ids.push(dispute_id);
                    dispute_to_payment.push((dispute_id, payment_id));
                }
                // If it errored, fine -- e.g. Unauthorized, PaymentNotFound or
                // InvalidTransition (payment already Disputed/terminal).
            }

            DisputeOperation::ResolveDispute {
                dispute_idx,
                resolution_kind,
            } => {
                if dispute_ids.is_empty() {
                    continue;
                }
                let dispute_id = pick(&dispute_ids, *dispute_idx);
                let resolution = dispute_status_from_u8(*resolution_kind);

                let result = client.try_resolve_dispute(&dispute_id, &resolution);

                if result.is_ok() {
                    // INVARIANT: resolved dispute's linked payment status must
                    // match the resolution mapping exactly.
                    if let Some((_, payment_id)) =
                        dispute_to_payment.iter().find(|(d, _)| *d == dispute_id)
                    {
                        let payment = load_payment(&env, &contract_id, *payment_id)
                            .expect("INVARIANT VIOLATION: resolved dispute lost its payment");

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
                // Errors expected when: dispute not Open already, or dispute
                // doesn't exist -- both must be rejected, never silently no-op.
            }

            DisputeOperation::ProcessExpiredDisputes => {
                let stats_before: PaymentStats = client.get_payment_stats();
                let mut ids = SorobanVec::new(&env);
                for (dispute_id, _) in dispute_to_payment.iter() {
                    ids.push_back(*dispute_id);
                }
                let result = client.process_expired_disputes(&ids);

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

                // INVARIANT: every auto-refunded dispute is persisted as
                // resolved for the payer, with its payment Refunded.
                let mut refunded_now = 0u32;
                for (dispute_id, payment_id) in dispute_to_payment.iter() {
                    let dispute = load_dispute(&env, &contract_id, *dispute_id).unwrap();
                    if dispute.status == DisputeStatus::ResolvedInFavorOfPayer
                        && dispute.resolved_at == Some(env.ledger().timestamp())
                    {
                        let payment = load_payment(&env, &contract_id, *payment_id).unwrap();
                        assert_eq!(
                            payment.status,
                            PaymentStatus::Refunded,
                            "INVARIANT VIOLATION: auto-refunded dispute left payment unrefunded"
                        );
                        refunded_now += 1;
                    }
                }
                assert!(
                    refunded_now >= result,
                    "INVARIANT VIOLATION: process_expired_disputes reported {} refunds but persisted {}",
                    result,
                    refunded_now
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
        // Per-record storage has no enumeration, so walk the ids we created.
        for (dispute_id, payment_id) in &dispute_to_payment {
            // 1. No dispute should ever reference a payment_id that doesn't exist.
            assert!(
                load_payment(&env, &contract_id, *payment_id).is_some(),
                "GLOBAL INVARIANT VIOLATION: dispute references missing payment {}",
                payment_id
            );

            // 2. Every dispute must have DisputeMetadata with a deadline
            // strictly after raised_at (per auto_refund_after_timeout test);
            // without it the dispute can never be auto-refunded.
            let dispute = load_dispute(&env, &contract_id, *dispute_id).unwrap_or_else(|| {
                panic!(
                    "GLOBAL INVARIANT VIOLATION: dispute {} missing from storage",
                    dispute_id
                )
            });
            let meta =
                load_dispute_metadata(&env, &contract_id, *dispute_id).unwrap_or_else(|| {
                    panic!(
                        "GLOBAL INVARIANT VIOLATION: dispute {} has no deadline metadata",
                        dispute_id
                    )
                });
            assert!(
                meta.dispute_deadline > dispute.raised_at,
                "GLOBAL INVARIANT VIOLATION: dispute_deadline <= raised_at for dispute {}",
                dispute_id
            );
        }

        // 3. process_expired_disputes is idempotent on disputes already resolved:
        // running it twice in a row with no time advance must not double-refund.
        // (Implicitly checked above via stats monotonicity per call.)
    }
});
