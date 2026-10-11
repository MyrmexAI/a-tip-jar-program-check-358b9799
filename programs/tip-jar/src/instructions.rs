use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    AccountView, Address, ProgramResult,
};
use pinocchio_system::create_account_with_minimum_balance_signed;
use pinocchio_token::{
    instructions::{InitializeAccount3, Transfer},
    state::Account as TokenAccount,
};

use crate::{
    state::{Jar, JAR_LEN, JAR_SEED, VAULT_SEED},
    TipJarError,
};

fn parse_amount(data: &[u8]) -> Result<u64, ProgramError> {
    let bytes: [u8; 8] = data
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let amount = u64::from_le_bytes(bytes);
    if amount == 0 {
        return Err(TipJarError::ZeroAmount.into());
    }
    Ok(amount)
}

fn check_token_program(token_program: &AccountView) -> ProgramResult {
    if token_program.address() != &pinocchio_token::ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    Ok(())
}

/// Accounts: payer (signer, writable), jar (writable), mint, vault (writable),
/// token program, system program. Data: owner address (32 bytes).
pub fn initialize(program_id: &Address, accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let [payer, jar, mint, vault, token_program, system_program] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !payer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    check_token_program(token_program)?;
    if system_program.address() != &pinocchio_system::ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !mint.owned_by(&pinocchio_token::ID) {
        return Err(ProgramError::InvalidAccountOwner);
    }

    let owner_bytes: [u8; 32] = data
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let owner = Address::new_from_array(owner_bytes);
    let mint_address = *mint.address();

    let (expected_jar, jar_bump) = Address::find_program_address(
        &[JAR_SEED, owner.as_ref(), mint_address.as_ref()],
        program_id,
    );
    if jar.address() != &expected_jar {
        return Err(ProgramError::InvalidSeeds);
    }
    let (expected_vault, vault_bump) =
        Address::find_program_address(&[VAULT_SEED, expected_jar.as_ref()], program_id);
    if vault.address() != &expected_vault {
        return Err(ProgramError::InvalidSeeds);
    }

    let jar_bump_seed = [jar_bump];
    let jar_seeds = [
        Seed::from(JAR_SEED),
        Seed::from(owner.as_array()),
        Seed::from(mint_address.as_array()),
        Seed::from(&jar_bump_seed),
    ];
    create_account_with_minimum_balance_signed(
        jar,
        JAR_LEN,
        program_id,
        payer,
        None,
        &[Signer::from(&jar_seeds)],
    )?;

    let vault_bump_seed = [vault_bump];
    let vault_seeds = [
        Seed::from(VAULT_SEED),
        Seed::from(expected_jar.as_array()),
        Seed::from(&vault_bump_seed),
    ];
    create_account_with_minimum_balance_signed(
        vault,
        TokenAccount::LEN,
        &pinocchio_token::ID,
        payer,
        None,
        &[Signer::from(&vault_seeds)],
    )?;
    InitializeAccount3::new(vault, mint, &expected_jar)
        .invoke_with_program(token_program.address())?;

    Jar {
        owner,
        mint: mint_address,
        vault: expected_vault,
        bump: jar_bump,
    }
    .store(jar)
}

/// Accounts: sender (signer), sender token account (writable), jar, vault
/// (writable), token program. Data: amount (u64 LE, > 0).
pub fn tip(program_id: &Address, accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let [sender, source, jar, vault, token_program] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !sender.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    check_token_program(token_program)?;
    let amount = parse_amount(data)?;

    let jar_state = Jar::load(jar, program_id)?;
    if vault.address() != &jar_state.vault || source.address() == vault.address() {
        return Err(ProgramError::InvalidArgument);
    }
    {
        let source_state = TokenAccount::from_account_view(source)?;
        if source_state.mint() != &jar_state.mint {
            return Err(ProgramError::InvalidAccountData);
        }
    }

    Transfer::new(source, vault, sender, amount)
        .invoke_with_program(token_program.address())
}

/// Accounts: owner (signer), jar, vault (writable), destination token account
/// (writable), token program. Data: amount (u64 LE, > 0).
pub fn withdraw(program_id: &Address, accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    let [owner, jar, vault, destination, token_program] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !owner.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    check_token_program(token_program)?;
    let amount = parse_amount(data)?;

    let jar_state = Jar::load(jar, program_id)?;
    if owner.address() != &jar_state.owner {
        return Err(TipJarError::Unauthorized.into());
    }
    if vault.address() != &jar_state.vault || destination.address() == vault.address() {
        return Err(ProgramError::InvalidArgument);
    }
    {
        let vault_state = TokenAccount::from_account_view(vault)?;
        if amount > vault_state.amount() {
            return Err(ProgramError::InsufficientFunds);
        }
        let destination_state = TokenAccount::from_account_view(destination)?;
        if destination_state.mint() != &jar_state.mint {
            return Err(ProgramError::InvalidAccountData);
        }
    }

    let bump_seed = [jar_state.bump];
    let seeds = [
        Seed::from(JAR_SEED),
        Seed::from(jar_state.owner.as_array()),
        Seed::from(jar_state.mint.as_array()),
        Seed::from(&bump_seed),
    ];
    Transfer::new(vault, destination, jar, amount)
        .invoke_signed_with_program(&[Signer::from(&seeds)], token_program.address())
}
