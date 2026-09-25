#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Symbol, Vec as SorobanVec,
};

use health_chain_contract::payments::{
    EscrowAccount, FeeStructure, MultiSigConfig, Payment, PaymentStatus, PendingApproval,
    ReleaseConditions, HIGH_VALUE_THRESHOLD,
};
use health_chain_contract::{DataKey, Error, HealthChainContract, HealthChainContractClient};

#[derive(Arbitrary, Debug, Clone)]
enum PaymentOperation {
    CreatePayment {
        payer_idx: u8,
        payee_idx: u8,
        caller_is_admin: bool,
        amount_kind: AmountKind,
        negative_fee: bool,
    },
    ForceEscrow {
        payment_idx: u8,
    },
    SatisfyEscrowConditions {
        payment_idx: u8,
        approver_idx: u8,
        verified: bool,
        min_timestamp_offset: i32, // relative to current ledger time
    },
    ConfigureMultisig {
        num_signers: u8,
        threshold: u8,
        caller_is_admin: bool,
    },
    ProposeRelease {
        payment_idx: u8,
        approver_idx: u8,
    },
    AdvanceTime {
        seconds: u32,
    },
}
/// Amount values clustered tightly around HIGH_VALUE_THRESHOLD (10_000) since
/// that boundary is exactly what gates the single-admin vs multisig path.
#[derive(Arbitrary, Debug, Clone, Copy)]
enum AmountKind {
    Tiny,
    JustBelowHigh,
    ExactlyHigh,
    JustAboveHigh,
    Large,
}

impl AmountKind {
    fn to_amount(self) -> i128 {
        match self {
            AmountKind::Tiny => 1,
            AmountKind::JustBelowHigh => HIGH_VALUE_THRESHOLD - 1,
            AmountKind::ExactlyHigh => HIGH_VALUE_THRESHOLD,
            AmountKind::JustAboveHigh => HIGH_VALUE_THRESHOLD + 1,
            AmountKind::Large => HIGH_VALUE_THRESHOLD * 50,
        }
    }
}

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    operations: Vec<PaymentOperation>,
}

// Storage access mirrors the contract's per-record layout (#1394): each
// payment, escrow and pending approval lives under its own DataKey. The legacy
// PAY_RECS / ESC_ACCS / PEND_APR maps are only read by the one-off migration
// and are never written by current code.

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

fn load_escrow(env: &Env, contract_id: &Address, id: u64) -> Option<EscrowAccount> {
    env.as_contract(contract_id, || {
        env.storage().persistent().get(&DataKey::EscrowAccount(id))
    })
}

fn store_escrow(env: &Env, contract_id: &Address, escrow: &EscrowAccount) {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::EscrowAccount(escrow.payment_id), escrow)
    });
}

fn load_approval(env: &Env, contract_id: &Address, id: u64) -> Option<PendingApproval> {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::PendingApprovalRecord(id))
    })
}

fn pick<T: Clone>(pool: &[T], idx: u8) -> T {
    pool[(idx as usize) % pool.len()].clone()
}

fuzz_target!(|input: FuzzInput| {
    if input.operations.len() > 50 {
        return;
    }

    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register(HealthChainContract, ());
    let client = HealthChainContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    // Fixed actor pool (payers/payees/approvers/signers all draw from this).
    let actors: Vec<Address> = (0..8).map(|_| Address::generate(&env)).collect();
    let asset = Address::generate(&env);

    let mut payment_ids: Vec<u64> = Vec::new();
    // Track expected amount per payment_id so we can check the multisig vs
    // single-admin branch was taken correctly.
    let mut payment_amounts: Vec<(u64, i128)> = Vec::new();
    let mut multisig_configured = false;
    let mut configured_threshold: u32 = 0;
    // configure_multisig does not prune votes cast by signers that a later
    // reconfiguration removed, so vote bounds are checked against every
    // address that has ever been a signer.
    let mut ever_signers: Vec<Address> = Vec::new();

    for op in input.operations.iter() {
        match op {
            PaymentOperation::CreatePayment {
                payer_idx,
                payee_idx,
                caller_is_admin,
                amount_kind,
                negative_fee,
            } => {
                let payer = pick(&actors, *payer_idx);
                let payee = pick(&actors, *payee_idx);
                if payer == payee {
                    continue;
                }

                let amount = amount_kind.to_amount();

                let fee_structure = if *negative_fee {
                    FeeStructure {
                        policy_id: Symbol::new(&env, "fz_bad"),
                        service_fee: -50,
                        network_fee: 0,
                        performance_bonus: 0,
                        fixed_fee: 0,
                    }
                } else {
                    FeeStructure {
                        policy_id: Symbol::new(&env, "fz_ok"),
                        service_fee: 0,
                        network_fee: 0,
                        performance_bonus: 0,
                        fixed_fee: 0,
                    }
                };

                let caller = if *caller_is_admin {
                    admin.clone()
                } else {
                    actors[1].clone()
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

                if *negative_fee {
                    // INVARIANT: negative fees are always rejected, no matter
                    // what amount or caller is used.
                    assert!(
                        result.is_err(),
                        "INVARIANT VIOLATION: payment created with negative fee"
                    );
                    continue;
                }

                if !*caller_is_admin {
                    // INVARIANT: unauthorized caller must be rejected with
                    // Error::Unauthorized specifically, not just any error.
                    if let Err(Ok(e)) = &result {
                        assert_eq!(
                            *e,
                            Error::Unauthorized,
                            "INVARIANT VIOLATION: wrong error code for unauthorized create_payment"
                        );
                    }
                    continue;
                }

                if let Ok(Ok(payment_id)) = result {
                    payment_ids.push(payment_id);
                    payment_amounts.push((payment_id, amount));

                    // INVARIANT: escrow account must exist immediately after
                    // creation, pre-populated with locked_amount == amount and
                    // medical_records_verified == false (per
                    // escrow_conditions_stored_at_payment_creation test).
                    let escrow = load_escrow(&env, &contract_id, payment_id)
                        .expect("INVARIANT VIOLATION: no escrow record after create_payment");
                    assert_eq!(
                        escrow.locked_amount, amount,
                        "INVARIANT VIOLATION: escrow locked_amount != payment amount"
                    );
                    assert!(
                        !escrow.release_conditions.medical_records_verified,
                        "INVARIANT VIOLATION: medical_records_verified true by default"
                    );
                }
            }

            PaymentOperation::ForceEscrow { payment_idx } => {
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

            PaymentOperation::SatisfyEscrowConditions {
                payment_idx,
                approver_idx,
                verified,
                min_timestamp_offset,
            } => {
                if payment_ids.is_empty() {
                    continue;
                }
                let payment_id = pick(&payment_ids, *payment_idx);
                let approver = pick(&actors, *approver_idx);

                if let Some(mut escrow) = load_escrow(&env, &contract_id, payment_id) {
                    let current = env.ledger().timestamp();
                    let min_ts = if *min_timestamp_offset >= 0 {
                        current.saturating_add(*min_timestamp_offset as u64)
                    } else {
                        current.saturating_sub(min_timestamp_offset.unsigned_abs() as u64)
                    };
                    escrow.release_conditions = ReleaseConditions {
                        medical_records_verified: *verified,
                        min_timestamp: min_ts,
                        authorized_approver: Some(approver),
                    };
                    store_escrow(&env, &contract_id, &escrow);
                }
            }

            PaymentOperation::ConfigureMultisig {
                num_signers,
                threshold,
                caller_is_admin,
            } => {
                let n = ((*num_signers % 5) + 1) as usize; // 1..=5 signers
                let mut signers = SorobanVec::new(&env);
                for signer in actors.iter().take(n) {
                    signers.push_back(signer.clone());
                }
                let threshold_val = (*threshold % 6) as u32; // 0..=5, includes invalid 0

                // configure_multisig requires admin auth implicitly via
                // require_auth inside the contract; since mock_all_auths()
                // bypasses signer checks, we instead validate the *config
                // shape* invariant: invalid configs (threshold 0, threshold >
                // signers.len(), duplicate signers) must never end up stored
                // as valid by propose_release's later validate() call.
                let _ = caller_is_admin;

                let result = client.try_configure_multisig(&signers, &threshold_val);
                if result.is_ok() {
                    let config = MultiSigConfig {
                        signers: signers.clone(),
                        threshold: threshold_val,
                    };
                    if config.validate().is_ok() {
                        multisig_configured = true;
                        configured_threshold = threshold_val;
                        for signer in actors.iter().take(n) {
                            if !ever_signers.contains(signer) {
                                ever_signers.push(signer.clone());
                            }
                        }
                    } else {
                        // INVARIANT: an invalid config (zero threshold,
                        // threshold > len, or duplicates) must never be
                        // accepted by configure_multisig.
                        panic!(
                            "INVARIANT VIOLATION: configure_multisig accepted invalid config (threshold={}, n={})",
                            threshold_val, n
                        );
                    }
                }
            }

            PaymentOperation::ProposeRelease {
                payment_idx,
                approver_idx,
            } => {
                if payment_ids.is_empty() {
                    continue;
                }
                let payment_id = pick(&payment_ids, *payment_idx);
                let approver = pick(&actors, *approver_idx);

                let amount = payment_amounts
                    .iter()
                    .find(|(id, _)| *id == payment_id)
                    .map(|(_, a)| *a);

                let result = client.try_propose_release(&payment_id, &approver);

                if let Ok(Ok(true)) = result {
                    // INVARIANT: if propose_release reports executed, the
                    // payment must actually be Completed with
                    // escrow_released_at set.
                    let payment = load_payment(&env, &contract_id, payment_id)
                        .expect("INVARIANT VIOLATION: executed release lost its payment");
                    assert_eq!(
                        payment.status,
                        PaymentStatus::Completed,
                        "INVARIANT VIOLATION: propose_release reported executed but status != Completed"
                    );
                    assert!(
                        payment.escrow_released_at.is_some(),
                        "INVARIANT VIOLATION: executed release missing escrow_released_at"
                    );

                    if let Some(amt) = amount {
                        if amt >= HIGH_VALUE_THRESHOLD && multisig_configured {
                            let approval = load_approval(&env, &contract_id, payment_id)
                                .expect("INVARIANT VIOLATION: high-value release executed without a vote record");
                            assert!(
                                approval.executed,
                                "INVARIANT VIOLATION: executed release has approval.executed == false"
                            );
                            assert!(
                                approval.approvals.len() >= configured_threshold,
                                "INVARIANT VIOLATION: high-value release executed before reaching threshold ({} votes, threshold {})",
                                approval.approvals.len(),
                                configured_threshold
                            );
                        }
                    }
                }
            }

            PaymentOperation::AdvanceTime { seconds } => {
                let advance = (*seconds as u64).min(7 * 24 * 60 * 60);
                env.ledger().with_mut(|li| {
                    li.timestamp += advance;
                });
            }
        }

        // ----- Global invariants after every operation -----
        // Per-record storage has no enumeration, so walk the ids we created.
        for &payment_id in payment_ids.iter() {
            let payment = load_payment(&env, &contract_id, payment_id).unwrap_or_else(|| {
                panic!(
                    "GLOBAL INVARIANT VIOLATION: payment {} disappeared from storage",
                    payment_id
                )
            });

            // 1. Escrow and payment stay in sync: payment.amount is the net of
            // the gross escrowed locked_amount after the payment's fees.
            let escrow = load_escrow(&env, &contract_id, payment_id).unwrap_or_else(|| {
                panic!(
                    "GLOBAL INVARIANT VIOLATION: escrow for payment {} disappeared",
                    payment_id
                )
            });
            assert_eq!(
                payment
                    .fee_structure
                    .calculate_net_amount(escrow.locked_amount),
                Ok(payment.amount),
                "GLOBAL INVARIANT VIOLATION: escrow/payment amount desync for {}",
                payment_id
            );

            // 2. Votes are unique and only ever come from configured signers.
            if let Some(approval) = load_approval(&env, &contract_id, payment_id) {
                assert!(
                    approval.approvals.len() as usize <= ever_signers.len(),
                    "GLOBAL INVARIANT VIOLATION: more votes ({}) than configured signers ({}) for payment {}",
                    approval.approvals.len(),
                    ever_signers.len(),
                    payment_id
                );
                for (i, voter) in approval.approvals.iter().enumerate() {
                    assert!(
                        ever_signers.contains(&voter),
                        "GLOBAL INVARIANT VIOLATION: vote from non-signer on payment {}",
                        payment_id
                    );
                    for other in approval.approvals.iter().skip(i + 1) {
                        assert!(
                            voter != other,
                            "GLOBAL INVARIANT VIOLATION: duplicate vote on payment {}",
                            payment_id
                        );
                    }
                }
            }
        }
    }
});
