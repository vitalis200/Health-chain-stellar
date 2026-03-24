use soroban_sdk::{contracttype, String};

#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum DataKey {
    Admin,
    InventoryContract,
    RequestCounter,
    Initialized,
    Metadata,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub struct ContractMetadata {
    pub name: String,
    pub version: u32,
}
