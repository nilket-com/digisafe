use super::{
	protected::{ProtectedBytes, fresh_key},
	secret::SecretMemory,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use zeroize::Zeroizing;

#[derive(Clone, Default)]
pub struct FieldAtlas {
	fields: BTreeMap<u8, Zeroizing<Vec<u8>>>,
}

impl FieldAtlas {
	pub fn deserialize(data: &[u8]) -> Self {
		let mut fields = BTreeMap::new();
		let mut cursor = 0;
		while cursor < data.len() {
			let field_type = data[cursor];
			cursor += 1;
			let len_bytes = &data[cursor..cursor + 4];
			let len = u32::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
			cursor += 4;
			let value = &data[cursor..cursor + len];
			fields.insert(field_type, value.to_vec().into());
			cursor += len;
		}
		FieldAtlas { fields }
	}
	pub fn serialize(&self) -> Zeroizing<Vec<u8>> {
		let mut buffer = Vec::with_capacity(self.fields.values().map(|v| 5 + v.len()).sum());
		for (&field_type, value) in &self.fields {
			buffer.push(field_type);
			let len = value.len() as u32;
			buffer.extend_from_slice(&len.to_le_bytes());
			buffer.extend_from_slice(value.as_ref());
		}
		Zeroizing::new(buffer)
	}
	pub fn get(&self, field_type: u8) -> Option<&[u8]> {
		self.fields.get(&field_type).map(|v| v.as_ref())
	}
	pub fn get_str(&self, field_type: u8) -> Option<&str> {
		self.get(field_type)
			.and_then(|bytes| std::str::from_utf8(bytes).ok())
	}
	pub fn take(&mut self, field_type: u8) -> Option<Zeroizing<Vec<u8>>> {
		self.fields.remove(&field_type)
	}

	pub fn set(&mut self, field_type: u8, value: Vec<u8>) {
		self.fields.insert(field_type, value.into());
	}
}

pub struct EntryAtlas {
	pub entries: BTreeMap<u32, (u8, ProtectedBytes)>,
	key: Arc<SecretMemory>,
}

impl Default for EntryAtlas {
	fn default() -> Self {
		Self {
			entries: BTreeMap::new(),
			key: fresh_key(),
		}
	}
}

impl EntryAtlas {
	fn context(id: u32, tag: u8) -> [u8; 5] {
		let mut context = [0; 5];
		context[..4].copy_from_slice(&id.to_le_bytes());
		context[4] = tag;
		context
	}

	pub fn deserialize(data: &[u8]) -> Self {
		let mut atlas = Self::default();
		let mut cursor = 0;
		let mut id = 0;
		while cursor < data.len() {
			let tag = data[cursor];
			cursor += 1;
			let len = u32::from_le_bytes(data[cursor..cursor + 4].try_into().unwrap()) as usize;
			cursor += 4;
			atlas.set(id, tag, &data[cursor..cursor + len]);
			cursor += len;
			id += 1;
		}
		atlas
	}

	pub fn serialize(&self) -> Zeroizing<Vec<u8>> {
		// Size from ciphertext lengths to avoid reallocating plaintext buffers.
		let size = self
			.entries
			.values()
			.map(|(_, value)| 5 + value.plaintext_len())
			.sum();
		let mut buffer = Zeroizing::new(Vec::with_capacity(size));
		for id in self.entries.keys() {
			let (tag, value) = self.get(*id).unwrap();
			buffer.push(tag);
			buffer.extend_from_slice(&(value.len() as u32).to_le_bytes());
			buffer.extend_from_slice(&value);
		}
		buffer
	}

	pub fn set(&mut self, id: u32, tag: u8, value: &[u8]) {
		let sealed = ProtectedBytes::seal(&self.key, value, &Self::context(id, tag));
		self.entries.insert(id, (tag, sealed));
	}

	pub fn get(&self, id: u32) -> Option<(u8, Zeroizing<Vec<u8>>)> {
		let (tag, sealed) = self.entries.get(&id)?;
		Some((*tag, sealed.open(&Self::context(id, *tag))))
	}

	pub fn clear(&mut self) {
		self.entries.clear();
		self.key.zeroize().expect("Failed to erase memory key");
	}
}
