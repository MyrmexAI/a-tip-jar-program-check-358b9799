# Audit report: tip-jar (Pinocchio, SPL Token)

Scope: `programs/tip-jar/src/{lib,instructions,state}.rs`. Focus: withdraw access control, arithmetic, PDA validation.
Verification: `cargo build-sbf --offline` succeeds. `cargo test --offline` passes 13/13: the 6 original tests plus 7 probe tests I added to `tests/tip_jar.rs`.

**Verdict: accept.** No exploitable issue was found in the three focus areas. There are only low and informational notes.

## 1. Withdraw access control: sound

Checks in `withdraw` (`instructions.rs`), in order:
- The owner is a signer (`is_signer`).
- The token program address is exactly SPL Token.
- The jar is loaded through `Jar::load`, which requires that it is owned by this program, has length 98 and has discriminator 1.
- `owner.address() == jar.owner` is checked, otherwise `Unauthorized` (Custom 1).
- `vault.address() == jar.vault`, and the destination is not the vault.
- The destination mint equals `jar.mint`.
- The CPI is signed with the jar PDA seeds (`jar`, owner, mint, stored canonical bump).

Why a forged jar cannot work:
- Only this program can write to program-owned data.
- A keypair account assigned to the program would have zeroed data, so its discriminator is 0, not 1.
- Real jars exist only at PDAs created in `initialize`.

Other points:
- Substituting an attacker's own jar with the victim's vault fails, because `jar.vault` differs from the supplied vault.
- The vault's token authority is the jar PDA, and the program never signs a `SetAuthority`. No other path can move funds out.

Tests covering this: `stranger_cannot_withdraw`, `withdraw_with_forged_jar_fails`, `withdraw_with_other_owners_jar_and_victim_vault_fails`, `fake_token_program_rejected`, `withdraw_to_wrong_mint_destination_fails`.

## 2. Arithmetic: sound

- The program does no add, sub or mul on balances. Amounts are parsed with `u64::from_le_bytes` from exactly 8 bytes, and zero is rejected.
- `amount > vault.amount()` returns `InsufficientFunds` before the CPI. The token program also enforces this.
- Vault balance overflow on tip is enforced by the token program. A vault holding more than `u64::MAX` is not possible.
- The only arithmetic in the code is `offset + 32` in `read_address`, with constant offsets, so it cannot overflow.
- `extreme_amounts_fail` covers `u64::MAX` for tip and withdraw.

## 3. PDA validation: sound

- `initialize` re-derives the jar from `[jar, owner, mint]` and the vault from `[vault, jar]` with `find_program_address`. It uses the canonical bump and stores it.
- Accounts are created with the PDA signer seeds.
- `tip` and `withdraw` bind the vault via `jar.vault`.
- Pre-funded PDAs (a lamport-transfer griefing attempt against `create_account`) do not block initialization. `create_account_with_minimum_balance_signed` handles accounts that already hold lamports. Both probes pass: `prefunded_jar_pda_does_not_block_initialize` and `prefunded_vault_pda_does_not_block_initialize`.
- A second `initialize` fails, because the account already exists (`initialize_twice_fails`).

## Findings (all low or informational)

1. **Low: `initialize` is permissionless.** Any payer can initialize a jar for any owner and mint. The owner is part of the seeds, so the jar still belongs to that owner and the funder bears the rent. The risk is only unsolicited jar creation. This is acceptable by design, and it is worth documenting. Requiring the owner to sign would remove it.
2. **Low: no close or reclaim path.** There is no instruction to close the jar or vault and recover the rent. The lamports in those two accounts are locked permanently. This is a design limitation, not a vulnerability.
3. **Info: mint validity is not checked in `initialize`.** The code only checks that the mint is owned by the token program. A malformed mint is rejected downstream when `InitializeAccount3` runs, and the transaction is atomic, so nothing is left half-created. Explicit `Mint` validation would be defense in depth.
4. **Info: `tip` does not check the source token account's owner.** The token program enforces that the sender is the authority, so this is safe. The source mint is checked against `jar.mint`.
5. **Info: `Jar::load` does not re-derive the jar address.** This is safe because of the ownership and discriminator checks. A comment would help future maintainers avoid regressions.

## Changes made
- `tests/tip_jar.rs`: added 7 regression tests (pre-funded PDAs, wrong-mint destination, forged jar, cross-jar vault substitution, extreme amounts, fake token program). No source changes were needed.
