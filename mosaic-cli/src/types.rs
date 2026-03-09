use borsh::{BorshDeserialize, BorshSerialize};
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;

#[derive(BorshSerialize, BorshDeserialize)]
pub struct InitializeRootIxData {
    pub operators: Vec<Pubkey>,
    pub threshold: u8,
    pub destination_program: Pubkey,
    pub bump: u8,
}

#[repr(u8)]
pub enum ProgramIx {
    InitializeOperators = 0,
    CreateSession = 1,
    Sign = 2,
    Execute = 3,
    CloseSession = 4
}

#[derive(Deserialize)]
pub struct InstructionAccountJson {
    pub pubkey: String,
    pub signer: bool,
    pub writable: bool,
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct InstructionAccount {
    pub pubkey: [u8; 32],
    pub signer: bool,
    pub writable: bool,
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct CreateSessionIxData {
    pub instruction_data: Vec<u8>,
    pub instruction_accounts: Vec<Vec<u8>>,
    pub bump: u8,
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct SignIxData {
    pub bump: u8,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct SigningSession {
    pub session_id: u16,
    pub root_pda: Pubkey,
    pub phase: SigningSessionPhase,
    pub approvals: Vec<Pubkey>,
    pub instruction_data: Vec<u8>,
    pub instruction_accounts: Vec<Vec<u8>>,
    pub bump: u8,
}


#[derive(BorshSerialize, BorshDeserialize, Debug, PartialEq)]
pub enum SigningSessionPhase {
    Uninitialized,
    Active,
    Approved,
    Executed,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Root {
    pub operators: Vec<Pubkey>,
    pub last_id: u16,
    pub threshold: u8,
    pub destination_program: Pubkey,
    pub bump: u8,
}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct ExecuteIxData {}

#[derive(BorshSerialize, BorshDeserialize)]
pub struct CloseSessionIxData {
    pub bump: u8,
}
