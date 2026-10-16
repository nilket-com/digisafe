use super::{
	protected::{ProtectedBytes, fresh_key},
	secret::SecretMemory,
};
use crate::storage::atlas::FieldAtlas;
use std::time::SystemTime;
use std::{collections::BTreeMap, sync::Arc};
use zeroize::Zeroizing;

#[derive(Debug)]
#[repr(u8)]
pub enum MetaField {
	Name = 1u8,
	Value = 2u8,
}

#[derive(Default)]
pub struct MetaEntry {
	field_atlas: FieldAtlas,
}

impl From<FieldAtlas> for MetaEntry {
	fn from(field_atlas: FieldAtlas) -> Self {
		MetaEntry { field_atlas }
	}
}

impl MetaEntry {
	pub fn new(name: &str, value: &str) -> Self {
		let mut meta_entry = MetaEntry::default();
		meta_entry.set_name(name);
		meta_entry.set_value(value);
		meta_entry
	}
	pub fn set_name(&mut self, name: &str) {
		self.field_atlas
			.set(MetaField::Name as u8, name.as_bytes().to_vec());
	}
	pub fn set_value(&mut self, value: &str) {
		self.field_atlas
			.set(MetaField::Value as u8, value.as_bytes().to_vec());
	}
	pub fn get_name(&self) -> &str {
		self.field_atlas.get_str(MetaField::Name as u8).unwrap()
	}
	pub fn get_value(&self) -> &str {
		self.field_atlas.get_str(MetaField::Value as u8).unwrap()
	}
	pub fn serialize(&self) -> Zeroizing<Vec<u8>> {
		self.field_atlas.serialize()
	}
}

#[derive(Debug)]
#[repr(u8)]
pub enum PasswordField {
	Name = 1u8,
	Username = 2u8,
	Password = 3u8,
	Pin = 4u8,
	Note = 5u8,
	Url = 6u8,
	Tags = 7u8,
	CreatedTimestamp = 8u8,
	ModifiedTimestamp = 9u8,
}

#[derive(Clone)]
pub struct PasswordEntry {
	field_atlas: FieldAtlas,
	secrets: BTreeMap<u8, ProtectedBytes>,
	key: Arc<SecretMemory>,
}

impl From<FieldAtlas> for PasswordEntry {
	fn from(mut field_atlas: FieldAtlas) -> Self {
		let key = fresh_key();
		let mut secrets = BTreeMap::new();
		for field in [
			PasswordField::Password,
			PasswordField::Pin,
			PasswordField::Note,
		] {
			let id = field as u8;
			if let Some(value) = field_atlas.take(id) {
				secrets.insert(id, ProtectedBytes::seal(&key, &value, &[id]));
			}
		}
		Self {
			field_atlas,
			secrets,
			key,
		}
	}
}

impl Default for PasswordEntry {
	fn default() -> Self {
		Self::new()
	}
}

impl PasswordEntry {
	pub fn new() -> Self {
		let now = SystemTime::now()
			.duration_since(SystemTime::UNIX_EPOCH)
			.unwrap()
			.as_secs()
			.to_string()
			.as_bytes()
			.to_vec();
		let mut field_atlas = FieldAtlas::default();
		field_atlas.set(PasswordField::CreatedTimestamp as u8, now.clone());
		field_atlas.set(PasswordField::ModifiedTimestamp as u8, now);
		PasswordEntry::from(field_atlas)
	}
	pub fn set_name(&mut self, name: &str) {
		self.update_ts();
		self.field_atlas
			.set(PasswordField::Name as u8, name.as_bytes().to_vec());
	}
	pub fn set_username(&mut self, username: &str) {
		self.update_ts();
		self.field_atlas
			.set(PasswordField::Username as u8, username.as_bytes().to_vec());
	}
	pub fn set_password(&mut self, password: &str) {
		self.update_ts();
		let id = PasswordField::Password as u8;
		self.secrets.insert(
			id,
			ProtectedBytes::seal(&self.key, password.as_bytes(), &[id]),
		);
	}
	pub fn set_pin(&mut self, pin: &str) {
		self.update_ts();
		let id = PasswordField::Pin as u8;
		self.secrets
			.insert(id, ProtectedBytes::seal(&self.key, pin.as_bytes(), &[id]));
	}
	pub fn set_url(&mut self, url: &str) {
		self.update_ts();
		self.field_atlas
			.set(PasswordField::Url as u8, url.as_bytes().to_vec());
	}
	pub fn set_tags(&mut self, tags: &str) {
		self.update_ts();
		self.field_atlas
			.set(PasswordField::Tags as u8, tags.as_bytes().to_vec());
	}
	pub fn set_note(&mut self, note: &str) {
		self.update_ts();
		let id = PasswordField::Note as u8;
		self.secrets
			.insert(id, ProtectedBytes::seal(&self.key, note.as_bytes(), &[id]));
	}
	fn update_ts(&mut self) {
		let now = SystemTime::now()
			.duration_since(SystemTime::UNIX_EPOCH)
			.unwrap()
			.as_secs()
			.to_string()
			.as_bytes()
			.to_vec();
		self.field_atlas
			.set(PasswordField::ModifiedTimestamp as u8, now);
	}
	pub fn get_name(&self) -> &str {
		self.field_atlas
			.get_str(PasswordField::Name as u8)
			.unwrap_or_default()
	}
	pub fn get_username(&self) -> &str {
		self.field_atlas
			.get_str(PasswordField::Username as u8)
			.unwrap_or_default()
	}
	pub fn get_password(&self) -> Zeroizing<String> {
		self.open_secret(PasswordField::Password as u8)
	}
	pub fn get_pin(&self) -> Zeroizing<String> {
		self.open_secret(PasswordField::Pin as u8)
	}
	pub fn get_url(&self) -> &str {
		self.field_atlas
			.get_str(PasswordField::Url as u8)
			.unwrap_or_default()
	}
	pub fn get_tags(&self) -> &str {
		self.field_atlas
			.get_str(PasswordField::Tags as u8)
			.unwrap_or_default()
	}
	pub fn get_note(&self) -> Zeroizing<String> {
		self.open_secret(PasswordField::Note as u8)
	}
	pub fn get_timestamps(&self) -> String {
		use chrono::{DateTime, Local, TimeZone, Utc};
		let now = SystemTime::now()
			.duration_since(SystemTime::UNIX_EPOCH)
			.unwrap()
			.as_secs()
			.to_string();
		let mts = self
			.field_atlas
			.get_str(PasswordField::ModifiedTimestamp as u8)
			.unwrap_or(&now)
			.parse::<i64>()
			.unwrap();
		let cts = self
			.field_atlas
			.get_str(PasswordField::CreatedTimestamp as u8)
			.unwrap_or(&now)
			.parse::<i64>()
			.unwrap();
		let mdt_utc = Utc.timestamp_opt(mts, 0).unwrap();
		let cdt_utc = Utc.timestamp_opt(cts, 0).unwrap();
		let mdt_local = DateTime::<Local>::from(mdt_utc);
		let cdt_local = DateTime::<Local>::from(cdt_utc);
		let mdt = mdt_local.format("%Y-%m-%d %H:%M");
		let cdt = cdt_local.format("%Y-%m-%d %H:%M");
		format!("Modified: {mdt} | Created: {cdt} ")
	}
	fn open_secret(&self, id: u8) -> Zeroizing<String> {
		let Some(value) = self.secrets.get(&id) else {
			return Zeroizing::new(String::new());
		};
		let mut bytes = value.open(&[id]);
		// Validate before transferring ownership to the wiping String.
		std::str::from_utf8(&bytes).expect("Invalid secret text");
		Zeroizing::new(String::from_utf8(std::mem::take(&mut *bytes)).unwrap())
	}

	pub fn serialize(&self) -> Zeroizing<Vec<u8>> {
		let mut fields = self.field_atlas.clone();
		for (id, value) in &self.secrets {
			let mut plaintext = value.open(&[*id]);
			fields.set(*id, std::mem::take(&mut *plaintext));
		}
		fields.serialize()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn secret_fields_are_absent_from_plaintext_atlas() {
		let mut entry = PasswordEntry::new();
		entry.set_password("secret");
		entry.set_pin("4567");
		entry.set_note("sensitive note");
		for id in [3, 4, 5] {
			assert!(entry.field_atlas.get(id).is_none());
		}
		let cloned = entry.clone();
		drop(entry);
		assert_eq!(cloned.get_password().as_str(), "secret");
		let restored = PasswordEntry::from(FieldAtlas::deserialize(&cloned.serialize()));
		assert_eq!(restored.get_note().as_str(), "sensitive note");
		assert_eq!(restored.get_pin().as_str(), "4567");
	}
}
