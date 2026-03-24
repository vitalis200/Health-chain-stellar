use soroban_sdk::{Env, Vec};
use crate::types::{DataKey, TemperatureReading, TemperatureThreshold};

pub fn get_admin(env: &Env) -> soroban_sdk::Address {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .unwrap()
}

pub fn set_admin(env: &Env, admin: &soroban_sdk::Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

pub fn get_threshold(env: &Env, unit_id: u64) -> Option<TemperatureThreshold> {
    env.storage()
        .persistent()
        .get(&DataKey::Threshold(unit_id))
}

pub fn set_threshold(env: &Env, unit_id: u64, threshold: &TemperatureThreshold) {
    env.storage()
        .persistent()
        .set(&DataKey::Threshold(unit_id), threshold);
}

pub fn get_temp_page(
    env: &Env,
    unit_id: u64,
    page: u32,
) -> Vec<TemperatureReading> {
    env.storage()
        .persistent()
        .get(&DataKey::TempPage(unit_id, page))
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_temp_page(
    env: &Env,
    unit_id: u64,
    page: u32,
    readings: &Vec<TemperatureReading>,
) {
    env.storage()
        .persistent()
        .set(&DataKey::TempPage(unit_id, page), readings);
}

pub fn get_temp_page_len(env: &Env, unit_id: u64, page: u32) -> u32 {
    env.storage()
        .persistent()
        .get(&DataKey::TempPageLen(unit_id, page))
        .unwrap_or(0)
}

pub fn set_temp_page_len(env: &Env, unit_id: u64, page: u32, len: u32) {
    env.storage()
        .persistent()
        .set(&DataKey::TempPageLen(unit_id, page), &len);
}
