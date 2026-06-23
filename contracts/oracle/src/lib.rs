#![no_std]
extern crate alloc;

mod contract;
mod storage;

#[cfg(test)]
mod tests;

pub use contract::{
    FinalPayload, OracleContract, OracleContractClient, Round2Attestation, Round2Bundle,
    Round2Payload,
};
pub use warpdrive_shared::interfaces::handler::{Ed25519SignatureData, HandlerError};
