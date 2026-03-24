use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ContractError {
    Unauthorized = 1,
    UnitNotFound = 2,
    ThresholdNotFound = 3,
    InvalidThreshold = 4,
    AlreadyInitialized = 5,
}
