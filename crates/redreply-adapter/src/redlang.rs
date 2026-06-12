use crate::AdapterResult;

pub fn get_random() -> AdapterResult<u64> {
    let uuid = uuid::Uuid::new_v4();
    let bytes = uuid.as_u128().to_le_bytes();
    Ok(u64::from_le_bytes(bytes[0..8].try_into()?))
}

