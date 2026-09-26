#![no_main]

use libfuzzer_sys::fuzz_target;
use arbitrary::Arbitrary;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    vec, Address, Env, Symbol,
};

use health_chain_contract::payments::{
    EscrowAccount, FeeStructure, MultiSigConfig, Payment, PaymentStatus, PendingApproval,
    HIGH_VALUE_THRESHOLD,
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
    FundEscrow {
        payment_idx: u8,
        caller_idx: u8,
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

fn read_payment(env: &Env, contract_id: &Address, payment_id: u64) -> Option<Payment> {
    env.as_contract(contract_id, || {
        env.storage().persistent().get(&DataKey::Payment(payment_id))
    })
}

fn read_escrow(env: &Env, contract_id: &Address, payment_id: u64) -> Option<EscrowAccount> {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::EscrowAccount(payment_id))
    })
}

fn read_approval(env: &Env, contract_id: &Address, payment_id: u64) -> Option<PendingApproval> {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get(&DataKey::PendingApprovalRecord(payment_id))
    })
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
    let mut payment_payers: Vec<(u64, Address)> = Vec::new();
    // Track expected amount per payment_id so we can check the multisig vs
    // single-admin branch was taken correctly.
    let mut payment_amounts: Vec<(u64, i128)> = Vec::new();
    let mut multisig_configured = false;
    let mut configured_threshold: u32 = 0;
    // Largest signer set ever configured: configure_multisig keeps in-flight
    // votes, so a vote count is bounded by every signer ever allowed to vote.
    let mut max_signer_count: usize = 0;

    for op in input.operations.iter() {
        match op {
            PaymentOperation::CreatePayment {
                payer_idx,
                payee_idx,
                caller_is_admin,
                amount_kind,
                negative_fee,
            } => {
                let payer = actors[*payer_idx as usize % actors.len()].clone();
                let payee = actors[*payee_idx as usize % actors.len()].clone();
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
                    &1u64, &payer, &payee, &amount, &asset, &fee_structure, &caller,
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
                    payment_payers.push((payment_id, payer.clone()));
                    payment_amounts.push((payment_id, amount));

                    // INVARIANT: a new payment starts Pending — it must be
                    // funded via fund_escrow before it can be released (#1431).
                    let payment = read_payment(&env, &contract_id, payment_id).unwrap();
                    assert_eq!(
                        payment.status,
                        PaymentStatus::Pending,
                        "INVARIANT VIOLATION: new payment not Pending"
                    );

                    // INVARIANT: escrow account must exist immediately after
                    // creation, pre-populated with locked_amount == amount and
                    // medical_records_verified == false (per
                    // escrow_conditions_stored_at_payment_creation test).
                    {
                        let escrow = read_escrow(&env, &contract_id, payment_id).unwrap();
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
            }

            PaymentOperation::FundEscrow {
                payment_idx,
                caller_idx,
            } => {
                if payment_ids.is_empty() {
                    continue;
                }
                let payment_id = payment_ids[(*payment_idx as usize) % payment_ids.len()];
                let payer = payment_payers
                    .iter()
                    .find(|(id, _)| *id == payment_id)
                    .map(|(_, p)| p.clone())
                    .unwrap();
                let caller = actors[*caller_idx as usize % actors.len()].clone();
                let before = read_payment(&env, &contract_id, payment_id).unwrap();

                let result = client.try_fund_escrow(&payment_id, &caller);
                let after = read_payment(&env, &contract_id, payment_id).unwrap();

                if caller != payer {
                    // INVARIANT: only the payer may fund the escrow.
                    assert_eq!(
                        result,
                        Err(Ok(Error::Unauthorized)),
                        "INVARIANT VIOLATION: non-payer funded escrow"
                    );
                } else if before.status == PaymentStatus::Pending {
                    // INVARIANT: fund_escrow is the Pending -> Escrowed entrypoint.
                    assert!(result.is_ok(), "INVARIANT VIOLATION: payer could not fund Pending payment");
                    assert_eq!(after.status, PaymentStatus::Escrowed);
                } else {
                    assert_eq!(
                        result,
                        Err(Ok(Error::InvalidPaymentStatus)),
                        "INVARIANT VIOLATION: fund_escrow accepted non-Pending payment"
                    );
                }
                if result.is_err() {
                    assert_eq!(before.status, after.status, "INVARIANT VIOLATION: failed fund_escrow mutated status");
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
                let payment_id = payment_ids[(*payment_idx as usize) % payment_ids.len()];
                let approver = actors[*approver_idx as usize % actors.len()].clone();

                let current = env.ledger().timestamp();
                let min_ts = if *min_timestamp_offset >= 0 {
                    current.saturating_add(*min_timestamp_offset as u64)
                } else {
                    current.saturating_sub(min_timestamp_offset.unsigned_abs() as u64)
                };
                let _ = client.try_set_escrow_conditions(
                    &payment_id,
                    verified,
                    &min_ts,
                    &Some(approver),
                );
            }

            PaymentOperation::ConfigureMultisig {
                num_signers,
                threshold,
                caller_is_admin,
            } => {
                let n = ((*num_signers % 5) + 1) as usize; // 1..=5 signers
                let mut signers = vec![&env];
                for i in 0..n {
                    signers.push_back(actors[i % actors.len()].clone());
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
                        max_signer_count = max_signer_count.max(n);
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
                let payment_id = payment_ids[(*payment_idx as usize) % payment_ids.len()];
                let approver = actors[*approver_idx as usize % actors.len()].clone();

                let amount = payment_amounts
                    .iter()
                    .find(|(id, _)| *id == payment_id)
                    .map(|(_, a)| *a);

                let result = client.try_propose_release(&payment_id, &approver);

                if let Ok(Ok(executed)) = result {
                    if executed {
                        // INVARIANT: if propose_release reports executed,
                        // the payment must actually be Completed with
                        // escrow_released_at set.
                        {
                            let payment = read_payment(&env, &contract_id, payment_id).unwrap();
                            assert_eq!(
                                payment.status,
                                PaymentStatus::Completed,
                                "INVARIANT VIOLATION: propose_release reported executed but status != Completed"
                            );
                            assert!(
                                payment.escrow_released_at.is_some(),
                                "INVARIANT VIOLATION: executed release missing escrow_released_at"
                            );
                        }

                        if let Some(amt) = amount {
                            if amt >= HIGH_VALUE_THRESHOLD && multisig_configured {
                                {
                                    let approval =
                                        read_approval(&env, &contract_id, payment_id).unwrap();
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

        // 1. Every payment's escrow must record the gross amount it was
        // created with. The fuzzer only creates zero-fee payments, so this is
        // also the stored net payment.amount (would indicate desync between
        // the two per-record entries).
        for (payment_id, amount) in &payment_amounts {
            let payment = read_payment(&env, &contract_id, *payment_id).unwrap();
            let escrow = read_escrow(&env, &contract_id, *payment_id).unwrap();
            assert_eq!(
                escrow.locked_amount, *amount,
                "GLOBAL INVARIANT VIOLATION: escrow amount changed for {}",
                payment_id
            );
            assert_eq!(
                escrow.locked_amount, payment.amount,
                "GLOBAL INVARIANT VIOLATION: escrow/payment amount desync for {}",
                payment_id
            );
        }

        // 2. A payment can never collect more votes than signers ever allowed.
        if multisig_configured {
            for payment_id in &payment_ids {
                if let Some(approval) = read_approval(&env, &contract_id, *payment_id) {
                    assert!(
                        approval.approvals.len() as usize <= max_signer_count,
                        "GLOBAL INVARIANT VIOLATION: more votes ({}) than configured signers ({}) for payment {}",
                        approval.approvals.len(),
                        max_signer_count,
                        payment_id
                    );
                }
            }
        }
    }
});
