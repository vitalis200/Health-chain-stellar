use crate::storage;
use crate::{ContractMetadata, RequestContract, RequestContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, String,
};

fn create_uninitialized_contract<'a>() -> (Env, RequestContractClient<'a>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(RequestContract, ());
    let client = RequestContractClient::new(&env, &contract_id);

    (env, client, contract_id)
}

#[test]
fn test_initialize_sets_admin_inventory_counter_and_metadata() {
    let (env, client, contract_id) = create_uninitialized_contract();
    let admin = Address::generate(&env);
    let inventory_contract = Address::generate(&env);

    client.initialize(&admin, &inventory_contract);

    assert!(client.is_initialized());
    assert_eq!(client.get_admin(), admin.clone());
    assert_eq!(client.get_inventory_contract(), inventory_contract.clone());
    assert_eq!(client.get_request_counter(), 0);
    assert_eq!(
        client.get_metadata(),
        ContractMetadata {
            name: String::from_str(&env, "Blood Request Management"),
            version: 1,
        }
    );

    let stored_admin = env.as_contract(&contract_id, || storage::get_admin(&env));
    let stored_inventory =
        env.as_contract(&contract_id, || storage::get_inventory_contract(&env));
    let stored_counter =
        env.as_contract(&contract_id, || storage::get_request_counter(&env));

    assert_eq!(stored_admin, admin);
    assert_eq!(stored_inventory, inventory_contract);
    assert_eq!(stored_counter, 0);
}

#[test]
fn test_initialize_emits_initialized_event() {
    let (env, client, _contract_id) = create_uninitialized_contract();
    let admin = Address::generate(&env);
    let inventory_contract = Address::generate(&env);

    client.initialize(&admin, &inventory_contract);

    let events = env.events().all();
    assert_eq!(events.len(), 1);
}

#[test]
#[should_panic(expected = "Error(Contract, #0)")]
fn test_initialize_cannot_run_twice() {
    let (env, client, _contract_id) = create_uninitialized_contract();
    let admin = Address::generate(&env);
    let inventory_contract = Address::generate(&env);

    client.initialize(&admin, &inventory_contract);
    client.initialize(&admin, &inventory_contract);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_readers_fail_before_initialization() {
    let (_env, client, _contract_id) = create_uninitialized_contract();
    let _ = client.get_admin();
}
