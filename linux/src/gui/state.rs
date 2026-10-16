use crate::gui::{components, messages::Message};
use crate::storage::{database::Database, entry::PasswordEntry, persistence};
use iced::{
	Element, Event, Size, Subscription, Task, application, keyboard,
	widget::{
		Id,
		operation::{focus_next, focus_previous},
		text_editor,
	},
	window,
};
use std::{
	thread,
	time::{Duration, Instant},
};
use zeroize::Zeroizing;

struct LockedState {
	db_name: String,
	db_password: Zeroizing<String>,
	is_processing: bool,
}

struct UnlockedState {
	query: String,
	password_entry: PasswordEntry,
	is_password_visible: bool,
	is_pin_visible: bool,
	is_note_visible: bool,
	last_copy_time: Option<Instant>,
	last_interaction_time: Option<Instant>,
	note: text_editor::Content,
	status: String,
	db: Database,
}

enum AppState {
	Locked(LockedState),
	Unlocked(UnlockedState),
}

pub const APP_NAME: &str = "DigiSafe";
const COPY_TIMEOUT: Duration = Duration::from_secs(if cfg!(debug_assertions) { 10 } else { 20 });
const IDLE_TIMEOUT: Duration = Duration::from_secs(if cfg!(debug_assertions) { 60 } else { 240 });

pub type Result = iced::Result;

pub fn run() -> Result {
	application(new, update, view)
		.subscription(|app_state| {
			Subscription::batch(vec![
				keyboard_subscription(app_state),
				tick_subscription(app_state),
			])
		})
		.theme(components::theme())
		.title(APP_NAME)
		.window(window::Settings {
			size: Size::new(1000.0, 800.0),
			position: window::Position::Centered,
			decorations: false,
			transparent: false,
			..Default::default()
		})
		.run()
}

fn new() -> AppState {
	AppState::Locked(LockedState {
		db_name: "".into(),
		db_password: Zeroizing::new(String::new()),
		is_processing: false,
	})
}

fn update(app_state: &mut AppState, message: Message) -> Task<Message> {
	match app_state {
		AppState::Locked(_) => update_locked(message, app_state),
		AppState::Unlocked(_) => update_unlocked(message, app_state),
	}
}

fn tick_subscription(app_state: &AppState) -> Subscription<Message> {
	if let AppState::Unlocked(_) = &app_state {
		iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick)
	} else {
		Subscription::none()
	}
}

fn keyboard_subscription(_app_state: &AppState) -> Subscription<Message> {
	iced::event::listen().map(|event| match event {
		Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
			if key == keyboard::Key::Named(keyboard::key::Named::Tab) {
				if modifiers.shift() {
					Message::FocusPrevious
				} else {
					Message::FocusNext
				}
			} else if key == keyboard::Key::Named(keyboard::key::Named::ArrowLeft) {
				if modifiers.control() {
					Message::NavigatePrevious
				} else {
					Message::None
				}
			} else if key == keyboard::Key::Named(keyboard::key::Named::ArrowRight) {
				if modifiers.control() {
					Message::NavigateNext
				} else {
					Message::None
				}
			} else {
				Message::None
			}
		}
		_ => Message::None,
	})
}

fn view(app_state: &AppState) -> Element<'_, Message> {
	match app_state {
		AppState::Locked(LockedState {
			db_name,
			db_password,
			is_processing,
		}) => components::unlock_screen(db_name, db_password, is_processing),
		AppState::Unlocked(UnlockedState {
			query,
			password_entry,
			is_password_visible,
			is_pin_visible,
			is_note_visible,
			last_copy_time: _,
			last_interaction_time: _,
			note,
			status,
			db: _,
		}) => components::password_screen(
			query,
			password_entry,
			is_password_visible,
			is_pin_visible,
			is_note_visible,
			note,
			status,
		),
	}
}

fn update_locked(message: Message, app_state: &mut AppState) -> Task<Message> {
	let AppState::Locked(LockedState {
		db_name,
		db_password,
		is_processing,
	}) = app_state
	else {
		unreachable!("update_locked called with non-Locked state")
	};
	match message {
		Message::AttemptUnlock => {
			if *is_processing {
				return Task::none();
			}
			*is_processing = true;
			let db_name_clone = db_name.clone();
			let unlock_password = std::mem::replace(db_password, Zeroizing::new(String::new()));
			return Task::perform(
				async move { persistence::load(&db_name_clone, unlock_password).await },
				Message::UnlockResult,
			);
		}
		Message::UnlockResult(db) => {
			*app_state = AppState::Unlocked(UnlockedState {
				query: "".into(),
				password_entry: PasswordEntry::new(),
				is_password_visible: false,
				is_pin_visible: false,
				is_note_visible: false,
				last_copy_time: None,
				last_interaction_time: Some(Instant::now()),
				note: text_editor::Content::new(),
				status: "Unlocked".into(),
				db,
			});
		}
		Message::DbNameChanged(new_db_name) => {
			*db_name = new_db_name;
		}
		Message::DbPasswordChanged(new_password) => {
			*db_password = new_password;
		}
		Message::CloseWindow => {
			return window::latest().and_then(window::close);
		}
		Message::DragWindow => {
			return window::latest().and_then(window::drag);
		}
		Message::FocusPrevious => {
			return focus_previous::<Id>().discard();
		}
		Message::FocusNext => {
			return focus_next::<Id>().discard();
		}
		_ => {}
	}
	Task::none()
}

fn update_unlocked(message: Message, app_state: &mut AppState) -> Task<Message> {
	let AppState::Unlocked(UnlockedState {
		query,
		password_entry,
		is_password_visible,
		is_pin_visible,
		is_note_visible,
		last_copy_time,
		last_interaction_time,
		note,
		status,
		db,
	}) = app_state
	else {
		unreachable!("update_unlocked called with locked state")
	};
	const IDLE_MESSAGE: &str = "Idle time remaining:";
	match message {
		Message::None | Message::Tick => {}
		_ => {
			*last_interaction_time = Some(Instant::now());
			if status.starts_with(IDLE_MESSAGE) {
				*status = "".to_string();
			}
		}
	}
	match message {
		Message::QueryInput(new_text) => {
			*query = new_text;
		}
		Message::QuerySubmit => {
			return Task::done(Message::PasswordEntryGet);
		}
		Message::PasswordEntryNameInput(new_text) => {
			password_entry.set_name(&new_text);
		}
		Message::PasswordEntryUsernameInput(new_text) => {
			password_entry.set_username(&new_text);
		}
		Message::PasswordEntryPasswordInput(new_text) => {
			if *is_password_visible {
				password_entry.set_password(&new_text);
			}
		}
		Message::PasswordEntryPinInput(new_text) => {
			if *is_pin_visible {
				password_entry.set_pin(&new_text);
			}
		}
		Message::PasswordEntryUrlInput(new_text) => {
			password_entry.set_url(&new_text);
		}
		Message::PasswordEntryTagsInput(new_text) => {
			password_entry.set_tags(&new_text);
		}
		Message::PasswordEntryNoteAction(action) => {
			if *is_note_visible {
				note.perform(action);
				password_entry.set_note(&Zeroizing::new(note.text()));
			}
		}
		Message::NavigatePrevious => {
			*is_password_visible = false;
			*is_pin_visible = false;
			*is_note_visible = false;
			if let Some(found_password_entry) = db.previous(password_entry.get_name()) {
				*status = "Previous entry retrieved.".to_string();
				*password_entry = found_password_entry.clone();
				*note = text_editor::Content::new();
			} else {
				*status = "Previous entry not retrieved.".to_string();
				*password_entry = PasswordEntry::new();
				*note = text_editor::Content::new();
			};
		}
		Message::NavigateNext => {
			*is_password_visible = false;
			*is_pin_visible = false;
			*is_note_visible = false;
			if let Some(found_password_entry) = db.next(password_entry.get_name()) {
				*status = "Next entry retrieved.".to_string();
				*password_entry = found_password_entry.clone();
				*note = text_editor::Content::new();
			} else {
				*status = "Next entry not retrieved.".to_string();
				*password_entry = PasswordEntry::new();
				*note = text_editor::Content::new();
			};
		}
		Message::PasswordEntryGet => {
			*is_password_visible = false;
			*is_pin_visible = false;
			*is_note_visible = false;
			if let Some(found_password_entry) = db.query(query) {
				*status = "Entry retrieved.".to_string();
				*password_entry = found_password_entry.clone();
				*note = text_editor::Content::new();
			} else {
				*status = "Entry not retrieved.".to_string();
				*password_entry = PasswordEntry::new();
				*note = text_editor::Content::new();
			};
		}
		Message::PasswordEntrySet => {
			let name = password_entry.get_name();
			let note_string = password_entry.get_note();
			if !name.is_empty() {
				if let Some(mut old_entry) = db.get_password_entry_by_name(name) {
					old_entry.set_username(password_entry.get_username());
					old_entry.set_password(&password_entry.get_password());
					old_entry.set_pin(&password_entry.get_pin());
					old_entry.set_url(password_entry.get_url());
					old_entry.set_tags(password_entry.get_tags());
					old_entry.set_note(&note_string);
					db.set_password_entry(old_entry);
					*status = "Entry set.".to_string();
				} else {
					let mut new_entry = PasswordEntry::new();
					new_entry.set_name(name);
					new_entry.set_username(password_entry.get_username());
					new_entry.set_password(&password_entry.get_password());
					new_entry.set_pin(&password_entry.get_pin());
					new_entry.set_url(password_entry.get_url());
					new_entry.set_tags(password_entry.get_tags());
					new_entry.set_note(&note_string);
					db.set_password_entry(new_entry.clone());
					*password_entry = new_entry;
					*status = "New entry set.".to_string();
				}
			} else {
				*status = "Name cannot be empty.".to_string();
			}
		}
		Message::Save => {
			*status = "Saving Database...".to_string();
			let db_clone = db.clone();
			return Task::perform(
				async move { persistence::save(&db_clone).await },
				Message::SaveResult,
			);
		}
		Message::SaveResult(msg) => {
			*status = msg;
		}
		Message::CloseWindow => {
			return window::latest().and_then(window::close);
		}
		Message::DragWindow => {
			return window::latest().and_then(window::drag);
		}
		Message::FocusPrevious => {
			return focus_previous::<Id>().discard();
		}
		Message::FocusNext => {
			return focus_next::<Id>().discard();
		}
		Message::ToggleNoteVisibility => {
			*is_note_visible = !*is_note_visible;
			*note = if *is_note_visible {
				text_editor::Content::with_text(&password_entry.get_note())
			} else {
				text_editor::Content::new()
			};
		}
		Message::TogglePasswordVisibility => {
			*is_password_visible = !*is_password_visible;
		}
		copy @ (Message::CopyUsername | Message::CopyPassword | Message::CopyPin) => {
			*last_copy_time = Some(Instant::now());
			let (label, value) = match copy {
				Message::CopyUsername => (
					"Username",
					Zeroizing::new(password_entry.get_username().to_owned()),
				),
				Message::CopyPassword => ("Password", password_entry.get_password()),
				Message::CopyPin => ("PIN", password_entry.get_pin()),
				_ => unreachable!(),
			};
			*status = format!("{label} copied.");
			thread::spawn(move || {
				use arboard::{Clipboard, LinuxClipboardKind as LCK, SetExtLinux};
				let mut cb = Clipboard::new().unwrap();
				let _ = cb
					.set()
					.clipboard(LCK::Clipboard)
					.exclude_from_history()
					.text(value.as_str());
				drop(value);
				thread::sleep(COPY_TIMEOUT);
			});
		}
		Message::TogglePinVisibility => {
			*is_pin_visible = !*is_pin_visible;
		}

		Message::ClearClipboard => {
			*status = "Clearing clipboard...".to_string();
			thread::spawn(move || {
				use arboard::{Clipboard, LinuxClipboardKind as LCK, SetExtLinux};
				let mut cb = Clipboard::new().unwrap();
				for idx in 0..60 {
					let mut noise = [0u8; 16];
					getrandom::fill(&mut noise).unwrap();
					let munge = format!(
						"DIGISAFE_CLIPBOARD_MUNGE_{:03}_{:040}",
						idx,
						u128::from_le_bytes(noise)
					);
					let _ = cb
						.set()
						.clipboard(LCK::Clipboard)
						.exclude_from_history()
						.text(&munge);
					let _ = cb
						.set()
						.clipboard(LCK::Primary)
						.exclude_from_history()
						.text(munge);
					thread::sleep(Duration::from_millis(4));
				}
				let _ = cb
					.set()
					.clipboard(LCK::Clipboard)
					.exclude_from_history()
					.text("");
				let _ = cb
					.set()
					.clipboard(LCK::Primary)
					.exclude_from_history()
					.text("");
				thread::sleep(Duration::from_millis(200));
			});
			*last_copy_time = None;
			*status = "Clipboard cleared.".to_string();
		}
		Message::Tick => {
			if let Some(lit) = last_interaction_time {
				let idle_elapsed = lit.elapsed();
				let idle_remaining = IDLE_TIMEOUT.saturating_sub(idle_elapsed).as_secs();
				if idle_remaining <= 20 {
					*status = format!("{IDLE_MESSAGE} {idle_remaining}s");
				}
				if idle_elapsed >= IDLE_TIMEOUT {
					return Task::done(Message::Lock);
				}
			}
			if let Some(lct) = *last_copy_time {
				let copy_elapsed = lct.elapsed();
				if copy_elapsed >= Duration::from_secs(4) {
					let copy_remaining = COPY_TIMEOUT.saturating_sub(copy_elapsed).as_secs();
					*status = format!("Clipboard clear time remaining: {copy_remaining}s");
				}
				if copy_elapsed >= COPY_TIMEOUT {
					return Task::done(Message::ClearClipboard);
				}
			}
		}
		Message::Lock => {
			db.zeroize();
			*app_state = AppState::Locked(LockedState {
				db_name: "".into(),
				db_password: Zeroizing::new(String::new()),
				is_processing: false,
			});
		}
		_ => {}
	}
	Task::none()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn unlocked() -> AppState {
		let db = Database::test_database();
		let mut entry = PasswordEntry::new();
		entry.set_name("example");
		entry.set_password("secret");
		entry.set_pin("1234");
		entry.set_note("private note");
		db.set_password_entry(entry);
		let mut app = new();
		let _ = update(&mut app, Message::UnlockResult(db));
		let _ = update(&mut app, Message::QueryInput("example".into()));
		let _ = update(&mut app, Message::PasswordEntryGet);
		app
	}

	#[test]
	fn hidden_fields_reveal_edit_hide_and_save_without_losing_secrets() {
		let mut app = unlocked();
		let AppState::Unlocked(state) = &app else {
			panic!()
		};
		assert!(!state.is_password_visible && !state.is_pin_visible && !state.is_note_visible);
		assert!(state.note.text().is_empty());
		let _ = view(&app);
		let _ = update(
			&mut app,
			Message::PasswordEntryPasswordInput(Zeroizing::new("ignored".into())),
		);
		let _ = update(&mut app, Message::PasswordEntrySet);
		let AppState::Unlocked(state) = &app else {
			panic!()
		};
		let saved = state.db.get_password_entry_by_name("example").unwrap();
		assert_eq!(saved.get_password().as_str(), "secret");
		assert_eq!(saved.get_note().as_str(), "private note");
		let _ = update(&mut app, Message::TogglePasswordVisibility);
		let _ = update(
			&mut app,
			Message::PasswordEntryPasswordInput(Zeroizing::new("replacement".into())),
		);
		let _ = update(&mut app, Message::TogglePasswordVisibility);
		let _ = update(&mut app, Message::ToggleNoteVisibility);
		let AppState::Unlocked(state) = &app else {
			panic!()
		};
		assert_eq!(state.note.text(), "private note");
		let _ = update(&mut app, Message::ToggleNoteVisibility);
		let _ = update(&mut app, Message::PasswordEntrySet);
		let _ = update(&mut app, Message::PasswordEntryGet);
		let AppState::Unlocked(state) = &app else {
			panic!()
		};
		assert!(!state.is_password_visible && !state.is_note_visible);
		assert!(state.note.text().is_empty());
		assert_eq!(state.password_entry.get_password().as_str(), "replacement");
		assert_eq!(state.password_entry.get_pin().as_str(), "1234");
		let _ = update(&mut app, Message::Lock);
		assert!(matches!(app, AppState::Locked(_)));
	}

	#[test]
	fn unlock_moves_password_out_of_ui_and_ignores_duplicate_submit() {
		let mut app = new();
		let _ = update(
			&mut app,
			Message::DbPasswordChanged(Zeroizing::new("master secret".into())),
		);
		let task = update(&mut app, Message::AttemptUnlock);
		let AppState::Locked(state) = &app else {
			panic!()
		};
		assert!(state.db_password.is_empty());
		assert!(state.is_processing);
		let _ = update(&mut app, Message::AttemptUnlock);
		// Dropping the unexecuted task also drops its wiping password buffer.
		drop(task);
	}
}
