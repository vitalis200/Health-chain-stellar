use crate::types::{BloodRegisteredEvent, BloodStatus, BloodType, StatusChangeEvent};
use soroban_sdk::{Address, Env, String, Symbol};

/// Emit a BloodRegistered event
///
/// # Arguments
/// * `env` - Contract environment
/// * `blood_unit_id` - Unique ID of the registered blood unit
/// * `bank_id` - Blood bank that registered the unit
/// * `blood_type` - Type of blood
/// * `quantity_ml` - Quantity in milliliters
/// * `expiration_timestamp` - When the unit expires
pub fn emit_blood_registered(
    env: &Env,
    blood_unit_id: u64,
    bank_id: &Address,
    blood_type: BloodType,
    quantity_ml: u32,
    expiration_timestamp: u64,
) {
    let registered_at = env.ledger().timestamp();

    let event = BloodRegisteredEvent {
        blood_unit_id,
        bank_id: bank_id.clone(),
        blood_type,
        quantity_ml,
        expiration_timestamp,
        registered_at,
    };

    env.events()
        .publish((Symbol::new(env, "blood_registered"),), event);
}


pub fn emit_status_change(
    env: &Env,
    blood_unit_id: u64,
    from_status: crate::types::BloodStatus,
    to_status: crate::types::BloodStatus,
    authorized_by: &Address,
    reason: Option<String>,
) {
    let changed_at = env.ledger().timestamp();

    let event = StatusChangeEvent {
        blood_unit_id,
        from_status,
        to_status,
        authorized_by: authorized_by.clone(),
        changed_at,
        reason,
    };

    env.events()
        .publish((Symbol::new(env, "status_changed"),), event);
}

/// Emit an event when an invalid status transition is attempted.
/// Includes both the `from` and `to` statuses for debuggability.
pub fn emit_invalid_transition(
    env: &Env,
    blood_unit_id: u64,
    from_status: BloodStatus,
    to_status: BloodStatus,
) {
    env.events().publish(
        (Symbol::new(env, "invalid_transition"),),
        (blood_unit_id, from_status as u32, to_status as u32),
    );
}
