extern crate alloc;

use crate::simple_wasmi_vm::*;
use crate::state::VM_STATE;
use crate::{
    error::ContractError,
    msg::{ExecuteMsg, InstantiateMsg, QueryMsg},
};
use alloc::collections::BTreeMap;
#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{Binary, Deps, DepsMut, Env, Event, MessageInfo, Response, StdError, StdResult};
use cosmwasm_vm::{executor::*, system::*};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let mut codes: BTreeMap<CosmwasmCodeId, Vec<u8>> = BTreeMap::new();
    let mut contracts: BTreeMap<BankAccount, CosmwasmContractMeta<BankAccount>> = BTreeMap::new();
    msg.contracts.iter().for_each(|contract| {
        codes.insert(contract.code_id, contract.code.clone());
        contracts.insert(
            BankAccount(contract.address),
            CosmwasmContractMeta {
                code_id: contract.code_id,
                admin: None,
                label: "".into(),
            },
        );
    });

    let extension = bincode::serialize(&SimpleWasmiVMExtension {
        storage: Default::default(),
        codes,
        contracts,
        next_account_id: BankAccount(msg.next_account_id),
        transaction_depth: 0,
    })
    .map_err(|_| ContractError::Unexpected)?;

    VM_STATE.save(deps.storage, &extension)?;
    Ok(Response::new())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Instantiate {
            sender,
            code_id,
            instantiate_msg,
        } => instantiate_contract(deps, BankAccount(sender), code_id, instantiate_msg),
        ExecuteMsg::Execute {
            sender,
            contract_address,
            execute_msg,
        } => execute_contract(
            deps,
            BankAccount(sender),
            BankAccount(contract_address),
            execute_msg,
        ),
    }
}

fn execute_entrypoint<I, V>(vm: &mut V, msg: &[u8]) -> Result<Response, ContractError>
where
    V: CosmwasmCallVM<I>,
{
    let (_, events) = cosmwasm_system_entrypoint::<I, V>(vm, msg).unwrap();
    let mut response = Response::default();
    for e in events {
        let mut event = Event::new(e.ty);
        for attr in e.attributes {
            event = event.add_attribute(format!("virtual-vm.{}", attr.key), attr.value);
        }
        response = response.add_event(event);
    }
    Ok(response)
}

fn execute_contract(
    deps: DepsMut,
    sender: BankAccount,
    contract_address: BankAccount,
    execute_msg: Vec<u8>,
) -> Result<Response, ContractError> {
    let mut extension: SimpleWasmiVMExtension = bincode::deserialize(&VM_STATE.load(deps.storage)?)
        .map_err(|_| ContractError::Unexpected)?;
    let mut vm = create_simple_vm(
        sender,
        contract_address,
        Vec::new(),
        &extension
            .codes
            .get(
                &extension
                    .contracts
                    .get(&contract_address)
                    .ok_or(ContractError::ContractNotFound)?
                    .code_id,
            )
            .cloned()
            .unwrap(),
        &mut extension,
    );
    let res = execute_entrypoint::<ExecuteInput, _>(&mut vm, &execute_msg);
    VM_STATE.save(
        deps.storage,
        &bincode::serialize(&extension).map_err(|_| ContractError::Unexpected)?,
    )?;
    res
}

fn instantiate_contract(
    deps: DepsMut,
    sender: BankAccount,
    code_id: u64,
    instantiate_msg: Vec<u8>,
) -> Result<Response, ContractError> {
    let mut extension: SimpleWasmiVMExtension =
        bincode::deserialize(&VM_STATE.load(deps.storage)?).unwrap();
    let contract_address = extension
        .contracts
        .iter()
        .find(|(_, v)| v.code_id == code_id)
        .map(|(k, _)| k)
        .ok_or(ContractError::CodeNotFound)?;
    let mut vm = create_simple_vm(
        sender,
        *contract_address,
        Vec::new(),
        &extension.codes.get(&code_id).cloned().unwrap(),
        &mut extension,
    );
    let res = execute_entrypoint::<InstantiateInput, _>(&mut vm, &instantiate_msg);
    VM_STATE.save(
        deps.storage,
        &bincode::serialize(&extension).map_err(|_| ContractError::Unexpected)?,
    )?;
    res
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Query {
            sender,
            contract_address,
            request,
        } => {
            let mut extension: SimpleWasmiVMExtension =
                bincode::deserialize(&VM_STATE.load(deps.storage)?).map_err(|_| {
                    StdError::parse_err("VM_STATE", "bincode deserialization error")
                })?;
            let contract_address = BankAccount(contract_address);
            let mut vm = create_simple_vm(
                BankAccount(sender),
                contract_address,
                Vec::new(),
                &extension
                    .codes
                    .get(
                        &extension
                            .contracts
                            .get(&contract_address)
                            .ok_or_else(|| StdError::not_found("contract not found"))?
                            .code_id,
                    )
                    .cloned()
                    .unwrap(),
                &mut extension,
            );
            cosmwasm_system_query::<_>(&mut vm, request)
                .map_err(|_| StdError::generic_err(""))?
                .into_result()
                .map_err(|_| StdError::generic_err(""))?
                .into_result()
                .map_err(StdError::generic_err)
                .map(|data| Binary(data.0))
        }
    }
}
