use super::secret::SecretMemory;
use chacha20poly1305::{
	XChaCha20Poly1305,
	aead::{AeadInPlace, KeyInit},
};
use std::sync::Arc;
use zeroize::Zeroizing;

pub fn fresh_key() -> Arc<SecretMemory> {
	let mut bytes = Zeroizing::new([0u8; 32]);
	getrandom::fill(bytes.as_mut()).expect("Memory key generation failed");
	let key = SecretMemory::new_pages(1).expect("Secret memory unavailable");
	key.write(0, bytes.as_ref())
		.expect("Memory key storage failed");
	Arc::new(key)
}

/// Only ciphertext is cloned. The key remains in unmapped secret memory.
#[derive(Clone)]
pub struct ProtectedBytes {
	ciphertext: Vec<u8>,
	nonce: [u8; 24],
	key: Arc<SecretMemory>,
}

impl ProtectedBytes {
	pub fn plaintext_len(&self) -> usize {
		self.ciphertext.len() - 16
	}

	pub fn seal(key: &Arc<SecretMemory>, plaintext: &[u8], context: &[u8]) -> Self {
		let mut nonce = [0u8; 24];
		getrandom::fill(&mut nonce).expect("Memory nonce generation failed");
		// Reserve the authentication tag up front, so encryption cannot leave
		// a reallocated plaintext buffer behind.
		let mut buffer = Zeroizing::new(Vec::with_capacity(plaintext.len() + 16));
		buffer.extend_from_slice(plaintext);
		let mapped = key.read().expect("Memory key mapping failed");
		let cipher = XChaCha20Poly1305::new_from_slice(&mapped).expect("Invalid memory key");
		drop(mapped);
		cipher
			.encrypt_in_place((&nonce).into(), context, &mut *buffer)
			.expect("Memory encryption failed");
		Self {
			ciphertext: std::mem::take(&mut *buffer),
			nonce,
			key: Arc::clone(key),
		}
	}

	pub fn open(&self, context: &[u8]) -> Zeroizing<Vec<u8>> {
		let mut buffer = Zeroizing::new(self.ciphertext.clone());
		let mapped = self.key.read().expect("Memory key mapping failed");
		let cipher = XChaCha20Poly1305::new_from_slice(&mapped).expect("Invalid memory key");
		drop(mapped);
		cipher
			.decrypt_in_place((&self.nonce).into(), context, &mut *buffer)
			.expect("Memory authentication failed");
		buffer
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn ciphertext_clones_and_unique_nonces() {
		let key = fresh_key();
		let plaintext = b"distinctive-password-123456789";
		let first = ProtectedBytes::seal(&key, plaintext, b"password");
		let second = ProtectedBytes::seal(&key, plaintext, b"password");
		assert_ne!(first.nonce, second.nonce);
		assert_ne!(first.ciphertext, second.ciphertext);
		assert!(
			!first
				.ciphertext
				.windows(plaintext.len())
				.any(|w| w == plaintext)
		);
		let cloned = first.clone();
		drop(first);
		assert_eq!(&*cloned.open(b"password"), plaintext);
	}

	#[test]
	#[should_panic(expected = "Memory authentication failed")]
	fn altered_ciphertext_is_rejected() {
		let mut value = ProtectedBytes::seal(&fresh_key(), b"secret", b"password");
		value.ciphertext[0] ^= 1;
		value.open(b"password");
	}

	#[test]
	#[should_panic(expected = "Memory authentication failed")]
	fn wrong_field_is_rejected() {
		let value = ProtectedBytes::seal(&fresh_key(), b"secret", b"password");
		value.open(b"pin");
	}
}
