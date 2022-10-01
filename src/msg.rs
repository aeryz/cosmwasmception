use cosmwasm_minimal_std::QueryRequest;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct ContractMeta {
    pub code: Vec<u8>,
    pub address: u128,
    pub code_id: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    pub contracts: Vec<ContractMeta>,
    pub next_account_id: u128,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    Instantiate {
        sender: u128,
        code_id: u64,
        instantiate_msg: Vec<u8>,
    },
    Execute {
        sender: u128,
        contract_address: u128,
        execute_msg: Vec<u8>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    Query {
        sender: u128,
        contract_address: u128,
        request: QueryRequest,
    },
}
