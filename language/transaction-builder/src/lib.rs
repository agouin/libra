// Copyright (c) The Libra Core Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

use libra_types::{
    access_path::AccessPath,
    account_address::AccountAddress,
    block_metadata::BlockMetadata,
    on_chain_config::{LibraVersion, VMPublishingOption},
    transaction::{
        authenticator::AuthenticationKey, ChangeSet, Script, Transaction, TransactionArgument,
    },
    write_set::{WriteOp, WriteSetMut},
};
use mirai_annotations::*;
use move_core_types::language_storage::TypeTag;
use std::convert::TryFrom;
use stdlib::{transaction_scripts::StdlibScript, StdLibOptions};
use vm::access::ModuleAccess;

fn validate_auth_key_prefix(auth_key_prefix: &[u8]) {
    let auth_key_prefix_length = auth_key_prefix.len();
    checked_assume!(
        auth_key_prefix_length == 0
            || auth_key_prefix_length == AuthenticationKey::LENGTH - AccountAddress::LENGTH,
        "Bad auth key prefix length {}",
        auth_key_prefix_length
    );
}

macro_rules! to_rust_ty {
    (U64) => { u64 };
    (Address) => { AccountAddress };
    (Bytes) => { Vec<u8> };
    (Bool) => { bool };
}

macro_rules! to_txn_arg {
    (U64, $param_name: ident) => {
        TransactionArgument::U64($param_name)
    };
    (Address, $param_name: ident) => {
        TransactionArgument::Address($param_name)
    };
    (Bytes, $param_name: ident) => {
        TransactionArgument::U8Vector($param_name)
    };
    (Bool, $param_name: ident) => {
        TransactionArgument::Bool($param_name)
    };
}

macro_rules! encode_txn_script {
    (name: $name:ident,
     type_arg: $ty_arg_name:ident,
     args: [$($arg_name:ident: $arg_ty:ident),*],
     script: $script_name:ident,
     doc: $comment:literal
    ) => {
        #[doc=$comment]
        pub fn $name($ty_arg_name: TypeTag, $($arg_name: to_rust_ty!($arg_ty),)*) -> Script {
            encode_txn_script!([$ty_arg_name], [$($arg_name: $arg_ty),*], $script_name)
        }
    };
    (name: $name:ident,
     args: [$($arg_name:ident: $arg_ty:ident),*],
     script: $script_name:ident,
     doc: $comment:literal
    ) => {
        #[doc=$comment]
        pub fn $name($($arg_name: to_rust_ty!($arg_ty),)*) -> Script {
            encode_txn_script!([], [$($arg_name: $arg_ty),*], $script_name)
        }
    };
    ([$($ty_arg_name:ident),*],
     [$($arg_name:ident: $arg_ty:ident),*],
     $script_name:ident
    ) => {
            Script::new(
                StdlibScript::$script_name.compiled_bytes().into_vec(),
                vec![$($ty_arg_name),*],
                vec![
                    $(to_txn_arg!($arg_ty, $arg_name),)*
                ],
            )
    };
}

/// Encode `stdlib_script` with arguments `args`.
/// Note: this is not type-safe; the individual type-safe wrappers below should be used when
/// possible.
pub fn encode_stdlib_script(
    stdlib_script: StdlibScript,
    type_args: Vec<TypeTag>,
    args: Vec<TransactionArgument>,
) -> Script {
    Script::new(stdlib_script.compiled_bytes().into_vec(), type_args, args)
}

encode_txn_script! {
    name: encode_add_validator_script,
    args: [new_validator: Address],
    script: AddValidator,
    doc: "Encode a program adding `new_validator` to the pending validator set. Fails if the\
          `new_validator` address is already in the validator set, already in the pending validator set,\
          or does not have a `ValidatorConfig` resource stored at the address"
}

encode_txn_script! {
    name: encode_burn_script,
    type_arg: type_,
    args: [nonce: U64, preburn_address: Address],
    script: Burn,
    doc: "Permanently destroy the coins stored in the oldest burn request under the `Preburn`\
          resource stored at `preburn_address`. This will only succeed if the sender has a\
          `MintCapability` stored under their account and `preburn_address` has a pending burn request"
}

encode_txn_script! {
    name: encode_burn_txn_fees_script,
    type_arg: currency,
    args: [],
    script: BurnTxnFees,
    doc: "Burn transaction fees that have been collected in the given `currency`,\
          and relinquish to the association. The currency must be non-synthetic."
}

encode_txn_script! {
    name: encode_cancel_burn_script,
    type_arg: type_,
    args: [preburn_address: Address],
    script: CancelBurn,
    doc: "Cancel the oldest burn request from `preburn_address` and return the funds to\
          `preburn_address`.  Fails if the sender does not have a published `MintCapability`."
}

encode_txn_script! {
    name: encode_transfer_with_metadata_script,
    type_arg: coin_type,
    args: [recipient_address: Address, amount: U64, metadata: Bytes, metadata_signature: Bytes],
    script: PeerToPeerWithMetadata,
    doc: "Encode a program transferring `amount` coins to `recipient_address` with (optional)\
          associated metadata `metadata` and (optional) `signature` on the metadata, amount, and\
          sender address. The `metadata` and `signature` parameters are only required if\
          `amount` >= 1000 LBR and the sender and recipient of the funds are two distinct VASPs.\
          Fails if there is no account at the recipient address or if the sender's balance is lower\
          than `amount`"
}

encode_txn_script! {
    name: encode_preburn_script,
    type_arg: type_,
    args: [amount: U64],
    script: Preburn,
    doc: "Preburn `amount` coins from the sender's account. This will only succeed if the sender\
          already has a published `Preburn` resource."
}

encode_txn_script! {
    name: encode_publish_shared_ed25519_public_key_script,
    args: [public_key: Bytes],
    script: PublishSharedEd2551PublicKey,
    doc: "(1) Rotate the authentication key of the sender to `public_key`\
          (2) Publish a resource containing a 32-byte ed25519 public key and the rotation capability\
          of the sender under the sender's address.\
          Aborts if the sender already has a `SharedEd25519PublicKey` resource.\
          Aborts if the length of `new_public_key` is not 32."
}

encode_txn_script! {
    name: encode_add_currency_to_account_script,
    type_arg: currency,
    args: [],
    script: AddCurrencyToAccount,
    doc: "Add the currency identified by the type `currency` to the sending accounts.\
          Aborts if the account already holds a balance fo `currency` type."
}

encode_txn_script! {
    name: encode_register_preburner_script,
    type_arg: type_,
    args: [],
    script: RegisterPreburner,
    doc: "Publish a newly created `Preburn` resource under the sender's account.\
          This will fail if the sender already has a published `Preburn` resource."
}

encode_txn_script! {
    name: encode_register_validator_script,
    args: [
        consensus_pubkey: Bytes,
        validator_network_identity_pubkey: Bytes,
        validator_network_address: Bytes,
        fullnodes_network_identity_pubkey: Bytes,
        fullnodes_network_address: Bytes
    ],
    script: RegisterValidator,
    doc: "Encode a program registering the sender as a candidate validator with the given key information.\
         `network_identity_pubkey` should be a X25519 public key\
         `consensus_pubkey` should be a Ed25519 public c=key."
}

encode_txn_script! {
    name: encode_remove_validator_script,
    args: [to_remove: Address],
    script: RemoveValidator,
    doc: "Encode a program adding `to_remove` to the set of pending validator removals. Fails if\
          the `to_remove` address is already in the validator set or already in the pending removals."
}

encode_txn_script! {
    name: encode_rotate_compliance_public_key_script,
    args: [new_key: Bytes],
    script: RotateCompliancePublicKey,
    doc: "Encode a program that rotates `vasp_root_addr`'s compliance public key to `new_key`."
}

encode_txn_script! {
    name: encode_rotate_base_url_script,
    args: [new_url: Bytes],
    script: RotateBaseUrl,
    doc: "Encode a program that rotates `vasp_root_addr`'s base URL to `new_url`."
}

encode_txn_script! {
    name: encode_rotate_consensus_pubkey_script,
    args: [new_key: Bytes],
    script: RotateConsensusPubkey,
    doc: "Encode a program that rotates the sender's consensus public key to `new_key`."
}

encode_txn_script! {
    name: rotate_authentication_key_script,
    args: [new_hashed_key: Bytes],
    script: RotateAuthenticationKey,
    doc: "Encode a program that rotates the sender's authentication key to `new_key`. `new_key`\
          should be a 256 bit sha3 hash of an ed25519 public key."
}

encode_txn_script! {
    name: encode_rotate_shared_ed25519_public_key_script,
    args: [new_public_key: Bytes],
    script: RotateSharedEd2551PublicKey,
    doc: "(1) rotate the public key stored in the sender's `SharedEd25519PublicKey` resource to\
          `new_public_key`\
          (2) rotate the authentication key using the capability stored in the sender's\
          `SharedEd25519PublicKey` to a new value derived from `new_public_key`\
          Aborts if the sender does not have a `SharedEd25519PublicKey` resource.\
          Aborts if the length of `new_public_key` is not 32."
}

// TODO: this should go away once we are no longer using it in tests
/// Encode a program creating `amount` coins for sender
pub fn encode_mint_script(
    token: TypeTag,
    sender: &AccountAddress,
    auth_key_prefix: Vec<u8>,
    amount: u64,
) -> Script {
    validate_auth_key_prefix(&auth_key_prefix);
    Script::new(
        StdlibScript::Mint.compiled_bytes().into_vec(),
        vec![token],
        vec![
            TransactionArgument::Address(*sender),
            TransactionArgument::U8Vector(auth_key_prefix),
            TransactionArgument::U64(amount),
        ],
    )
}

/// Encode a program creating `amount` LBR for `address`
pub fn encode_mint_lbr_to_address_script(
    address: &AccountAddress,
    auth_key_prefix: Vec<u8>,
    amount: u64,
) -> Script {
    validate_auth_key_prefix(&auth_key_prefix);
    Script::new(
        StdlibScript::MintLbrToAddress.compiled_bytes().into_vec(),
        vec![],
        vec![
            TransactionArgument::Address(*address),
            TransactionArgument::U8Vector(auth_key_prefix),
            TransactionArgument::U64(amount),
        ],
    )
}

pub fn encode_publishing_option_script(config: VMPublishingOption) -> Script {
    let bytes = lcs::to_bytes(&config).expect("Cannot deserialize VMPublishingOption");
    Script::new(
        StdlibScript::ModifyPublishingOption
            .compiled_bytes()
            .into_vec(),
        vec![],
        vec![TransactionArgument::U8Vector(bytes)],
    )
}

pub fn encode_update_libra_version(libra_version: LibraVersion) -> Script {
    Script::new(
        StdlibScript::UpdateLibraVersion.compiled_bytes().into_vec(),
        vec![],
        vec![TransactionArgument::U64(libra_version.major as u64)],
    )
}

// TODO: this should go away once we are no longer using it in tests
pub fn encode_block_prologue_script(block_metadata: BlockMetadata) -> Transaction {
    Transaction::BlockMetadata(block_metadata)
}

// TODO: delete and use StdlibScript::try_from directly if it's ok to drop the "_transaction"?
/// Returns a user friendly mnemonic for the transaction type if the transaction is
/// for a known, white listed, transaction.
pub fn get_transaction_name(code: &[u8]) -> String {
    StdlibScript::try_from(code).map_or("<unknown transaction>".to_string(), |name| {
        format!("{}_transaction", name)
    })
}

//...........................................................................
// on-chain LBR scripts
//...........................................................................

encode_txn_script! {
    name: encode_mint_lbr,
    args: [amount_lbr: U64],
    script: MintLbr,
    doc: "Mints `amount_lbr` LBR from the sending account's constituent coins and deposits the\
          resulting LBR into the sending account."
}

encode_txn_script! {
    name: encode_unmint_lbr,
    args: [amount_lbr: U64],
    script: UnmintLbr,
    doc: "Unmints `amount_lbr` LBR from the sending account into the constituent coins and deposits\
          the resulting coins into the sending account."
}

//...........................................................................
//  Association-related scripts
//...........................................................................

encode_txn_script! {
    name: encode_update_exchange_rate,
    type_arg: currency,
    args: [new_exchange_rate_denominator: U64, new_exchange_rate_numerator: U64],
    script: UpdateExchangeRate,
    doc: "Updates the on-chain exchange rate to LBR for the given `currency` to be given by\
         `new_exchange_rate_denominator/new_exchange_rate_numerator`."
}

encode_txn_script! {
    name: encode_update_minting_ability,
    type_arg: currency,
    args: [allow_minting: Bool],
    script: UpdateMintingAbility,
    doc: "Allows--true--or disallows--false--minting of `currency` based upon `allow_minting`."
}

//...........................................................................
// VASP-related scripts
//...........................................................................

encode_txn_script! {
    name: encode_create_parent_vasp_account,
    type_arg: currency,
    args: [address: Address, auth_key_prefix: Bytes, human_name: Bytes, base_url: Bytes, compliance_public_key: Bytes, add_all_currencies: Bool],
    script: CreateParentVaspAccount,
    doc: "Create an account with the ParentVASP role at `address` with authentication key\
          `auth_key_prefix` | `new_account_address` and a 0 balance of type `currency`. If\
          `add_all_currencies` is true, 0 balances for all available currencies in the system will\
          also be added. This can only be invoked by an Association account."
}

encode_txn_script! {
    name: encode_create_child_vasp_account,
    type_arg: currency,
    args: [address: Address, auth_key_prefix: Bytes, add_all_currencies: Bool, initial_balance: U64],
    script: CreateChildVaspAccount,
    doc: "Create an account with the ChildVASP role at `address` with authentication key\
          `auth_key_prefix` | `new_account_address` and `initial_balance` of type `currency`\
          transferred from the sender. If `add_all_currencies` is true, 0 balances for all\
          available currencies in the system will also be added to the account. This account will\
          be a child of the transaction sender, which must be a ParentVASP."
}

//...........................................................................
// Treasury Compliance Scripts
//...........................................................................

encode_txn_script! {
    name: encode_tiered_mint,
    type_arg: coin_type,
    args: [nonce: U64, designated_dealer_address: Address, mint_amount: U64, tier_index: U64],
    script: TieredMint,
    doc: "Mints 'mint_amount' to 'designated_dealer_address' for 'tier_index' tier.\
          Max valid tier index is 3 since there are max 4 tiers per DD.
          Sender should be treasury compliance account and receiver authorized DD"
}

encode_txn_script! {
    name: encode_create_designated_dealer,
    type_arg: coin_type,
    args: [nonce: U64, new_account_address: Address, auth_key_prefix: Bytes],
    script: CreateDesignatedDealer,
    doc: "Creates designated dealer at 'new_account_address"
}

encode_txn_script! {
    name: encode_freeze_account,
    args: [nonce: U64, addr: Address],
    script: FreezeAccount,
    doc: "Freezes account with address addr."
}

encode_txn_script! {
    name: encode_unfreeze_account,
    args: [nonce: U64, addr: Address],
    script: UnfreezeAccount,
    doc: "Unfreezes account with address addr."
}

encode_txn_script! {
    name: encode_rotate_authentication_key_script_with_nonce,
    args: [nonce: U64, new_hashed_key: Bytes],
    script: RotateAuthenticationKeyWithNonce,
    doc: "Encode a program that rotates the sender's authentication key to `new_key`. `new_key`\
          should be a 256 bit sha3 hash of an ed25519 public key. This script also takes nonce"

}
//...........................................................................
// WriteSets
//...........................................................................

pub fn encode_stdlib_upgrade_transaction(option: StdLibOptions) -> ChangeSet {
    let mut write_set = WriteSetMut::new(vec![]);
    let stdlib = stdlib::stdlib_modules(option);
    for module in stdlib {
        let mut bytes = vec![];
        module
            .serialize(&mut bytes)
            .expect("Failed to serialize module");
        write_set.push((
            AccessPath::code_access_path(&module.self_id()),
            WriteOp::Value(bytes),
        ));
    }
    ChangeSet::new(
        write_set.freeze().expect("Failed to create writeset"),
        vec![],
    )
}
