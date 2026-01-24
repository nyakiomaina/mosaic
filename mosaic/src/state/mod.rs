use borsh::{BorshDeserialize, BorshSerialize};
use pinocchio::error::ProgramError;

pub mod root;
pub mod signing_session;

pub trait PackUnpack: BorshDeserialize + BorshSerialize {
    /// returns serialized data with length
    fn pack(&self) -> Result<(Vec<u8>, usize), ProgramError> {
        let data = borsh::to_vec(&self).map_err(|_| ProgramError::InvalidAccountData)?;
        let size = data.len();
        Ok((data, size))
    }
    /// returns deserialized data
    fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        borsh::from_slice(&data).map_err(|_| ProgramError::InvalidAccountData)
    }
}
