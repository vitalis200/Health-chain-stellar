#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, BytesN, Env, String,
    Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    InvalidInput = 1,
    LicenseAlreadyRegistered = 2,
    InvalidOrgType = 3,
    AlreadyInitialized = 4,
    Unauthorized = 5,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OrgType {
    BloodBank,
    Hospital,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Role {
    BloodBank,
    Hospital,
    Admin,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Organization {
    pub id: Address,
    pub org_type: OrgType,
    pub name: String,
    pub license_number: String,
    pub verified: bool,
    pub verified_timestamp: Option<u64>,
    pub rating: u32,
    pub total_ratings: u32,
    pub location_hash: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct OrganizationRegistered {
    pub org_id: Address,
    pub org_type: OrgType,
    pub name: String,
}

#[contracttype]
#[derive(Clone, Debug)]
pub enum DataKey {
    Org(Address),
    License(String),
    Docs(Address),
    Role(Address),
    OrgCounter,
    Admin,
}

#[contract]
pub struct IdentityContract;

#[contractimpl]
impl IdentityContract {
    /// Initialize the contract with an admin
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        Self::grant_role(env, admin, Role::Admin);
        Ok(())
    }

    /// Register a new organization
    pub fn register_organization(
        env: Env,
        owner: Address,
        org_type: OrgType,
        name: String,
        license_number: String,
        location_hash: BytesN<32>,
        document_hashes: Vec<BytesN<32>>,
    ) -> Result<Address, Error> {
        owner.require_auth();

        // Validate data
        if name.len() == 0 || license_number.len() == 0 {
            return Err(Error::InvalidInput);
        }

        // Check if license number is unique
        let license_key = DataKey::License(license_number.clone());
        if env.storage().persistent().has(&license_key) {
            return Err(Error::LicenseAlreadyRegistered);
        }

        // Generate organization ID (using owner address as ID)
        let org_id = owner.clone();

        // Create organization
        let organization = Organization {
            id: org_id.clone(),
            org_type: org_type.clone(),
            name: name.clone(),
            license_number: license_number.clone(),
            verified: false,
            verified_timestamp: None,
            rating: 0,
            total_ratings: 0,
            location_hash,
        };

        // Store organization
        env.storage()
            .persistent()
            .set(&DataKey::Org(org_id.clone()), &organization);

        // Store license mapping
        env.storage().persistent().set(&license_key, &org_id);

        // Store documents
        env.storage()
            .persistent()
            .set(&DataKey::Docs(org_id.clone()), &document_hashes);

        // Assign role based on type
        let role = match org_type {
            OrgType::BloodBank => Role::BloodBank,
            OrgType::Hospital => Role::Hospital,
        };
        Self::grant_role(env.clone(), org_id.clone(), role);

        // Increment counter
        Self::increment_counter(&env, DataKey::OrgCounter);

        // Emit event
        env.events().publish(
            (symbol_short!("org_reg"),),
            OrganizationRegistered {
                org_id: org_id.clone(),
                org_type,
                name,
            },
        );

        Ok(org_id)
    }

    /// Internal helper to grant a role to an address
    pub fn grant_role(env: Env, address: Address, role: Role) {
        env.storage().persistent().set(&DataKey::Role(address), &role);
    }

    /// Get the role of an address
    pub fn get_role(env: Env, address: Address) -> Option<Role> {
        env.storage().persistent().get(&DataKey::Role(address))
    }

    /// Internal helper to increment a counter
    fn increment_counter(env: &Env, key: DataKey) -> u32 {
        let mut count: u32 = env.storage().instance().get(&key).unwrap_or(0);
        count += 1;
        env.storage().instance().set(&key, &count);
        count
    }

    /// Get organization by ID
    pub fn get_organization(env: Env, org_id: Address) -> Option<Organization> {
        env.storage().persistent().get(&DataKey::Org(org_id))
pub struct AccessControlContract;

#[contractimpl]
impl AccessControlContract {
    /// Initialize the contract with an administrator
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().persistent().has(&DataKey::Admin) {
            panic!("Already initialized");
        }
        env.storage().persistent().set(&DataKey::Admin, &admin);
    }

    /// Grant a role to an address
    ///
    /// # Arguments
    /// * `address` - The address to grant the role to
    /// * `role` - The role to grant
    /// * `expires_at` - Optional expiration timestamp
    pub fn grant_role_with_expiry(env: Env, address: Address, role: Role, expires_at: Option<u64>) {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();

        // Proactive cleanup: remove expired roles for this address first
        Self::cleanup_expired_roles_internal(&env, &address);

        let key = DataKey::AddressRoles(address.clone());
        let mut roles: Vec<RoleGrant> = env
            .storage()
            .persistent()
            .get(&key)
            .unwrap_or(Vec::new(&env));

        let granted_at = env.ledger().timestamp();
        let new_grant = RoleGrant {
            role: role.clone(),
            granted_at,
            expires_at,
        };

        // Remove any existing grant for this role to avoid duplicates
        roles = Self::remove_role_from_vec(&env, roles, &role);

        // Insert the new grant in sorted order
        roles = Self::insert_sorted(&env, roles, new_grant);

        env.storage().persistent().set(&key, &roles);
    }

    /// Revoke a role from an address
    ///
    /// # Arguments
    /// * `address` - The address to revoke the role from
    /// * `role` - The role to revoke
    pub fn revoke_role(env: Env, address: Address, role: Role) {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();

        let key = DataKey::AddressRoles(address.clone());

        if let Some(mut roles) = env
            .storage()
            .persistent()
            .get::<DataKey, Vec<RoleGrant>>(&key)
        {
            roles = Self::remove_role_from_vec(&env, roles, &role);

            if roles.is_empty() {
                env.storage().persistent().remove(&key);
            } else {
                env.storage().persistent().set(&key, &roles);
            }
        }
    }

    /// Check if an address has a specific role
    ///
    /// # Arguments
    /// * `address` - The address to check
    /// * `role` - The role to check for
    ///
    /// # Returns
    /// `true` if the address has the role and it hasn't expired, `false` otherwise
    ///
    /// Implementation Note
    /// This function implements lazy deletion: if it encounters ANY expired role grants
    /// for the user, it will remove them all from storage before returning.
    pub fn has_role(env: Env, address: Address, role: Role) -> bool {
        // Full lazy deletion: clean up ALL expired roles for this address
        Self::cleanup_expired_roles_internal(&env, &address);

        let key = DataKey::AddressRoles(address);

        if let Some(roles) = env
            .storage()
            .persistent()
            .get::<DataKey, Vec<RoleGrant>>(&key)
        {
            for i in 0..roles.len() {
                let grant = roles.get(i).unwrap();
                if grant.role == role {
                    // We already performed cleanup, so if it's here, it's valid
                    return true;
                }
            }
        }

        false
    }

    /// Get all roles for an address
    ///
    /// # Arguments
    /// * `address` - The address to get roles for
    ///
    /// # Returns
    /// A vector of all role grants for the address (including expired ones)
    pub fn get_roles(env: Env, address: Address) -> Vec<RoleGrant> {
        let key = DataKey::AddressRoles(address);
        env.storage()
            .persistent()
            .get(&key)
            .unwrap_or(Vec::new(&env))
    }

    /// Clean up all expired role grants for an address
    ///
    /// This function proactively removes all expired role grants from storage for a given address.
    /// It's useful for batch cleanup operations to reduce storage footprint.
    ///
    /// # Arguments
    /// * `address` - The address to clean up expired roles for
    ///
    /// Returns
    /// The number of expired roles that were removed
    pub fn cleanup_expired_roles(env: Env, address: Address) -> u32 {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Admin)
            .expect("Not initialized");
        admin.require_auth();

        Self::cleanup_expired_roles_internal(&env, &address)
    }

    /// Internal helper for cleaning up expired roles
    fn cleanup_expired_roles_internal(env: &Env, address: &Address) -> u32 {
        let key = DataKey::AddressRoles(address.clone());

        if let Some(roles) = env
            .storage()
            .persistent()
            .get::<DataKey, Vec<RoleGrant>>(&key)
        {
            let current_time = env.ledger().timestamp();
            let mut new_roles = Vec::new(env);
            let mut removed_count = 0u32;

            // Filter out expired roles
            for i in 0..roles.len() {
                let grant = roles.get(i).unwrap();
                let is_expired = if let Some(expires_at) = grant.expires_at {
                    current_time >= expires_at
                } else {
                    false
                };

                if is_expired {
                    removed_count += 1;
                } else {
                    new_roles.push_back(grant);
                }
            }

            // Update storage
            if removed_count > 0 {
                if new_roles.is_empty() {
                    env.storage().persistent().remove(&key);
                } else {
                    env.storage().persistent().set(&key, &new_roles);
                }
            }

            removed_count
        } else {
            0
        }
    }

    /// Helper function to remove a role from a vector
    fn remove_role_from_vec(env: &Env, roles: Vec<RoleGrant>, role: &Role) -> Vec<RoleGrant> {
        let mut new_roles = Vec::new(env);
        for i in 0..roles.len() {
            let grant = roles.get(i).unwrap();
            if &grant.role != role {
                new_roles.push_back(grant);
            }
        }
        new_roles
    }

    /// Helper function to insert a role grant in sorted order
    fn insert_sorted(env: &Env, roles: Vec<RoleGrant>, new_grant: RoleGrant) -> Vec<RoleGrant> {
        let mut new_roles = Vec::new(env);
        let mut inserted = false;

        for i in 0..roles.len() {
            let grant = roles.get(i).unwrap();
            if !inserted && new_grant.role < grant.role {
                new_roles.push_back(new_grant.clone());
                inserted = true;
            }
            new_roles.push_back(grant);
        }

        if !inserted {
            new_roles.push_back(new_grant);
        }

        new_roles
    }
}

mod test;
