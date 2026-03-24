use crate::error::ContractError;
use crate::types::BloodStatus;

/// Validate blood quantity is within acceptable range (100-600ml)
pub fn validate_quantity(quantity_ml: u32) -> Result<(), ContractError> {
    if quantity_ml < 100 || quantity_ml > 600 {
        return Err(ContractError::InvalidQuantity);
    }
    Ok(())
}

/// Validate status transition is allowed according to the blood unit lifecycle state machine.
///
/// Uses the pure `is_valid_transition` function to determine whether the transition
/// is legal. Returns `ContractError::InvalidStatusTransition` if the transition
/// is not in the allowed set.
pub fn validate_status_transition(
    current_status: BloodStatus,
    new_status: BloodStatus,
) -> Result<(), ContractError> {
    if !crate::types::is_valid_transition(&current_status, &new_status) {
        return Err(ContractError::InvalidStatusTransition);
    }
    Ok(())
}
