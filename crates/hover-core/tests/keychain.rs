//! note.key on macOS against the real login Keychain. Skipped (and says so) where there
//! is none to write to (a CI runner's locked keychain, an ssh session). The item it
//! makes is deleted again.
#![cfg(target_os = "macos")]

use hover_core::crypto::KeyGuard;
use hover_core::platform::macos::{account, marker_id, Keychain, Login, SERVICE};
use hover_core::platform::{keychain_find, SystemKeyGuard};

#[test]
fn the_key_round_trips_through_the_login_keychain() {
    // Only by hand (cargo test -p hover-core --test keychain): a test run never asks the
    // user's Keychain, which shows a prompt on their screen.
    if hover_core::platform::macos::keychain_off() { eprintln!("skipped: the Keychain is off in test runs"); return; }
    let guard = SystemKeyGuard;
    let key = [0x5au8; 32];
    let stored = guard.wrap(&key).unwrap();
    let Some(id) = marker_id(&stored).map(str::to_owned) else {
        assert_eq!(stored, key, "without a Keychain, note.key holds the key");
        eprintln!("skipped: no login Keychain to write to");
        return;
    };
    let read = guard.unwrap(&stored);
    let found = Login.get(SERVICE, &account(&id));
    let by_service = keychain_find(SERVICE);
    let _ = security_framework::passwords::delete_generic_password(SERVICE, &account(&id));
    assert_eq!(read.unwrap(), key);
    assert_eq!(found.unwrap(), Some(key.to_vec()));
    assert!(by_service.is_ok(), "a search by service alone works");
    // Deleted: gone for good, not "try again later".
    let gone = guard.unwrap(&stored).unwrap_err();
    assert!(!gone.transient, "{gone:?}");
}

#[test]
fn an_item_that_is_not_there_is_none() {
    assert_eq!(Login.get("Hover", "note.key:does-not-exist-0000").unwrap(), None);
    assert_eq!(keychain_find("Hover-no-such-service-0000").unwrap(), None);
}
