extern crate alloc;

use alloc::{collections::BTreeMap, vec::Vec};
use core::{fmt::Display, str::FromStr};
use cosmwasm_minimal_std::{
    Addr, Binary, BlockInfo, CanonicalAddr, Coin, ContractInfo, CosmwasmQueryResult, Empty, Env,
    Event, MessageInfo, Order, QueryResult, SystemResult, Timestamp,
};
use cosmwasm_vm::{executor::*, has::*, memory::*, system::*, transaction::*, vm::*};
use cosmwasm_vm_wasmi::*;
use serde::{Deserialize, Serialize};
use wasmi::CanResume;

#[derive(Debug)]
pub enum SimpleVMError {
    Interpreter(wasmi::Error),
    VMError(WasmiVMError),
    CodeNotFound(CosmwasmCodeId),
    ContractNotFound(BankAccount),
    InvalidAddress,
    InvalidAccountFormat,
    NoCustomQuery,
    NoCustomMessage,
    Unsupported,
    IteratorDoesNotExist,
}
impl From<wasmi::Error> for SimpleVMError {
    fn from(e: wasmi::Error) -> Self {
        Self::Interpreter(e)
    }
}
impl From<WasmiVMError> for SimpleVMError {
    fn from(e: WasmiVMError) -> Self {
        SimpleVMError::VMError(e)
    }
}
impl From<SystemError> for SimpleVMError {
    fn from(e: SystemError) -> Self {
        SimpleVMError::VMError(e.into())
    }
}
impl From<ExecutorError> for SimpleVMError {
    fn from(e: ExecutorError) -> Self {
        SimpleVMError::VMError(e.into())
    }
}
impl From<MemoryReadError> for SimpleVMError {
    fn from(e: MemoryReadError) -> Self {
        SimpleVMError::VMError(e.into())
    }
}
impl From<MemoryWriteError> for SimpleVMError {
    fn from(e: MemoryWriteError) -> Self {
        SimpleVMError::VMError(e.into())
    }
}
impl Display for SimpleVMError {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{:?}", self)
    }
}
impl CanResume for SimpleVMError {
    fn can_resume(&self) -> bool {
        false
    }
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Iter {
    data: Vec<(Vec<u8>, Vec<u8>)>,
    position: usize,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct SimpleWasmiVMStorage {
    data: BTreeMap<Vec<u8>, Vec<u8>>,
    iterators: BTreeMap<u32, Iter>,
}

#[derive(Serialize, Deserialize)]
pub struct SimpleWasmiVMExtension {
    pub storage: BTreeMap<BankAccount, SimpleWasmiVMStorage>,
    pub codes: BTreeMap<CosmwasmCodeId, Vec<u8>>,
    pub contracts: BTreeMap<BankAccount, CosmwasmContractMeta<BankAccount>>,
    pub next_account_id: BankAccount,
    pub transaction_depth: u32,
}

pub struct SimpleWasmiVM<'a> {
    pub host_functions: BTreeMap<WasmiHostFunctionIndex, WasmiHostFunction<Self>>,
    pub executing_module: WasmiModule,
    pub env: Env,
    pub info: MessageInfo,
    pub extension: &'a mut SimpleWasmiVMExtension,
}

impl<'a> WasmiModuleExecutor for SimpleWasmiVM<'a> {
    fn executing_module(&self) -> WasmiModule {
        self.executing_module.clone()
    }
    fn host_function(&self, index: WasmiHostFunctionIndex) -> Option<&WasmiHostFunction<Self>> {
        self.host_functions.get(&index)
    }
}

impl<'a> Pointable for SimpleWasmiVM<'a> {
    type Pointer = u32;
}

impl<'a> ReadableMemory for SimpleWasmiVM<'a> {
    type Error = VmErrorOf<Self>;
    fn read(&self, offset: Self::Pointer, buffer: &mut [u8]) -> Result<(), Self::Error> {
        self.executing_module
            .memory
            .get_into(offset, buffer)
            .map_err(|_| WasmiVMError::LowLevelMemoryReadError.into())
    }
}

impl<'a> WritableMemory for SimpleWasmiVM<'a> {
    type Error = VmErrorOf<Self>;
    fn write(&self, offset: Self::Pointer, buffer: &[u8]) -> Result<(), Self::Error> {
        self.executing_module
            .memory
            .set(offset, buffer)
            .map_err(|_| WasmiVMError::LowLevelMemoryWriteError.into())
    }
}

impl<'a> ReadWriteMemory for SimpleWasmiVM<'a> {}

impl<'a> SimpleWasmiVM<'a> {
    fn load_subvm<R>(
        &mut self,
        address: <Self as VMBase>::Address,
        funds: Vec<Coin>,
        f: impl FnOnce(&mut WasmiVM<SimpleWasmiVM>) -> R,
    ) -> Result<R, VmErrorOf<Self>> {
        let code = (|| {
            let CosmwasmContractMeta { code_id, .. } = self
                .extension
                .contracts
                .get(&address)
                .cloned()
                .ok_or(SimpleVMError::ContractNotFound(address))?;
            self.extension
                .codes
                .get(&code_id)
                .ok_or(SimpleVMError::CodeNotFound(code_id))
                .cloned()
        })()?;
        let host_functions_definitions =
            WasmiImportResolver(host_functions::definitions::<SimpleWasmiVM>());
        let module = new_wasmi_vm(&host_functions_definitions, &code)?;
        let mut sub_vm: WasmiVM<SimpleWasmiVM> = WasmiVM(SimpleWasmiVM {
            host_functions: host_functions_definitions
                .0
                .into_iter()
                .flat_map(|(_, modules)| modules.into_iter().map(|(_, function)| function))
                .collect(),
            executing_module: module,
            env: Env {
                block: self.env.block.clone(),
                transaction: self.env.transaction.clone(),
                contract: ContractInfo {
                    address: address.into(),
                },
            },
            info: MessageInfo {
                sender: self.env.contract.address.clone(),
                funds,
            },
            extension: self.extension,
        });
        Ok(f(&mut sub_vm))
    }
}

#[derive(Debug, Clone)]
pub struct CanonicalAddress(pub CanonicalAddr);

impl TryFrom<Vec<u8>> for CanonicalAddress {
    type Error = SimpleVMError;
    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        Ok(CanonicalAddress(CanonicalAddr(Binary::from(value))))
    }
}

impl From<CanonicalAddress> for Vec<u8> {
    fn from(addr: CanonicalAddress) -> Self {
        addr.0.into()
    }
}

impl From<CanonicalAddress> for CanonicalAddr {
    fn from(addr: CanonicalAddress) -> Self {
        addr.0
    }
}

impl<'a> VMBase for SimpleWasmiVM<'a> {
    type Input<'x> = WasmiInput<'x, WasmiVM<Self>>;
    type Output<'x> = WasmiOutput<'x, WasmiVM<Self>>;
    type QueryCustom = Empty;
    type MessageCustom = Empty;
    type ContractMeta = CosmwasmContractMeta<BankAccount>;
    type Address = BankAccount;
    type CanonicalAddress = CanonicalAddress;
    type StorageKey = Vec<u8>;
    type StorageValue = Vec<u8>;
    type Error = SimpleVMError;

    fn running_contract_meta(&mut self) -> Result<Self::ContractMeta, Self::Error> {
        Ok(self
            .extension
            .contracts
            .get(
                &BankAccount::try_from(self.env.contract.address.clone())
                    .expect("contract address is set by vm, this should never happen"),
            )
            .cloned()
            .expect("contract is inserted by vm, this should never happen"))
    }

    fn set_contract_meta(
        &mut self,
        address: Self::Address,
        contract_meta: Self::ContractMeta,
    ) -> Result<(), Self::Error> {
        let meta = self
            .extension
            .contracts
            .get_mut(&address)
            .ok_or(SimpleVMError::ContractNotFound(address))?;

        *meta = contract_meta;

        Ok(())
    }

    fn contract_meta(&mut self, address: Self::Address) -> Result<Self::ContractMeta, Self::Error> {
        self.extension
            .contracts
            .get_mut(&address)
            .ok_or(SimpleVMError::ContractNotFound(address))
            .cloned()
    }

    fn query_continuation(
        &mut self,
        address: Self::Address,
        message: &[u8],
    ) -> Result<QueryResult, Self::Error> {
        self.load_subvm(address, vec![], |sub_vm| {
            cosmwasm_call::<QueryInput, WasmiVM<SimpleWasmiVM>>(sub_vm, message)
        })?
    }

    fn continue_execute(
        &mut self,
        address: Self::Address,
        funds: Vec<Coin>,
        message: &[u8],
        event_handler: &mut dyn FnMut(Event),
    ) -> Result<Option<Binary>, Self::Error> {
        self.load_subvm(address, funds, |sub_vm| {
            cosmwasm_system_run::<ExecuteInput<Self::MessageCustom>, _>(
                sub_vm,
                message,
                event_handler,
            )
        })?
    }

    fn continue_instantiate(
        &mut self,
        contract_meta: Self::ContractMeta,
        funds: Vec<Coin>,
        message: &[u8],
        event_handler: &mut dyn FnMut(Event),
    ) -> Result<(Self::Address, Option<Binary>), Self::Error> {
        let BankAccount(address) = self.extension.next_account_id;
        self.extension.next_account_id = BankAccount(address + 1);
        self.extension
            .contracts
            .insert(BankAccount(address), contract_meta);

        self.load_subvm(BankAccount(address), funds, |sub_vm| {
            cosmwasm_system_run::<InstantiateInput<Self::MessageCustom>, _>(
                sub_vm,
                message,
                event_handler,
            )
        })?
        .map(|data| (BankAccount(address), data))
    }

    fn continue_migrate(
        &mut self,
        address: Self::Address,
        message: &[u8],
        event_handler: &mut dyn FnMut(Event),
    ) -> Result<Option<Binary>, Self::Error> {
        self.load_subvm(address, vec![], |sub_vm| {
            cosmwasm_system_run::<MigrateInput<Self::MessageCustom>, _>(
                sub_vm,
                message,
                event_handler,
            )
        })?
    }

    fn query_custom(
        &mut self,
        _: Self::QueryCustom,
    ) -> Result<SystemResult<CosmwasmQueryResult>, Self::Error> {
        Err(SimpleVMError::NoCustomQuery)
    }

    fn message_custom(
        &mut self,
        _: Self::MessageCustom,
        _: &mut dyn FnMut(Event),
    ) -> Result<Option<Binary>, Self::Error> {
        Err(SimpleVMError::NoCustomMessage)
    }

    fn query_raw(
        &mut self,
        address: Self::Address,
        key: Self::StorageKey,
    ) -> Result<Option<Self::StorageValue>, Self::Error> {
        Ok(self
            .extension
            .storage
            .get(&address)
            .unwrap_or(&Default::default())
            .data
            .get(&key)
            .cloned())
    }

    fn transfer(&mut self, _: &Self::Address, _: &[Coin]) -> Result<(), Self::Error> {
        Err(SimpleVMError::Unsupported)
    }

    fn burn(&mut self, _: &[Coin]) -> Result<(), Self::Error> {
        Err(SimpleVMError::Unsupported)
    }

    fn balance(&mut self, _: &Self::Address, _: String) -> Result<Coin, Self::Error> {
        Err(SimpleVMError::Unsupported)
    }

    fn all_balance(&mut self, _: &Self::Address) -> Result<Vec<Coin>, Self::Error> {
        Err(SimpleVMError::Unsupported)
    }

    fn query_info(
        &mut self,
        _: Self::Address,
    ) -> Result<cosmwasm_minimal_std::ContractInfoResponse, Self::Error> {
        Err(SimpleVMError::Unsupported)
    }

    fn debug(&mut self, _: Vec<u8>) -> Result<(), Self::Error> {
        Ok(())
    }

    fn db_scan(
        &mut self,
        _: Option<Self::StorageKey>,
        _: Option<Self::StorageKey>,
        _: Order,
    ) -> Result<u32, Self::Error> {
        let contract_addr = self.env.contract.address.clone().try_into()?;
        let mut empty = SimpleWasmiVMStorage::default();
        let storage = self
            .extension
            .storage
            .get_mut(&contract_addr)
            .unwrap_or(&mut empty);

        let data = storage.data.clone().into_iter().collect::<Vec<_>>();
        // Exceeding u32 size is fatal
        let last_id: u32 = storage
            .iterators
            .len()
            .try_into()
            .expect("Found more iterator IDs than supported");

        let new_id = last_id + 1;
        let iter = Iter { data, position: 0 };
        storage.iterators.insert(new_id, iter);

        Ok(new_id)
    }

    fn db_next(
        &mut self,
        iterator_id: u32,
    ) -> Result<(Self::StorageKey, Self::StorageValue), Self::Error> {
        let contract_addr = self.env.contract.address.clone().try_into()?;
        let storage = self
            .extension
            .storage
            .get_mut(&contract_addr)
            .ok_or(SimpleVMError::IteratorDoesNotExist)?;

        let iterator = storage
            .iterators
            .get_mut(&iterator_id)
            .ok_or(SimpleVMError::IteratorDoesNotExist)?;

        let position = iterator.position;
        if iterator.data.len() > position {
            iterator.position += 1;
            Ok(iterator.data[position].clone())
        } else {
            // Empty data works like `None` in rust iterators
            Ok((Default::default(), Default::default()))
        }
    }

    fn secp256k1_verify(&mut self, _: &[u8], _: &[u8], _: &[u8]) -> Result<bool, Self::Error> {
        Err(SimpleVMError::Unsupported)
    }

    fn secp256k1_recover_pubkey(
        &mut self,
        _: &[u8],
        _: &[u8],
        _: u8,
    ) -> Result<Result<Vec<u8>, ()>, Self::Error> {
        Err(SimpleVMError::Unsupported)
    }

    fn ed25519_verify(&mut self, _: &[u8], _: &[u8], _: &[u8]) -> Result<bool, Self::Error> {
        Err(SimpleVMError::Unsupported)
    }

    fn ed25519_batch_verify(
        &mut self,
        _: &[&[u8]],
        _: &[&[u8]],
        _: &[&[u8]],
    ) -> Result<bool, Self::Error> {
        Err(SimpleVMError::Unsupported)
    }

    fn addr_validate(&mut self, input: &str) -> Result<Result<(), Self::Error>, Self::Error> {
        Ok(match BankAccount::try_from(input.to_string()) {
            Ok(_) => Ok(()),
            Err(_) => Err(SimpleVMError::InvalidAddress),
        })
    }

    fn addr_canonicalize(
        &mut self,
        input: &str,
    ) -> Result<Result<Self::CanonicalAddress, Self::Error>, Self::Error> {
        if let Ok(account) = BankAccount::try_from(input.to_string()) {
            Ok(account.0.to_le_bytes().to_vec().try_into())
        } else {
            Ok(Err(SimpleVMError::InvalidAddress))
        }
    }

    fn addr_humanize(
        &mut self,
        addr: &Self::CanonicalAddress,
    ) -> Result<Result<Self::Address, Self::Error>, Self::Error> {
        let le_bytes: [u8; 16] = match Into::<Vec<u8>>::into(addr.clone()).try_into() {
            Ok(le_bytes) => le_bytes,
            Err(_) => return Ok(Err(SimpleVMError::InvalidAddress)),
        };

        Ok(Ok(BankAccount(u128::from_le_bytes(le_bytes))))
    }

    fn db_read(
        &mut self,
        key: Self::StorageKey,
    ) -> Result<Option<Self::StorageValue>, Self::Error> {
        let contract_addr = self.env.contract.address.clone().try_into()?;
        let empty = SimpleWasmiVMStorage::default();
        Ok(self
            .extension
            .storage
            .get(&contract_addr)
            .unwrap_or(&empty)
            .data
            .get(&key)
            .cloned())
    }

    fn db_write(
        &mut self,
        key: Self::StorageKey,
        value: Self::StorageValue,
    ) -> Result<(), Self::Error> {
        let contract_addr = self.env.contract.address.clone().try_into()?;
        self.extension
            .storage
            .entry(contract_addr)
            .or_insert_with(SimpleWasmiVMStorage::default)
            .data
            .insert(key, value);
        Ok(())
    }

    fn db_remove(&mut self, key: Self::StorageKey) -> Result<(), Self::Error> {
        let contract_addr = self.env.contract.address.clone().try_into()?;
        self.extension
            .storage
            .get_mut(&contract_addr)
            .map(|contract_storage| contract_storage.data.remove(&key));
        Ok(())
    }

    fn abort(&mut self, message: String) -> Result<(), Self::Error> {
        Err(SimpleVMError::from(WasmiVMError::from(
            SystemError::ContractExecutionFailure(message),
        )))
    }

    // Our vm is so nice to people that it doesn't charge them
    fn charge(&mut self, _: VmGas) -> Result<(), Self::Error> {
        Ok(())
    }

    fn gas_checkpoint_push(&mut self, _: VmGasCheckpoint) -> Result<(), Self::Error> {
        Ok(())
    }

    fn gas_checkpoint_pop(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn gas_ensure_available(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct BankAccount(pub u128);

impl FromStr for BankAccount {
    type Err = SimpleVMError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        BankAccount::try_from(String::from(s))
    }
}

impl TryFrom<Addr> for BankAccount {
    type Error = SimpleVMError;
    fn try_from(value: Addr) -> Result<Self, Self::Error> {
        value.to_string().try_into()
    }
}

impl TryFrom<String> for BankAccount {
    type Error = SimpleVMError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Ok(BankAccount(
            u128::from_str(&value).map_err(|_| SimpleVMError::InvalidAccountFormat)?,
        ))
    }
}

impl From<BankAccount> for Addr {
    fn from(BankAccount(account): BankAccount) -> Self {
        Addr::unchecked(format!("{}", account))
    }
}

impl<'a> Has<Env> for SimpleWasmiVM<'a> {
    fn get(&self) -> Env {
        self.env.clone()
    }
}
impl<'a> Has<MessageInfo> for SimpleWasmiVM<'a> {
    fn get(&self) -> MessageInfo {
        self.info.clone()
    }
}

impl<'a> Transactional for SimpleWasmiVM<'a> {
    type Error = SimpleVMError;
    fn transaction_begin(&mut self) -> Result<(), Self::Error> {
        self.extension.transaction_depth += 1;
        Ok(())
    }
    fn transaction_commit(&mut self) -> Result<(), Self::Error> {
        self.extension.transaction_depth -= 1;
        Ok(())
    }
    fn transaction_rollback(&mut self) -> Result<(), Self::Error> {
        self.extension.transaction_depth -= 1;
        Ok(())
    }
}

pub fn create_simple_vm<'a>(
    sender: BankAccount,
    address: BankAccount,
    funds: Vec<Coin>,
    code: &[u8],
    extension: &'a mut SimpleWasmiVMExtension,
) -> WasmiVM<SimpleWasmiVM<'a>> {
    let host_functions_definitions = WasmiImportResolver(host_functions::definitions());
    let module = new_wasmi_vm(&host_functions_definitions, code).unwrap();

    WasmiVM(SimpleWasmiVM {
        host_functions: host_functions_definitions
            .0
            .clone()
            .into_iter()
            .flat_map(|(_, modules)| modules.into_iter().map(|(_, function)| function))
            .collect(),
        executing_module: module,
        env: Env {
            block: BlockInfo {
                height: 0xDEADC0DE,
                time: Timestamp(0),
                chain_id: "abstract-test".into(),
            },
            transaction: None,
            contract: ContractInfo {
                address: address.into(),
            },
        },
        info: MessageInfo {
            sender: sender.into(),
            funds,
        },
        extension,
    })
}
