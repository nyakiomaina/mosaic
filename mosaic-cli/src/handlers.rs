use std::{path::PathBuf, str::FromStr};

use anyhow::{Context, Result, anyhow};
use borsh::BorshDeserialize;
use solana_client::{rpc_client::RpcClient, rpc_config::CommitmentConfig};
use solana_sdk::{
    message::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, read_keypair_file},
    signer::Signer,
    transaction::Transaction,
};
use solana_sdk_ids::system_program;
use tracing::{debug, info};

use crate::{
    config::{Config, get_mosaic_id},
    types::{
        CloseSessionIxData, CreateSessionIxData, ExecuteIxData, InitializeRootIxData,
        InstructionAccount, InstructionAccountJson, ProgramIx, Root, SignIxData, SigningSession,
        SigningSessionPhase,
    },
};

const ROOT_PDA: &[u8] = b"root_pda";
const SIGNING_SESSION_PDA: &[u8] = b"signing_session_pda";

fn load_keypair(path: &PathBuf) -> Result<Keypair> {
    match read_keypair_file(path) {
        Ok(kp) => Ok(kp),
        Err(e) => Err(anyhow!("Could not read keyfile: {}", e)),
    }
}

pub async fn handle_initialize_root(
    config: &Config,
    operators: Vec<String>,
    threshold: u8,
    destination_program: String,
    payer_path: Option<PathBuf>,
) -> Result<()> {
    info!("Initializing root account...");

    let program_id = get_mosaic_id(config)?;
    let rpc_client = RpcClient::new_with_commitment(&config.rpc_url, CommitmentConfig::confirmed());
    let operator_pubkeys: Result<Vec<Pubkey>> = operators
        .iter()
        .map(|s| Pubkey::from_str(s).context("Invalid operator pubkey"))
        .collect();
    let operator_pubkeys = operator_pubkeys?;

    debug!("Operators: {:?}", operator_pubkeys);
    debug!("Threshold: {}/{}", threshold, operator_pubkeys.len());

    let destination_program_pubkey =
        Pubkey::from_str(&destination_program).context("Invalid destination program ID")?;
    info!("Destination program: {}", destination_program_pubkey);

    let payer_keypair_path = payer_path
        .or_else(|| config.payer_keypair.clone())
        .ok_or_else(|| anyhow!("Payer keypair not specified"))?;
    let payer = load_keypair(&payer_keypair_path)?;
    info!("Payer: {}", payer.pubkey());

    let (root_pda, root_bump) = Pubkey::find_program_address(&[ROOT_PDA], &program_id);
    debug!("Root PDA: {} (bump: {})", root_pda, root_bump);

    let ix_data = InitializeRootIxData {
        operators: operator_pubkeys,
        threshold,
        destination_program: destination_program_pubkey,
        bump: root_bump,
    };
    let mut data = vec![ProgramIx::InitializeOperators as u8];
    data.extend_from_slice(&borsh::to_vec(&ix_data)?);

    let instruction = Instruction::new_with_bytes(
        program_id,
        &data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(root_pda, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );

    let recent_blockhash = rpc_client.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    let signature = rpc_client.send_and_confirm_transaction(&transaction)?;
    info!("\n✅ Root account initialized successfully!");
    info!("Transaction signature: {}", signature);
    info!("Root PDA: {}", root_pda);

    Ok(())
}

pub async fn handle_create_session(
    config: &Config,
    instruction_data: String,
    accounts: String,
    payer_path: Option<PathBuf>,
) -> Result<()> {
    let program_id = get_mosaic_id(config)?;
    let rpc_client = RpcClient::new_with_commitment(&config.rpc_url, CommitmentConfig::confirmed());
    let instruction_data = hex::decode(instruction_data.trim_start_matches("0x"))
        .context("Invalid hex string for instruction data")?;
    debug!("Instruction data: {} bytes", instruction_data.len());

    let accounts: Vec<InstructionAccountJson> =
        serde_json::from_str(&accounts).context("Invalid JSON for accounts")?;

    let instruction_accounts: Vec<Vec<u8>> = accounts
        .iter()
        .map(|acc| {
            let pubkey = Pubkey::from_str(&acc.pubkey)?;
            let instruction_account = InstructionAccount {
                pubkey: pubkey.to_bytes(),
                signer: acc.signer,
                writable: acc.writable,
            };
            borsh::to_vec(&instruction_account).context("Failed to serialize instruction account")
        })
        .collect::<Result<Vec<_>>>()?;

    debug!("Instruction accounts: {}", instruction_accounts.len());

    let payer_keypair_path = payer_path
        .or_else(|| config.payer_keypair.clone())
        .ok_or_else(|| anyhow!("Payer keypair not specified"))?;
    let payer = load_keypair(&payer_keypair_path)?;

    let (root_pda, _) = Pubkey::find_program_address(&[ROOT_PDA], &program_id);

    // Read root to get next session_id
    let root_account = rpc_client.get_account(&root_pda)
        .context("Failed to fetch root account. Has it been initialized?")?;
    let root = Root::try_from_slice(&root_account.data)
        .context("Failed to deserialize root account")?;
    let session_id = root.last_id.checked_add(1)
        .ok_or_else(|| anyhow!("Session ID overflow"))?;
    info!("Creating signing session {}...", session_id);

    let (signing_pda, signing_bump) = Pubkey::find_program_address(
        &[
            &root_pda.to_bytes(),
            &session_id.to_be_bytes(),
            SIGNING_SESSION_PDA,
        ],
        &program_id,
    );
    debug!(
        "Signing session PDA: {} (bump: {})",
        signing_pda, signing_bump
    );

    let create_ix_data = CreateSessionIxData {
        instruction_data,
        instruction_accounts,
        bump: signing_bump,
    };
    let mut data = vec![ProgramIx::CreateSession as u8];
    data.extend_from_slice(&borsh::to_vec(&create_ix_data)?);

    let instruction = Instruction::new_with_bytes(
        program_id,
        &data,
        vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(root_pda, false),
            AccountMeta::new(signing_pda, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );

    let recent_blockhash = rpc_client.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    let signature = rpc_client.send_and_confirm_transaction(&transaction)?;
    info!("\n✅ Signing session created successfully!");
    info!("Transaction signature: {}", signature);
    info!("Session ID: {}", session_id);
    info!("Signing session PDA: {}", signing_pda);

    Ok(())
}

pub async fn handle_sign(config: &Config, session_id: u16, signer_path: PathBuf) -> Result<()> {
    info!("Signing session {}...", session_id);

    let program_id = get_mosaic_id(config)?;
    let rpc_client = RpcClient::new_with_commitment(&config.rpc_url, CommitmentConfig::confirmed());

    let signer = load_keypair(&signer_path)?;
    info!("Signer: {}", signer.pubkey());

    let (root_pda, _) = Pubkey::find_program_address(&[ROOT_PDA], &program_id);

    let (signing_pda, signing_bump) = Pubkey::find_program_address(
        &[
            &root_pda.to_bytes(),
            &session_id.to_be_bytes(),
            SIGNING_SESSION_PDA,
        ],
        &program_id,
    );

    let sign_ix_data = SignIxData { bump: signing_bump };
    let mut data = vec![ProgramIx::Sign as u8];
    data.extend_from_slice(&borsh::to_vec(&sign_ix_data)?);

    let instruction = Instruction::new_with_bytes(
        program_id,
        &data,
        vec![
            AccountMeta::new(signer.pubkey(), true),
            AccountMeta::new_readonly(root_pda, false),
            AccountMeta::new(signing_pda, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );

    let recent_blockhash = rpc_client.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&signer.pubkey()),
        &[&signer],
        recent_blockhash,
    );

    let signature = rpc_client.send_and_confirm_transaction(&transaction)?;

    let account = rpc_client.get_account(&signing_pda)?;
    let session = SigningSession::try_from_slice(&account.data)?;

    let root_account = rpc_client.get_account(&root_pda)?;
    let root = Root::try_from_slice(&root_account.data)?;
    info!("\n✅ Session signed successfully!");
    info!("Transaction signature: {}", signature);
    info!(
        "Current approvals: {}/{}",
        session.approvals.len(),
        root.threshold
    );
    info!("Approvers:");
    for approver in &session.approvals {
        info!("  - {}", approver);
    }
    info!("Phase: {:?}", session.phase);

    Ok(())
}

pub async fn handle_execute(
    config: &Config,
    session_id: u16,
    executor_path: PathBuf,
) -> Result<()> {
    info!("Executing session {}...", session_id);

    let program_id = get_mosaic_id(config)?;
    let rpc_client = RpcClient::new_with_commitment(&config.rpc_url, CommitmentConfig::confirmed());

    let executor = load_keypair(&executor_path)?;
    info!("Executor: {}", executor.pubkey());

    let (root_pda, _) = Pubkey::find_program_address(&[ROOT_PDA], &program_id);

    let destination_program = if let Some(dest) = &config.destination_program {
        Pubkey::from_str(dest).context("Invalid destination program ID")?
    } else {
        let account = rpc_client.get_account(&root_pda)?;
        let root = Root::try_from_slice(&account.data)?;
        root.destination_program
    };

    let (signing_pda, _) = Pubkey::find_program_address(
        &[
            &root_pda.to_bytes(),
            &session_id.to_be_bytes(),
            SIGNING_SESSION_PDA,
        ],
        &program_id,
    );

    let account = rpc_client.get_account(&signing_pda)?;
    let session = SigningSession::try_from_slice(&account.data)?;

    if session.phase != SigningSessionPhase::Approved {
        return Err(anyhow!(
            "Session is not approved yet (current phase: {:?})",
            session.phase
        ));
    }

    // Build CPI accounts dynamically from session data
    let mut account_metas = vec![
        AccountMeta::new(executor.pubkey(), true),
        AccountMeta::new_readonly(root_pda, false),
        AccountMeta::new(signing_pda, false),
        AccountMeta::new_readonly(system_program::id(), false),
        AccountMeta::new_readonly(destination_program, false),
    ];

    for serialized_account in &session.instruction_accounts {
        let ia = InstructionAccount::try_from_slice(serialized_account)
            .context("Failed to deserialize instruction account from session")?;
        let pubkey = Pubkey::new_from_array(ia.pubkey);
        if ia.writable {
            account_metas.push(AccountMeta::new(pubkey, false));
        } else {
            account_metas.push(AccountMeta::new_readonly(pubkey, false));
        }
    }

    let execute_ix_data = ExecuteIxData {};
    let mut data = vec![ProgramIx::Execute as u8];
    data.extend_from_slice(&borsh::to_vec(&execute_ix_data)?);

    let instruction = Instruction::new_with_bytes(
        program_id,
        &data,
        account_metas,
    );

    let recent_blockhash = rpc_client.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&executor.pubkey()),
        &[&executor],
        recent_blockhash,
    );

    let signature = rpc_client.send_and_confirm_transaction(&transaction)?;

    info!("\n✅ Session executed successfully!");
    info!("Transaction signature: {}", signature);
    info!("Session ID: {}", session_id);

    Ok(())
}

pub async fn handle_view_root(config: &Config) -> Result<()> {
    info!("Fetching root account state...\n");

    let program_id = get_mosaic_id(config)?;
    let rpc_client = RpcClient::new_with_commitment(&config.rpc_url, CommitmentConfig::confirmed());

    let (root_pda, _) = Pubkey::find_program_address(&[ROOT_PDA], &program_id);
    info!("Root PDA: {}", root_pda);

    let account = rpc_client
        .get_account(&root_pda)
        .context("Failed to fetch root account. Has it been initialized?")?;
    let root =
        Root::try_from_slice(&account.data).context("Failed to deserialize root account data")?;

    info!("\n=== Root Account State ===");
    info!("Operators ({}):", root.operators.len());
    for (i, operator) in root.operators.iter().enumerate() {
        info!("  {}. {}", i + 1, operator);
    }
    info!("Threshold: {}/{}", root.threshold, root.operators.len());
    info!("Last Session ID: {}", root.last_id);
    info!("Destination Program: {}", root.destination_program);
    info!("Bump: {}", root.bump);
    info!("Account Owner: {}", account.owner);
    info!("Balance: {} lamports", account.lamports);

    Ok(())
}

pub async fn handle_view_session(config: &Config, session_id: u16) -> Result<()> {
    info!("Fetching signing session {}...\n", session_id);

    let program_id = get_mosaic_id(config)?;
    let rpc_client = RpcClient::new_with_commitment(&config.rpc_url, CommitmentConfig::confirmed());

    let (root_pda, _) = Pubkey::find_program_address(&[ROOT_PDA], &program_id);

    let (signing_pda, _) = Pubkey::find_program_address(
        &[
            &root_pda.to_bytes(),
            &session_id.to_be_bytes(),
            SIGNING_SESSION_PDA,
        ],
        &program_id,
    );
    info!("Signing Session PDA: {}", signing_pda);

    let account = rpc_client
        .get_account(&signing_pda)
        .context("Failed to fetch signing session. Does it exist?")?;

    let session = SigningSession::try_from_slice(&account.data)
        .context("Failed to deserialize signing session data")?;

    info!("\n=== Signing Session State ===");
    info!("Session ID: {}", session.session_id);
    info!("Root PDA: {}", session.root_pda);
    info!("Phase: {:?}", session.phase);
    info!("Approvals ({}):", session.approvals.len());
    for (i, approver) in session.approvals.iter().enumerate() {
        info!("  {}. {}", i + 1, approver);
    }
    info!("Instruction Data: {} bytes", session.instruction_data.len());
    info!("  Hex: {}", hex::encode(&session.instruction_data));
    info!(
        "Instruction Accounts: {}",
        session.instruction_accounts.len()
    );
    info!("Bump: {}", session.bump);
    info!("Account Owner: {}", account.owner);
    info!("Balance: {} lamports", account.lamports);

    Ok(())
}

pub async fn handle_list_sessions(config: &Config) -> Result<()> {
    info!("Listing all signing sessions...\n");

    let program_id = get_mosaic_id(config)?;
    let rpc_client = RpcClient::new_with_commitment(&config.rpc_url, CommitmentConfig::confirmed());

    let accounts = rpc_client.get_program_accounts(&program_id)?;

    let mut sessions = Vec::new();
    for (pubkey, account) in accounts {
        if let Ok(session) = SigningSession::try_from_slice(&account.data) {
            sessions.push((pubkey, session));
        }
    }

    if sessions.is_empty() {
        info!("No signing sessions found.");
        return Ok(());
    }

    sessions.sort_by_key(|(_, s)| s.session_id);

    info!("Found {} signing session(s):\n", sessions.len());
    info!(
        "{:<6} {:<44} {:<12} {:<10}",
        "ID", "PDA", "Phase", "Approvals"
    );
    info!("{}", "-".repeat(78));

    for (pubkey, session) in sessions {
        info!(
            "{:<6} {:<44} {:<12?} {:<10}",
            session.session_id,
            pubkey,
            session.phase,
            session.approvals.len()
        );
    }

    Ok(())
}

pub async fn handle_close_session(
    config: &Config,
    session_id: u16,
    closer_path: PathBuf,
) -> Result<()> {
    info!("Closing session {}...", session_id);

    let program_id = get_mosaic_id(config)?;
    let rpc_client = RpcClient::new_with_commitment(&config.rpc_url, CommitmentConfig::confirmed());

    let closer = load_keypair(&closer_path)?;
    info!("Closer: {}", closer.pubkey());

    let (root_pda, _) = Pubkey::find_program_address(&[ROOT_PDA], &program_id);

    let (signing_pda, signing_bump) = Pubkey::find_program_address(
        &[
            &root_pda.to_bytes(),
            &session_id.to_be_bytes(),
            SIGNING_SESSION_PDA,
        ],
        &program_id,
    );

    let account = rpc_client
        .get_account(&signing_pda)
        .context("Failed to fetch signing session. Does it exist?")?;
    let session = SigningSession::try_from_slice(&account.data)?;

    info!("Current phase: {:?}", session.phase);

    let close_ix_data = CloseSessionIxData { bump: signing_bump };
    let mut data = vec![ProgramIx::CloseSession as u8];
    data.extend_from_slice(&borsh::to_vec(&close_ix_data)?);

    let instruction = Instruction::new_with_bytes(
        program_id,
        &data,
        vec![
            AccountMeta::new(closer.pubkey(), true),
            AccountMeta::new_readonly(root_pda, false),
            AccountMeta::new(signing_pda, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
    );

    let recent_blockhash = rpc_client.get_latest_blockhash()?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&closer.pubkey()),
        &[&closer],
        recent_blockhash,
    );

    let signature = rpc_client.send_and_confirm_transaction(&transaction)?;

    info!("\n✅ Session closed successfully!");
    info!("Transaction signature: {}", signature);
    info!("Session ID: {}", session_id);
    info!("Reclaimed rent sent to: {}", closer.pubkey());

    Ok(())
}
