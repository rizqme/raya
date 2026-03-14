//! Heap serialization for snapshots.

use std::io::{Read, Write};

use crate::vm::snapshot::format::byteswap;
use crate::vm::snapshot::value::SerializedValue;

/// Stable heap-object ID for snapshot serialization.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct ObjectId(u64);

impl ObjectId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SerializedDynEntry {
    pub key: String,
    pub value: SerializedValue,
}

impl SerializedDynEntry {
    fn encode(&self, writer: &mut impl Write) -> std::io::Result<()> {
        let key = self.key.as_bytes();
        writer.write_all(&(key.len() as u64).to_le_bytes())?;
        writer.write_all(key)?;
        self.value.encode(writer)
    }

    fn decode(reader: &mut impl Read, needs_byte_swap: bool) -> std::io::Result<Self> {
        let mut buf = [0u8; 8];
        reader.read_exact(&mut buf)?;
        let len = byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap) as usize;
        let mut key_bytes = vec![0u8; len];
        reader.read_exact(&mut key_bytes)?;
        let key = String::from_utf8(key_bytes).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid utf-8 in snapshot dyn key: {err}"),
            )
        })?;
        let value = SerializedValue::decode(reader, needs_byte_swap)?;
        Ok(Self { key, value })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SerializedHeapEntry {
    Object {
        object_id: ObjectId,
        layout_id: u32,
        nominal_type_id: Option<u32>,
        flags: u32,
        fields: Vec<SerializedValue>,
        dyn_entries: Vec<SerializedDynEntry>,
    },
    Array {
        object_id: ObjectId,
        type_id: usize,
        elements: Vec<SerializedValue>,
    },
    String {
        object_id: ObjectId,
        data: String,
    },
    Closure {
        object_id: ObjectId,
        func_id: usize,
        captures: Vec<SerializedValue>,
        module_checksum: Option<[u8; 32]>,
    },
    BoundMethod {
        object_id: ObjectId,
        receiver: SerializedValue,
        func_id: usize,
        module_checksum: Option<[u8; 32]>,
    },
    BoundNativeMethod {
        object_id: ObjectId,
        receiver: SerializedValue,
        native_id: u16,
    },
    RefCell {
        object_id: ObjectId,
        value: SerializedValue,
    },
    Channel {
        object_id: ObjectId,
        capacity: usize,
        queue: Vec<SerializedValue>,
        closed: bool,
    },
    Proxy {
        object_id: ObjectId,
        proxy_id: u64,
        target: SerializedValue,
        handler: SerializedValue,
    },
}

impl SerializedHeapEntry {
    pub fn object_id(&self) -> ObjectId {
        match self {
            Self::Object { object_id, .. }
            | Self::Array { object_id, .. }
            | Self::String { object_id, .. }
            | Self::Closure { object_id, .. }
            | Self::BoundMethod { object_id, .. }
            | Self::BoundNativeMethod { object_id, .. }
            | Self::RefCell { object_id, .. }
            | Self::Channel { object_id, .. }
            | Self::Proxy { object_id, .. } => *object_id,
        }
    }

    fn encode_module_checksum(
        checksum: &Option<[u8; 32]>,
        writer: &mut impl Write,
    ) -> std::io::Result<()> {
        match checksum {
            Some(checksum) => {
                writer.write_all(&[1])?;
                writer.write_all(checksum)
            }
            None => writer.write_all(&[0]),
        }
    }

    fn decode_module_checksum(
        reader: &mut impl Read,
        needs_byte_swap: bool,
    ) -> std::io::Result<Option<[u8; 32]>> {
        let mut tag = [0u8; 1];
        reader.read_exact(&mut tag)?;
        if tag[0] == 0 {
            return Ok(None);
        }
        let mut checksum = [0u8; 32];
        reader.read_exact(&mut checksum)?;
        let _ = needs_byte_swap;
        Ok(Some(checksum))
    }

    fn encode_values(values: &[SerializedValue], writer: &mut impl Write) -> std::io::Result<()> {
        writer.write_all(&(values.len() as u64).to_le_bytes())?;
        for value in values {
            value.encode(writer)?;
        }
        Ok(())
    }

    fn decode_values(
        reader: &mut impl Read,
        needs_byte_swap: bool,
    ) -> std::io::Result<Vec<SerializedValue>> {
        let mut buf = [0u8; 8];
        reader.read_exact(&mut buf)?;
        let len = byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap) as usize;
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            values.push(SerializedValue::decode(reader, needs_byte_swap)?);
        }
        Ok(values)
    }

    fn encode_dyn_entries(
        entries: &[SerializedDynEntry],
        writer: &mut impl Write,
    ) -> std::io::Result<()> {
        writer.write_all(&(entries.len() as u64).to_le_bytes())?;
        for entry in entries {
            entry.encode(writer)?;
        }
        Ok(())
    }

    fn decode_dyn_entries(
        reader: &mut impl Read,
        needs_byte_swap: bool,
    ) -> std::io::Result<Vec<SerializedDynEntry>> {
        let mut buf = [0u8; 8];
        reader.read_exact(&mut buf)?;
        let len = byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap) as usize;
        let mut entries = Vec::with_capacity(len);
        for _ in 0..len {
            entries.push(SerializedDynEntry::decode(reader, needs_byte_swap)?);
        }
        Ok(entries)
    }

    fn encode(&self, writer: &mut impl Write) -> std::io::Result<()> {
        match self {
            Self::Object {
                object_id,
                layout_id,
                nominal_type_id,
                flags,
                fields,
                dyn_entries,
            } => {
                writer.write_all(&[1])?;
                writer.write_all(&object_id.as_u64().to_le_bytes())?;
                writer.write_all(&layout_id.to_le_bytes())?;
                match nominal_type_id {
                    Some(id) => {
                        writer.write_all(&[1])?;
                        writer.write_all(&id.to_le_bytes())?;
                    }
                    None => writer.write_all(&[0])?,
                }
                writer.write_all(&flags.to_le_bytes())?;
                Self::encode_values(fields, writer)?;
                Self::encode_dyn_entries(dyn_entries, writer)?;
            }
            Self::Array {
                object_id,
                type_id,
                elements,
            } => {
                writer.write_all(&[2])?;
                writer.write_all(&object_id.as_u64().to_le_bytes())?;
                writer.write_all(&(*type_id as u64).to_le_bytes())?;
                Self::encode_values(elements, writer)?;
            }
            Self::String { object_id, data } => {
                writer.write_all(&[3])?;
                writer.write_all(&object_id.as_u64().to_le_bytes())?;
                let bytes = data.as_bytes();
                writer.write_all(&(bytes.len() as u64).to_le_bytes())?;
                writer.write_all(bytes)?;
            }
            Self::Closure {
                object_id,
                func_id,
                captures,
                module_checksum,
            } => {
                writer.write_all(&[4])?;
                writer.write_all(&object_id.as_u64().to_le_bytes())?;
                writer.write_all(&(*func_id as u64).to_le_bytes())?;
                Self::encode_values(captures, writer)?;
                Self::encode_module_checksum(module_checksum, writer)?;
            }
            Self::BoundMethod {
                object_id,
                receiver,
                func_id,
                module_checksum,
            } => {
                writer.write_all(&[5])?;
                writer.write_all(&object_id.as_u64().to_le_bytes())?;
                receiver.encode(writer)?;
                writer.write_all(&(*func_id as u64).to_le_bytes())?;
                Self::encode_module_checksum(module_checksum, writer)?;
            }
            Self::BoundNativeMethod {
                object_id,
                receiver,
                native_id,
            } => {
                writer.write_all(&[6])?;
                writer.write_all(&object_id.as_u64().to_le_bytes())?;
                receiver.encode(writer)?;
                writer.write_all(&native_id.to_le_bytes())?;
            }
            Self::RefCell { object_id, value } => {
                writer.write_all(&[7])?;
                writer.write_all(&object_id.as_u64().to_le_bytes())?;
                value.encode(writer)?;
            }
            Self::Channel {
                object_id,
                capacity,
                queue,
                closed,
            } => {
                writer.write_all(&[8])?;
                writer.write_all(&object_id.as_u64().to_le_bytes())?;
                writer.write_all(&(*capacity as u64).to_le_bytes())?;
                writer.write_all(&[*closed as u8])?;
                Self::encode_values(queue, writer)?;
            }
            Self::Proxy {
                object_id,
                proxy_id,
                target,
                handler,
            } => {
                writer.write_all(&[9])?;
                writer.write_all(&object_id.as_u64().to_le_bytes())?;
                writer.write_all(&proxy_id.to_le_bytes())?;
                target.encode(writer)?;
                handler.encode(writer)?;
            }
        }
        Ok(())
    }

    fn decode(reader: &mut impl Read, needs_byte_swap: bool) -> std::io::Result<Self> {
        let mut tag = [0u8; 1];
        let mut u64_buf = [0u8; 8];
        let mut u32_buf = [0u8; 4];
        let mut u16_buf = [0u8; 2];
        reader.read_exact(&mut tag)?;
        Ok(match tag[0] {
            1 => {
                reader.read_exact(&mut u64_buf)?;
                let object_id = ObjectId::new(byteswap::swap_u64(
                    u64::from_le_bytes(u64_buf),
                    needs_byte_swap,
                ));
                reader.read_exact(&mut u32_buf)?;
                let layout_id = byteswap::swap_u32(u32::from_le_bytes(u32_buf), needs_byte_swap);
                let mut nominal_tag = [0u8; 1];
                reader.read_exact(&mut nominal_tag)?;
                let nominal_type_id = if nominal_tag[0] == 1 {
                    reader.read_exact(&mut u32_buf)?;
                    Some(byteswap::swap_u32(
                        u32::from_le_bytes(u32_buf),
                        needs_byte_swap,
                    ))
                } else {
                    None
                };
                reader.read_exact(&mut u32_buf)?;
                let flags = byteswap::swap_u32(u32::from_le_bytes(u32_buf), needs_byte_swap);
                let fields = Self::decode_values(reader, needs_byte_swap)?;
                let dyn_entries = Self::decode_dyn_entries(reader, needs_byte_swap)?;
                Self::Object {
                    object_id,
                    layout_id,
                    nominal_type_id,
                    flags,
                    fields,
                    dyn_entries,
                }
            }
            2 => {
                reader.read_exact(&mut u64_buf)?;
                let object_id = ObjectId::new(byteswap::swap_u64(
                    u64::from_le_bytes(u64_buf),
                    needs_byte_swap,
                ));
                reader.read_exact(&mut u64_buf)?;
                let type_id =
                    byteswap::swap_u64(u64::from_le_bytes(u64_buf), needs_byte_swap) as usize;
                let elements = Self::decode_values(reader, needs_byte_swap)?;
                Self::Array {
                    object_id,
                    type_id,
                    elements,
                }
            }
            3 => {
                reader.read_exact(&mut u64_buf)?;
                let object_id = ObjectId::new(byteswap::swap_u64(
                    u64::from_le_bytes(u64_buf),
                    needs_byte_swap,
                ));
                reader.read_exact(&mut u64_buf)?;
                let len = byteswap::swap_u64(u64::from_le_bytes(u64_buf), needs_byte_swap) as usize;
                let mut bytes = vec![0u8; len];
                reader.read_exact(&mut bytes)?;
                let data = String::from_utf8(bytes).map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("invalid utf-8 in snapshot string: {err}"),
                    )
                })?;
                Self::String { object_id, data }
            }
            4 => {
                reader.read_exact(&mut u64_buf)?;
                let object_id = ObjectId::new(byteswap::swap_u64(
                    u64::from_le_bytes(u64_buf),
                    needs_byte_swap,
                ));
                reader.read_exact(&mut u64_buf)?;
                let func_id =
                    byteswap::swap_u64(u64::from_le_bytes(u64_buf), needs_byte_swap) as usize;
                let captures = Self::decode_values(reader, needs_byte_swap)?;
                let module_checksum = Self::decode_module_checksum(reader, needs_byte_swap)?;
                Self::Closure {
                    object_id,
                    func_id,
                    captures,
                    module_checksum,
                }
            }
            5 => {
                reader.read_exact(&mut u64_buf)?;
                let object_id = ObjectId::new(byteswap::swap_u64(
                    u64::from_le_bytes(u64_buf),
                    needs_byte_swap,
                ));
                let receiver = SerializedValue::decode(reader, needs_byte_swap)?;
                reader.read_exact(&mut u64_buf)?;
                let func_id =
                    byteswap::swap_u64(u64::from_le_bytes(u64_buf), needs_byte_swap) as usize;
                let module_checksum = Self::decode_module_checksum(reader, needs_byte_swap)?;
                Self::BoundMethod {
                    object_id,
                    receiver,
                    func_id,
                    module_checksum,
                }
            }
            6 => {
                reader.read_exact(&mut u64_buf)?;
                let object_id = ObjectId::new(byteswap::swap_u64(
                    u64::from_le_bytes(u64_buf),
                    needs_byte_swap,
                ));
                let receiver = SerializedValue::decode(reader, needs_byte_swap)?;
                reader.read_exact(&mut u16_buf)?;
                let native_id = byteswap::swap_u16(u16::from_le_bytes(u16_buf), needs_byte_swap);
                Self::BoundNativeMethod {
                    object_id,
                    receiver,
                    native_id,
                }
            }
            7 => {
                reader.read_exact(&mut u64_buf)?;
                let object_id = ObjectId::new(byteswap::swap_u64(
                    u64::from_le_bytes(u64_buf),
                    needs_byte_swap,
                ));
                let value = SerializedValue::decode(reader, needs_byte_swap)?;
                Self::RefCell { object_id, value }
            }
            8 => {
                reader.read_exact(&mut u64_buf)?;
                let object_id = ObjectId::new(byteswap::swap_u64(
                    u64::from_le_bytes(u64_buf),
                    needs_byte_swap,
                ));
                reader.read_exact(&mut u64_buf)?;
                let capacity =
                    byteswap::swap_u64(u64::from_le_bytes(u64_buf), needs_byte_swap) as usize;
                let mut closed = [0u8; 1];
                reader.read_exact(&mut closed)?;
                let queue = Self::decode_values(reader, needs_byte_swap)?;
                Self::Channel {
                    object_id,
                    capacity,
                    queue,
                    closed: closed[0] != 0,
                }
            }
            9 => {
                reader.read_exact(&mut u64_buf)?;
                let object_id = ObjectId::new(byteswap::swap_u64(
                    u64::from_le_bytes(u64_buf),
                    needs_byte_swap,
                ));
                reader.read_exact(&mut u64_buf)?;
                let proxy_id = byteswap::swap_u64(u64::from_le_bytes(u64_buf), needs_byte_swap);
                let target = SerializedValue::decode(reader, needs_byte_swap)?;
                let handler = SerializedValue::decode(reader, needs_byte_swap)?;
                Self::Proxy {
                    object_id,
                    proxy_id,
                    target,
                    handler,
                }
            }
            other => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("invalid heap entry tag {other}"),
                ));
            }
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HeapSnapshot {
    entries: Vec<SerializedHeapEntry>,
}

impl HeapSnapshot {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn empty() -> Self {
        Self::new()
    }

    pub fn add_entry(&mut self, entry: SerializedHeapEntry) {
        self.entries.push(entry);
    }

    pub fn entries(&self) -> &[SerializedHeapEntry] {
        &self.entries
    }

    pub fn encode(&self, writer: &mut impl Write) -> std::io::Result<()> {
        writer.write_all(&(self.entries.len() as u64).to_le_bytes())?;
        for entry in &self.entries {
            entry.encode(writer)?;
        }
        Ok(())
    }

    pub fn decode(reader: &mut impl Read, needs_byte_swap: bool) -> std::io::Result<Self> {
        let mut buf = [0u8; 8];
        reader.read_exact(&mut buf)?;
        let count = byteswap::swap_u64(u64::from_le_bytes(buf), needs_byte_swap) as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            entries.push(SerializedHeapEntry::decode(reader, needs_byte_swap)?);
        }
        Ok(Self { entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_object_entry() {
        let mut snapshot = HeapSnapshot::new();
        snapshot.add_entry(SerializedHeapEntry::Object {
            object_id: ObjectId::new(7),
            layout_id: 42,
            nominal_type_id: Some(3),
            flags: 1,
            fields: vec![SerializedValue::I32(9)],
            dyn_entries: vec![SerializedDynEntry {
                key: "name".to_string(),
                value: SerializedValue::Bool(true),
            }],
        });

        let mut buf = Vec::new();
        snapshot.encode(&mut buf).unwrap();
        let decoded = HeapSnapshot::decode(&mut &buf[..], false).unwrap();
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn round_trip_string_entry() {
        let mut snapshot = HeapSnapshot::new();
        snapshot.add_entry(SerializedHeapEntry::String {
            object_id: ObjectId::new(9),
            data: "raya".to_string(),
        });
        let mut buf = Vec::new();
        snapshot.encode(&mut buf).unwrap();
        let decoded = HeapSnapshot::decode(&mut &buf[..], false).unwrap();
        assert_eq!(decoded, snapshot);
    }
}
