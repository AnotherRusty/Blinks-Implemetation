use std::collections::HashMap;
use std::str::FromStr;

use base64::{engine::general_purpose::STANDARD as base64, Engine as _};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, message::Message, pubkey::Pubkey, system_instruction,
    transaction::Transaction,
};

use crate::get_latest_blockhash;

pub async fn create_merch_blink_transaction(
    buyer_address: &str,
    payment_splits: HashMap<String, u64>,
) -> Result<String, String> {
    let buyer = Pubkey::from_str(buyer_address)
        .map_err(|e| format!("invalid buyer pubkey {buyer_address}: {e}"))?;

    let mut instructions = vec![ComputeBudgetInstruction::set_compute_unit_price(1_000_000)];
    let mut invalid_pubkeys = vec![];
    for (address, lamports) in payment_splits.iter() {
        match Pubkey::from_str(address) {
            Ok(pubkey) => {
                instructions.push(system_instruction::transfer(&buyer, &pubkey, *lamports))
            }
            Err(e) => invalid_pubkeys.push(format!("  could not parse {address} as pubkey: {e}")),
        }
    }

    if !invalid_pubkeys.is_empty() {
        return Err(format!(
            "invalid recipients:\n{}",
            invalid_pubkeys.join("\n")
        ));
    }

    let (latest_blockhash, _) = get_latest_blockhash().await?;
    let tx = Transaction::new_unsigned(Message::new_with_blockhash(
        &instructions,
        Some(&buyer),
        &latest_blockhash,
    ));
    let serialized_transaction =
        bincode::serialize(&tx).map_err(|e| format!("could not serialize transaction: {e}"))?;

    Ok(base64.encode(serialized_transaction))
}
