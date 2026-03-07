//! Snapshot-safe value serialization.

use std::collections::HashMap;
use std::io::{Read, Write};

use crate::vm::snapshot::format::byteswap;
use crate::vm::snapshot::heap::ObjectId;
use crate::vm::value::Value;

/// Snapshot-safe representation of a VM value.
#[derive(Debug, Clone, PartialEq)]
pub enum SerializedValue {
    Null,
    Bool(bool),
    I32(i32),
    F64(u64),
    U32(u32),
    F32(u32),
    I64(i64),
    U64(u64),
    HeapRef(ObjectId),
}

impl SerializedValue {
    pub fn null() -> Self {
        Self::Null
    }

    pub fn bool(value: bool) -> Self {
        Self::Bool(value)
    }

    pub fn i32(value: i32) -> Self {
        Self::I32(value)
    }

    pub fn f64(value: f64) -> Self {
        Self::F64(value.to_bits())
    }

    pub fn u32(value: u32) -> Self {
        Self::U32(value)
    }

    pub fn f32(value: f32) -> Self {
        Self::F32(value.to_bits())
    }

    pub fn i64(value: i64) -> Self {
        Self::I64(value)
    }

    pub fn u64(value: u64) -> Self {
        Self::U64(value)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::I32(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::F64(bits) => Some(f64::from_bits(*bits)),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Self::U32(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Self::F32(bits) => Some(f32::from_bits(*bits)),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::I64(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::U64(value) => Some(*value),
            _ => None,
        }
    }

    pub fn encode(&self, writer: &mut impl Write) -> std::io::Result<()> {
        match self {
            Self::Null => writer.write_all(&[0]),
            Self::Bool(value) => {
                writer.write_all(&[1])?;
                writer.write_all(&[*value as u8])
            }
            Self::I32(value) => {
                writer.write_all(&[2])?;
                writer.write_all(&value.to_le_bytes())
            }
            Self::F64(bits) => {
                writer.write_all(&[3])?;
                writer.write_all(&bits.to_le_bytes())
            }
            Self::U32(value) => {
                writer.write_all(&[4])?;
                writer.write_all(&value.to_le_bytes())
            }
            Self::F32(bits) => {
                writer.write_all(&[5])?;
                writer.write_all(&bits.to_le_bytes())
            }
            Self::I64(value) => {
                writer.write_all(&[6])?;
                writer.write_all(&value.to_le_bytes())
            }
            Self::U64(value) => {
                writer.write_all(&[7])?;
                writer.write_all(&value.to_le_bytes())
            }
            Self::HeapRef(id) => {
                writer.write_all(&[8])?;
                writer.write_all(&id.as_u64().to_le_bytes())
            }
        }
    }

    pub fn decode(reader: &mut impl Read, needs_byte_swap: bool) -> std::io::Result<Self> {
        let mut tag = [0u8; 1];
        reader.read_exact(&mut tag)?;
        Ok(match tag[0] {
            0 => Self::Null,
            1 => {
                let mut buf = [0u8; 1];
                reader.read_exact(&mut buf)?;
                Self::Bool(buf[0] != 0)
            }
            2 => {
                let mut buf = [0u8; 4];
                reader.read_exact(&mut buf)?;
                Self::I32(byteswap::swap_u32(u32::from_le_bytes(buf), needs_byte_swap) as i32)
            }
            3 => {
                let mut buf = [0u8; 8];
                reader.read_exact(&mut buf)?;
                Self::F64(byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap))
            }
            4 => {
                let mut buf = [0u8; 4];
                reader.read_exact(&mut buf)?;
                Self::U32(byteswap::swap_u32(u32::from_le_bytes(buf), needs_byte_swap))
            }
            5 => {
                let mut buf = [0u8; 4];
                reader.read_exact(&mut buf)?;
                Self::F32(byteswap::swap_u32(u32::from_le_bytes(buf), needs_byte_swap))
            }
            6 => {
                let mut buf = [0u8; 8];
                reader.read_exact(&mut buf)?;
                Self::I64(byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap) as i64)
            }
            7 => {
                let mut buf = [0u8; 8];
                reader.read_exact(&mut buf)?;
                Self::U64(byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap))
            }
            8 => {
                let mut buf = [0u8; 8];
                reader.read_exact(&mut buf)?;
                Self::HeapRef(ObjectId::new(byteswap::swap_u64(
                    u64::from_le_bytes(buf),
                    needs_byte_swap,
                )))
            }
            other => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid serialized value tag {other}"),
                ));
            }
        })
    }

    pub fn from_live(
        value: Value,
        pointer_map: &HashMap<usize, ObjectId>,
    ) -> std::io::Result<Self> {
        if value.is_null() {
            Ok(Self::Null)
        } else if let Some(v) = value.as_bool() {
            Ok(Self::Bool(v))
        } else if let Some(v) = value.as_i32() {
            Ok(Self::I32(v))
        } else if let Some(v) = value.as_f64() {
            Ok(Self::F64(v.to_bits()))
        } else if let Some(v) = value.as_u32() {
            Ok(Self::U32(v))
        } else if let Some(v) = value.as_f32() {
            Ok(Self::F32(v.to_bits()))
        } else if let Some(v) = value.as_i64() {
            Ok(Self::I64(v))
        } else if let Some(v) = value.as_u64() {
            Ok(Self::U64(v))
        } else if value.is_ptr() {
            let ptr = unsafe { value.as_ptr::<u8>() }
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid heap pointer")
                })?
                .as_ptr() as usize;
            let object_id = pointer_map.get(&ptr).copied().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("heap pointer 0x{ptr:x} missing from snapshot pointer map"),
                )
            })?;
            Ok(Self::HeapRef(object_id))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unsupported live value tag {}", value.tag()),
            ))
        }
    }

    pub fn to_live(&self, object_map: &HashMap<ObjectId, Value>) -> std::io::Result<Value> {
        Ok(match self {
            Self::Null => Value::null(),
            Self::Bool(value) => Value::bool(*value),
            Self::I32(value) => Value::i32(*value),
            Self::F64(bits) => Value::f64(f64::from_bits(*bits)),
            Self::U32(value) => Value::u32(*value),
            Self::F32(bits) => Value::f32(f32::from_bits(*bits)),
            Self::I64(value) => Value::i64(*value),
            Self::U64(value) => Value::u64(*value),
            Self::HeapRef(id) => *object_map.get(id).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("snapshot heap object {} missing during restore", id.as_u64()),
                )
            })?,
        })
    }
}

impl From<Value> for SerializedValue {
    fn from(value: Value) -> Self {
        if value.is_null() {
            Self::Null
        } else if let Some(v) = value.as_bool() {
            Self::Bool(v)
        } else if let Some(v) = value.as_i32() {
            Self::I32(v)
        } else if let Some(v) = value.as_f64() {
            Self::F64(v.to_bits())
        } else if let Some(v) = value.as_u32() {
            Self::U32(v)
        } else if let Some(v) = value.as_f32() {
            Self::F32(v.to_bits())
        } else if let Some(v) = value.as_i64() {
            Self::I64(v)
        } else if let Some(v) = value.as_u64() {
            Self::U64(v)
        } else {
            panic!("cannot convert heap pointer Value to SerializedValue without snapshot object map");
        }
    }
}

impl PartialEq<Value> for SerializedValue {
    fn eq(&self, other: &Value) -> bool {
        *self == SerializedValue::from(*other)
    }
}

impl PartialEq<SerializedValue> for Value {
    fn eq(&self, other: &SerializedValue) -> bool {
        SerializedValue::from(*self) == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_primitives() {
        let values = [
            SerializedValue::Null,
            SerializedValue::Bool(true),
            SerializedValue::I32(42),
            SerializedValue::F64(3.5f64.to_bits()),
            SerializedValue::U32(7),
            SerializedValue::F32(1.25f32.to_bits()),
            SerializedValue::I64(-9),
            SerializedValue::U64(11),
            SerializedValue::HeapRef(ObjectId::new(99)),
        ];
        for value in values {
            let mut buf = Vec::new();
            value.encode(&mut buf).unwrap();
            let decoded = SerializedValue::decode(&mut &buf[..], false).unwrap();
            assert_eq!(decoded, value);
        }
    }
}
