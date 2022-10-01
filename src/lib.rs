#![feature(generic_associated_types)]

pub mod contract;
mod error;
pub mod msg;
mod simple_wasmi_vm;
pub mod state;

pub use crate::error::ContractError;
