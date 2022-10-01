use cw_storage_plus::Item;

pub const VM_STATE: Item<Vec<u8>> = Item::new("vm_state");
