use crate::storage::{
	atlas::{EntryAtlas, FieldAtlas},
	entry::{MetaEntry, PasswordEntry},
	secret::SecretMemory,
};
use std::{
	collections::BTreeMap,
	mem,
	sync::{Arc, RwLock},
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug)]
#[repr(u8)]
pub enum EntryTag {
	Meta = 0u8,
	Password = 1u8,
}

#[derive(Clone)]
pub struct Database {
	idb: Arc<RwLock<InteriorDatabase>>,
	master_key: Arc<RwLock<SecretMemory>>,
}

impl Database {
	#[cfg(test)]
	#[allow(dead_code)] // The library target does not include GUI tests.
	pub(crate) fn test_database() -> Self {
		let key = SecretMemory::new_pages(1).unwrap();
		key.write(0, &[42; 32]).unwrap();
		Self::new(key)
	}

	pub fn new(master_key: SecretMemory) -> Self {
		let idb = InteriorDatabase::default();
		Self {
			idb: Arc::new(RwLock::new(idb)),
			master_key: Arc::new(RwLock::new(master_key)),
		}
	}
	pub fn old(master_key: SecretMemory, idb: InteriorDatabase) -> Self {
		Self {
			idb: Arc::new(RwLock::new(idb)),
			master_key: Arc::new(RwLock::new(master_key)),
		}
	}
	pub fn set_password_entry(&self, entry: PasswordEntry) {
		self.idb.write().unwrap().set_password_entry(entry);
	}
	pub fn get_password_entry_by_name(&self, name: &str) -> Option<PasswordEntry> {
		self.idb.read().unwrap().get_password_entry_by_name(name)
	}
	pub fn get_password_entries_by_tag(&self, tag: &str) -> Option<Vec<PasswordEntry>> {
		self.idb.read().unwrap().get_password_entries_by_tag(tag)
	}
	pub fn query(&self, query: &str) -> Option<PasswordEntry> {
		if let Some(entry) = self.get_password_entry_by_name(query) {
			return Some(entry);
		}
		let tag_query = match query.split_once(".") {
			Some((tag, tag_index_str)) => match tag_index_str.parse::<usize>().ok() {
				Some(tag_index) => (tag, tag_index.saturating_sub(1)),
				None => (query, 0),
			},
			_ => (query, 0),
		};
		self.get_password_entries_by_tag(tag_query.0)
			.and_then(|entries| entries.get(tag_query.1).cloned())
	}
	pub fn previous(&self, name: &str) -> Option<PasswordEntry> {
		self.idb.read().unwrap().previous(name)
	}
	pub fn next(&self, name: &str) -> Option<PasswordEntry> {
		self.idb.read().unwrap().next(name)
	}
	pub fn set_meta_entry(&self, entry: MetaEntry) {
		self.idb.write().unwrap().set_meta_entry(entry);
	}
	pub fn get_meta_entry(&self, name: &str) -> Option<MetaEntry> {
		self.idb.read().unwrap().get_meta_entry(name)
	}
	pub fn serialize(&self) -> Zeroizing<Vec<u8>> {
		self.idb.read().unwrap().serialize()
	}
	pub fn meta_only(&self) -> Self {
		let mut meta = EntryAtlas::default();
		let mut idx = 1u32;
		let idb = self.idb.read().unwrap();
		for id in idb.entries.entries.keys() {
			let (entry_tag, entry_data) = idb.entries.get(*id).unwrap();
			let entry_tag = unsafe { mem::transmute::<u8, EntryTag>(entry_tag) };
			if let EntryTag::Meta = entry_tag {
				meta.set(idx, entry_tag as u8, &entry_data);
				idx += 1;
			}
		}
		Self {
			idb: Arc::new(RwLock::new(InteriorDatabase::from_entry_atlas(meta))),
			master_key: Arc::clone(&self.master_key),
		}
	}
	pub fn clone_master_key(&self) -> Arc<RwLock<SecretMemory>> {
		self.master_key.clone()
	}
	pub fn zeroize(&self) {
		let _ = self.master_key.write().unwrap().zeroize();
		self.idb.write().unwrap().zeroize();
	}
}

#[derive(Default)]
pub struct InteriorDatabase {
	next_id: u32,
	entries: EntryAtlas,
	index_by_name: BTreeMap<String, u32>,
	index_by_tag: BTreeMap<String, Vec<u32>>,
}

impl InteriorDatabase {
	fn set_password_entry(&mut self, entry: PasswordEntry) {
		let name = entry.get_name();
		let index_by_name_key = format!("password\x00{}", name);
		let is_new = !self.index_by_name.contains_key(&index_by_name_key);
		let id: u32;
		if is_new {
			id = self.next_id;
			self.next_id += 1;
			self.index_by_name.insert(index_by_name_key, id);
			for tag in entry.get_tags().split(",").map(|tg| tg.trim()) {
				if tag.is_empty() {
					continue;
				}
				let index_by_tag_key = format!("tag\x00{}", tag);
				self.index_by_tag
					.entry(index_by_tag_key)
					.or_default()
					.push(id);
			}
		} else {
			id = *self.index_by_name.get(&index_by_name_key).unwrap();
		}
		self.entries
			.set(id, EntryTag::Password as u8, &entry.serialize());
	}
	fn get_password_entry_by_name(&self, name: &str) -> Option<PasswordEntry> {
		let index_by_name_key = format!("password\x00{}", name);
		if let Some(id) = self.index_by_name.get(&index_by_name_key)
			&& let Some(entry) = self.entries.get(*id)
		{
			return Some(PasswordEntry::from(FieldAtlas::deserialize(&entry.1)));
		}
		None
	}
	fn get_password_entries_by_tag(&self, tag: &str) -> Option<Vec<PasswordEntry>> {
		let index_by_tag_key = format!("tag\x00{}", tag);
		if let Some(ids) = self.index_by_tag.get(&index_by_tag_key) {
			let mut entries = vec![];
			for id in ids {
				let entry = self.entries.get(*id).unwrap();
				entries.push(PasswordEntry::from(FieldAtlas::deserialize(&entry.1)));
			}
			return Some(entries);
		}
		None
	}
	fn previous(&self, name: &str) -> Option<PasswordEntry> {
		let lower_bound = "password\x00".to_string();
		let index_by_name_key = format!("{lower_bound}{name}");
		let previous_id = self
			.index_by_name
			.range(lower_bound..index_by_name_key)
			.next_back()
			.map(|kv| kv.1)?;
		let (entry_tag, entry_data) = self.entries.get(*previous_id)?;
		let entry_tag = unsafe { mem::transmute::<u8, EntryTag>(entry_tag) };
		match entry_tag {
			EntryTag::Password => Some(PasswordEntry::from(FieldAtlas::deserialize(&entry_data))),
			_ => None,
		}
	}
	fn next(&self, name: &str) -> Option<PasswordEntry> {
		let lower_bound = "password\x00".to_string();
		let upper_bound = "password\x01".to_string();
		let index_by_name_key = format!("{lower_bound}{name}");
		let offset = if self.index_by_name.contains_key(&index_by_name_key) {
			1
		} else {
			0
		};
		let next_id = self
			.index_by_name
			.range(index_by_name_key..upper_bound)
			.nth(offset)
			.map(|kv| kv.1)?;
		let (entry_tag, entry_data) = self.entries.get(*next_id)?;
		let entry_tag = unsafe { mem::transmute::<u8, EntryTag>(entry_tag) };
		match entry_tag {
			EntryTag::Password => Some(PasswordEntry::from(FieldAtlas::deserialize(&entry_data))),
			_ => None,
		}
	}
	fn set_meta_entry(&mut self, entry: MetaEntry) {
		let name = entry.get_name();
		let index_by_name_key = format!("meta\x00{}", name);
		let is_new = !self.index_by_name.contains_key(&index_by_name_key);
		let id: u32;
		if is_new {
			id = self.next_id;
			self.next_id += 1;
			self.index_by_name.insert(index_by_name_key, id);
		} else {
			id = *self.index_by_name.get(&index_by_name_key).unwrap();
		}
		self.entries
			.set(id, EntryTag::Meta as u8, &entry.serialize());
	}
	pub fn get_meta_entry(&self, name: &str) -> Option<MetaEntry> {
		let index_by_name_key = format!("meta\x00{}", name);
		if let Some(id) = self.index_by_name.get(&index_by_name_key)
			&& let Some(entry) = self.entries.get(*id)
		{
			return Some(MetaEntry::from(FieldAtlas::deserialize(&entry.1)));
		}
		None
	}
	pub fn serialize(&self) -> Zeroizing<Vec<u8>> {
		self.entries.serialize()
	}
	pub fn deserialize(data: &[u8]) -> Self {
		let entry_atlas = EntryAtlas::deserialize(data);
		InteriorDatabase::from_entry_atlas(entry_atlas)
	}
	pub fn from_entry_atlas(entry_atlas: EntryAtlas) -> Self {
		let mut db = InteriorDatabase::default();
		for id in entry_atlas.entries.keys() {
			let (entry_tag, entry_data) = entry_atlas.get(*id).unwrap();
			let entry = FieldAtlas::deserialize(&entry_data);
			let entry_tag = unsafe { mem::transmute::<u8, EntryTag>(entry_tag) };
			match entry_tag {
				EntryTag::Meta => {
					db.set_meta_entry(MetaEntry::from(entry));
				}
				EntryTag::Password => {
					db.set_password_entry(PasswordEntry::from(entry));
				}
			}
		}
		db
	}
	pub fn zeroize(&mut self) {
		self.entries.clear();
		let index_by_name = mem::take(&mut self.index_by_name);
		for (mut key, _) in index_by_name {
			key.zeroize();
		}
		let index_by_tag = mem::take(&mut self.index_by_tag);
		for (mut key, _) in index_by_tag {
			key.zeroize();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn original_format_fixture() -> Vec<u8> {
		// The existing on-disk TLV representation, without the new memory layer.
		let mut fields = Vec::new();
		for (id, text) in [
			(1u8, "example"),
			(2, "alice"),
			(3, "password 🔑"),
			(4, "1234"),
			(5, "private note"),
			(7, "test"),
		] {
			fields.push(id);
			fields.extend_from_slice(&(text.len() as u32).to_le_bytes());
			fields.extend_from_slice(text.as_bytes());
		}
		let mut bytes = vec![1];
		bytes.extend_from_slice(&(fields.len() as u32).to_le_bytes());
		bytes.extend_from_slice(&fields);
		bytes
	}

	#[test]
	fn existing_format_roundtrips_and_edits_without_migration() {
		let fixture = original_format_fixture();
		let mut db = InteriorDatabase::deserialize(&fixture);
		assert_eq!(&*db.serialize(), &fixture);
		let mut entry = db.get_password_entry_by_name("example").unwrap();
		assert_eq!(entry.get_password().as_str(), "password 🔑");
		assert_eq!(entry.get_pin().as_str(), "1234");
		assert_eq!(entry.get_note().as_str(), "private note");
		assert_eq!(db.get_password_entries_by_tag("test").unwrap().len(), 1);
		entry.set_password("replacement");
		db.set_password_entry(entry);
		let reloaded = InteriorDatabase::deserialize(&db.serialize());
		assert_eq!(
			reloaded
				.get_password_entry_by_name("example")
				.unwrap()
				.get_password()
				.as_str(),
			"replacement"
		);
		assert_eq!(
			reloaded
				.get_password_entry_by_name("example")
				.unwrap()
				.get_note()
				.as_str(),
			"private note"
		);
		db.zeroize();
		assert!(db.entries.entries.is_empty());
		assert!(db.index_by_name.is_empty());
	}
}
