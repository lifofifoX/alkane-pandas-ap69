use metashrew_support::index_pointer::KeyValuePointer;
use metashrew_support::compat::to_arraybuffer_layout;
use metashrew_support::utils::consensus_decode;

use alkanes_runtime::{
  declare_alkane, message::MessageDispatch, storage::StoragePointer, token::Token,
  runtime::AlkaneResponder
};

use alkanes_support::{
  id::AlkaneId,
  parcel::AlkaneTransfer, response::CallResponse,
  utils::overflow_error
};

use bitcoin::hashes::Hash;
use bitcoin::{Txid, Transaction};

use anyhow::{anyhow, Result};
use std::sync::Arc;

// We could validate pandas ids against the collection contract 2:614, but we cbf. Save fuel.
mod panda_ids;
use panda_ids::PANDA_IDS;

mod panda_image;
use panda_image::PANDA_IMAGE;

const PANDA_BLOCK: u128 = 0x2;

const BAMBOO_PER_PANDA: u128 = 10_000_000_000_000;
const PANDA_SUPPLY: u128 = 10_000;
const BAMBOO_CAP: u128 = PANDA_SUPPLY * BAMBOO_PER_PANDA;

#[derive(Default)]
pub struct BambooSwap(());

impl AlkaneResponder for BambooSwap {}

#[derive(MessageDispatch)]
enum BambooSwapMessage {
  #[opcode(0)]
  Initialize,

  #[opcode(42)]
  PandaToBamboo,

  #[opcode(69)]
  BambooToPanda,

  #[opcode(77)]
  MintTokens,

  #[opcode(99)]
  #[returns(String)]
  GetName,

  #[opcode(100)]
  #[returns(String)]
  GetSymbol,

  #[opcode(101)]
  #[returns(u128)]
  GetTotalSupply,
  
  #[opcode(102)]
  #[returns(u128)]
  GetCap,

  #[opcode(103)]
  #[returns(u128)]
  GetMinted,

  #[opcode(104)]
  #[returns(u128)]
  GetValuePerMint,

  #[opcode(1000)]
  #[returns(Vec<u8>)]
  GetData,

  #[opcode(2000)]
  #[returns(u128)]
  GetPandaStackCount,

  #[opcode(2001)]
  #[returns(Vec<Vec<u8>>)]
  GetPandaStack,

  #[opcode(2002)]
  #[returns(String)]
  GetPandaStackJson,
}

impl Token for BambooSwap {
  fn name(&self) -> String {
    return String::from("BAMBOO")
  }

  fn symbol(&self) -> String {
    return String::from("BAMBOO");
  }
}

impl BambooSwap {
  fn initialize(&self) -> Result<CallResponse> {
    self.observe_initialization()?;
    let context = self.context()?;

    let response = CallResponse::forward(&context.incoming_alkanes);
    Ok(response)
  }

  fn get_name(&self) -> Result<CallResponse> {
    let context = self.context()?;
    let mut response = CallResponse::forward(&context.incoming_alkanes);

    response.data = self.name().into_bytes();

    Ok(response)
  }

  fn get_symbol(&self) -> Result<CallResponse> {
    let context = self.context()?;
    let mut response = CallResponse::forward(&context.incoming_alkanes);

    response.data = self.symbol().into_bytes();

    Ok(response)
  }

  fn get_total_supply(&self) -> Result<CallResponse> {
    let context = self.context()?;
    let mut response = CallResponse::forward(&context.incoming_alkanes);

    response.data = self.total_supply().to_le_bytes().to_vec();

    Ok(response)
  }

  fn get_cap(&self) -> Result<CallResponse> {
    let context = self.context()?;
    let mut response = CallResponse::forward(&context.incoming_alkanes);

    response.data = BAMBOO_CAP.to_le_bytes().to_vec();

    Ok(response)
  }

  fn get_minted(&self) -> Result<CallResponse> {
    let context = self.context()?;
    let mut response = CallResponse::forward(&context.incoming_alkanes);

    response.data = self.instances_count().to_le_bytes().to_vec();

    Ok(response)
  }

  fn get_value_per_mint(&self) -> Result<CallResponse> {
    let context = self.context()?;
    let mut response = CallResponse::forward(&context.incoming_alkanes);

    response.data = BAMBOO_PER_PANDA.to_le_bytes().to_vec();

    Ok(response)
  }

  fn get_data(&self) -> Result<CallResponse> {
    let context = self.context()?;
    let mut response = CallResponse::forward(&context.incoming_alkanes);

    response.data = PANDA_IMAGE.to_vec();

    Ok(response)
  }

  fn total_supply_pointer(&self) -> StoragePointer {
    StoragePointer::from_keyword("/total_supply")
  }

  fn total_supply(&self) -> u128 {
    self.total_supply_pointer().get_value::<u128>()
  }

  fn set_total_supply(&self, v: u128) {
    self.total_supply_pointer().set_value::<u128>(v);
  }

  fn increase_total_supply(&self, v: u128) -> Result<()> {
    self.set_total_supply(overflow_error(self.total_supply().checked_add(v))?);
    Ok(())
  }

  fn decrease_total_supply(&self, v: u128) -> Result<()> {
    self.set_total_supply(overflow_error(self.total_supply().checked_sub(v))?);
    Ok(())
  }

  fn is_valid_panda(&self, id: &AlkaneId) -> Result<bool> {
    Ok(id.block == PANDA_BLOCK && PANDA_IDS.contains(&id.tx))
  }

  fn panda_to_bamboo(&self) -> Result<CallResponse> {
    let context = self.context()?;
    let txid = self.transaction_id()?;

    // Enforce one swap per transaction
    if self.has_tx_hash(&txid) {
      return Err(anyhow!("Transaction already used for swap"));
    }
    
    if context.incoming_alkanes.0.is_empty() {
      return Err(anyhow!("Must send at least 1 Panda to swap"));
    }

    self.add_tx_hash(&txid)?;

    let mut response = CallResponse::default();
    let mut total_bamboo = 0u128;

    for alkane in context.incoming_alkanes.0.iter() {
      if !self.is_valid_panda(&alkane.id)? {
        return Err(anyhow!("Invalid Panda ID"));
      }

      self.add_instance(&alkane.id)?;

      total_bamboo = total_bamboo.checked_add(BAMBOO_PER_PANDA)
        .ok_or_else(|| anyhow!("Bamboo amount overflow"))?;
    }

    self.increase_total_supply(total_bamboo)?;

    response.alkanes.0.push(AlkaneTransfer {
      id: context.myself.clone(),
      value: total_bamboo,
    }); 

    Ok(response)
  }

  fn bamboo_to_panda(&self) -> Result<CallResponse> {
    let context = self.context()?;
    let txid = self.transaction_id()?;

    // Enforce one swap per transaction
    if self.has_tx_hash(&txid) {
      return Err(anyhow!("Transaction already used for swap"));
    }
    
    if context.incoming_alkanes.0.len() != 1 {
      return Err(anyhow!("Must send $BAMBOO to swap"));
    }

    let transfer = context.incoming_alkanes.0[0].clone();
    if transfer.id != context.myself.clone() {
      return Err(anyhow!("Supplied alkane is not $BAMBOO"));
    }

    if transfer.value < BAMBOO_PER_PANDA {
      return Err(anyhow!(
        "Not enough $BAMBOO supplied to swap"
      ));
    }

    let panda_count = transfer.value / BAMBOO_PER_PANDA;
    let bamboo_used = panda_count * BAMBOO_PER_PANDA;
    let bamboo_change = transfer.value % BAMBOO_PER_PANDA;

    let count = self.instances_count();
    if count < panda_count {
      return Err(anyhow!("Not enough Pandas available to swap"));
    }

    self.add_tx_hash(&txid)?;

    let mut response = CallResponse::default();

    self.decrease_total_supply(bamboo_used)?;
  
    // Pandas
    for _ in 0..panda_count {
      response.alkanes.0.push(AlkaneTransfer {
        id: self.pop_instance()?,
        value: 1u128,
      });
    }

    // Change
    if bamboo_change > 0 {
      response.alkanes.0.push(AlkaneTransfer {
        id: context.myself.clone(),
        value: bamboo_change,
      });
    }

    Ok(response)
  }

  fn mint_tokens(&self) -> Result<CallResponse> {
    return Err(anyhow!("Minting not implemented"));
  }

  fn get_panda_stack_count(&self) -> Result<CallResponse> {
    let context = self.context()?;
    let mut response = CallResponse::forward(&context.incoming_alkanes);

    response.data = self.instances_count().to_le_bytes().to_vec();

    Ok(response)
  }

  fn get_panda_stack(&self) -> Result<CallResponse> {
    let context = self.context()?;
    let mut response = CallResponse::forward(&context.incoming_alkanes);

    let count = self.instances_count();
    let mut panda_ids = Vec::new();

    for i in 0..count {
      let instance_id = self.lookup_instance(i)?;
      let mut bytes = Vec::with_capacity(32);
      bytes.extend_from_slice(&instance_id.block.to_le_bytes());
      bytes.extend_from_slice(&instance_id.tx.to_le_bytes());
      panda_ids.push(bytes);
    }

    let mut flattened = Vec::new();
    for bytes in panda_ids {
      flattened.extend(bytes);
    }

    response.data = flattened;
    Ok(response)
  }

  fn get_panda_stack_json(&self) -> Result<CallResponse> {
    let context = self.context()?;
    let mut response = CallResponse::forward(&context.incoming_alkanes);

    let count = self.instances_count();
    let mut panda_ids = Vec::new();

    for i in 0..count {
      let instance_id = self.lookup_instance(i)?;
      panda_ids.push(format!("{}:{}", instance_id.block, instance_id.tx));
    }

    response.data = serde_json::to_string(&panda_ids)?.into_bytes();
    Ok(response)
  }

  fn instances_pointer(&self) -> StoragePointer {
    StoragePointer::from_keyword("/instances")
  }

  fn instances_count(&self) -> u128 {
    self.instances_pointer().get_value::<u128>()
  }

  fn set_instances_count(&self, count: u128) {
    self.instances_pointer().set_value::<u128>(count);
  }

  fn add_instance(&self, instance_id: &AlkaneId) -> Result<u128> {
    let count = self.instances_count();
    let new_count = count.checked_add(1)
      .ok_or_else(|| anyhow!("instances count overflow"))?;

    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(&instance_id.block.to_le_bytes());
    bytes.extend_from_slice(&instance_id.tx.to_le_bytes());

    let bytes_vec = new_count.to_le_bytes().to_vec();
    let mut instance_pointer = self.instances_pointer().select(&bytes_vec);
    instance_pointer.set(Arc::new(bytes));
    
    self.set_instances_count(new_count);
    
    Ok(new_count)
  }

  fn pop_instance(&self) -> Result<AlkaneId> {
    let count = self.instances_count();

    let new_count = count.checked_sub(1)
      .ok_or_else(|| anyhow!("instances count underflow"))?;

    let instance_id = self.lookup_instance(count - 1)?;
    
    // Remove the instance by setting it to empty
    let bytes_vec = count.to_le_bytes().to_vec();
    let mut instance_pointer = self.instances_pointer().select(&bytes_vec);
    instance_pointer.set(Arc::new(Vec::new()));
    
    self.set_instances_count(new_count);
    
    Ok(instance_id)
  }

  fn lookup_instance(&self, index: u128) -> Result<AlkaneId> {
    let bytes_vec = (index + 1).to_le_bytes().to_vec();
    let instance_pointer = self.instances_pointer().select(&bytes_vec);
    
    let bytes = instance_pointer.get();
    if bytes.len() != 32 {
      return Err(anyhow!("Invalid instance data length"));
    }

    let block_bytes = &bytes[..16];
    let tx_bytes = &bytes[16..];

    let block = u128::from_le_bytes(block_bytes.try_into().unwrap());
    let tx = u128::from_le_bytes(tx_bytes.try_into().unwrap());

    Ok(AlkaneId { block, tx })
  }

  fn transaction_id(&self) -> Result<Txid> {
    Ok(
      consensus_decode::<Transaction>(&mut std::io::Cursor::new(self.transaction()))?
        .compute_txid(),
    )
  }

  fn has_tx_hash(&self, txid: &Txid) -> bool {
    StoragePointer::from_keyword("/tx-hashes/")
      .select(&txid.as_byte_array().to_vec())
      .get_value::<u8>()
      == 1
  }

  fn add_tx_hash(&self, txid: &Txid) -> Result<()> {
    StoragePointer::from_keyword("/tx-hashes/")
      .select(&txid.as_byte_array().to_vec())
      .set_value::<u8>(0x01);

    Ok(())
  }
}

declare_alkane! {
  impl AlkaneResponder for BambooSwap {
    type Message = BambooSwapMessage;
  }
}
