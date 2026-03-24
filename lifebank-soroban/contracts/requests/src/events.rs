use soroban_sdk::{Address, Env, Symbol};

pub fn emit_initialized(env: &Env, admin: &Address, inventory_contract: &Address) {
    env.events().publish(
        (Symbol::new(env, "initialized"),),
        (admin.clone(), inventory_contract.clone()),
    );
}
