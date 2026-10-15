//! Host shim for the renderer's `training_mod_sync` calls.
//! Lock helpers match the product API used by `input_log.rs`.

pub use std::sync::{LazyLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub fn read<T: Copy>(rwlock: &RwLock<T>) -> T {
    *rwlock.read().expect("Read lock is poisoned")
}

pub fn assign<T>(rwlock: &RwLock<T>, new_val: T) {
    *rwlock.write().expect("Assign lock is poisoned") = new_val
}

pub fn lock_read<T>(rwlock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    rwlock.read().expect("Lock_Read lock is poisoned")
}

pub fn lock_write<T>(rwlock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    rwlock.write().expect("Lock_Write lock is poisoned")
}
