use libc::{
	_SC_PAGESIZE, MAP_FAILED, MAP_SHARED, O_CLOEXEC, PROT_READ, PROT_WRITE, SYS_memfd_secret,
	ftruncate, mmap, munmap, off_t, syscall, sysconf,
};
use std::{
	io,
	ops::{Deref, DerefMut},
	os::fd::{AsRawFd, FromRawFd, OwnedFd},
	ptr, slice,
	sync::{
		Mutex, MutexGuard,
		atomic::{AtomicUsize, Ordering},
	},
};
use zeroize::Zeroize;

/// A secret file is unmapped between operations. The mutex prevents aliases to
/// writable mappings, including when callers share this object through Arc.
pub struct SecretMemory {
	fd: OwnedFd,
	capacity: usize,
	length: AtomicUsize,
	mapping: Mutex<()>,
}

impl SecretMemory {
	pub fn new(capacity: usize) -> io::Result<Self> {
		if capacity == 0 || capacity > off_t::MAX as usize {
			return Err(io::Error::new(
				io::ErrorKind::InvalidInput,
				"Invalid secret capacity",
			));
		}
		let fd = unsafe { syscall(SYS_memfd_secret, O_CLOEXEC) as i32 };
		if fd < 0 {
			return Err(io::Error::last_os_error());
		}
		let fd = unsafe { OwnedFd::from_raw_fd(fd) };
		if unsafe { ftruncate(fd.as_raw_fd(), capacity as off_t) } < 0 {
			return Err(io::Error::last_os_error());
		}
		Ok(Self {
			fd,
			capacity,
			length: AtomicUsize::new(0),
			mapping: Mutex::new(()),
		})
	}

	pub fn new_pages(pages: usize) -> io::Result<Self> {
		let size = unsafe { sysconf(_SC_PAGESIZE) };
		if size <= 0 {
			return Err(io::Error::last_os_error());
		}
		let capacity = pages
			.checked_mul(size as usize)
			.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Capacity overflow"))?;
		Self::new(capacity)
	}

	fn map(&self, full: bool) -> io::Result<MappedGuard<'_>> {
		let lock = self.mapping.lock().unwrap_or_else(|e| e.into_inner());
		let ptr = unsafe {
			mmap(
				ptr::null_mut(),
				self.capacity,
				PROT_READ | PROT_WRITE,
				MAP_SHARED,
				self.fd.as_raw_fd(),
				0,
			)
		};
		if ptr == MAP_FAILED {
			return Err(io::Error::last_os_error());
		}
		Ok(MappedGuard {
			ptr: ptr.cast(),
			mapped_len: self.capacity,
			visible_len: if full {
				self.capacity
			} else {
				self.length.load(Ordering::Acquire)
			},
			_lock: lock,
		})
	}

	pub fn write(&self, offset: usize, data: &[u8]) -> io::Result<()> {
		let end = offset
			.checked_add(data.len())
			.filter(|end| *end <= self.capacity)
			.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Buffer overflow"))?;
		let mut guard = self.map(true)?;
		guard[offset..end].copy_from_slice(data);
		self.length.fetch_max(end, Ordering::Release);
		Ok(())
	}

	pub fn read(&self) -> io::Result<MappedGuard<'_>> {
		self.map(false)
	}

	pub fn zeroize(&self) -> io::Result<()> {
		let mut guard = self.map(true)?;
		guard.zeroize();
		self.length.store(0, Ordering::Release);
		Ok(())
	}
}

impl Drop for SecretMemory {
	fn drop(&mut self) {
		// Closing the final fd also releases the secret pages if mapping fails.
		let _ = self.zeroize();
	}
}

pub struct MappedGuard<'a> {
	ptr: *mut u8,
	mapped_len: usize,
	visible_len: usize,
	_lock: MutexGuard<'a, ()>,
}
impl Deref for MappedGuard<'_> {
	type Target = [u8];
	fn deref(&self) -> &[u8] {
		unsafe { slice::from_raw_parts(self.ptr, self.visible_len) }
	}
}
impl DerefMut for MappedGuard<'_> {
	fn deref_mut(&mut self) -> &mut [u8] {
		unsafe { slice::from_raw_parts_mut(self.ptr, self.visible_len) }
	}
}
impl Drop for MappedGuard<'_> {
	fn drop(&mut self) {
		unsafe {
			munmap(self.ptr.cast(), self.mapped_len);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn mappings_are_exclusive_and_keep_the_full_unmap_length() {
		let secret = SecretMemory::new_pages(2).unwrap();
		secret.write(0, b"secret").unwrap();
		let guard = secret.read().unwrap();
		assert_eq!(&*guard, b"secret");
		assert_eq!(guard.mapped_len, secret.capacity);
		assert!(secret.mapping.try_lock().is_err());
		drop(guard);
		assert!(secret.mapping.try_lock().is_ok());
		secret.zeroize().unwrap();
		assert!(secret.read().unwrap().is_empty());
		assert!(secret.map(true).unwrap().iter().all(|b| *b == 0));
		assert!(secret.write(usize::MAX, b"x").is_err());
	}
}
