//! Boot: bring a Horizon device up into its identity.
//!
//! A device boots by finding its identity store, unlocking the one 32-byte master
//! everything turns on, and handing that master to the session it launches. The
//! standalone tools each do their own unlock (`identity unlock` tries a security
//! key then the passphrase; `compositor drm --background` re-derives from the
//! passphrase), so today a device that unlocked with a touch would still be asked
//! for the passphrase when the desktop opens. This crate is the seam that joins the
//! two: unlock once, then carry that master into the session, so a touch boots
//! straight into the desktop.
//!
//! Three pure steps, in order.
//!
//! 1. [`discover`] finds the store to boot. On a real device the store lives at a
//!    known place on the mounted Key; here that is a directory, identified by the
//!    `keysalt` marker every tool checks. Exactly one store must resolve; zero or
//!    many is an error a real boot surfaces rather than guessing which identity to
//!    open.
//! 2. [`unlock`] recovers the master, trying an enrolled keyslot through a present
//!    authenticator first (a [`identity::Authenticator`]: a FIDO2 key touched, a
//!    software token) and falling back to the passphrase. The keyslot path is the
//!    touch-to-boot path; the passphrase is the way in when no key is present or
//!    enrolled. The KDF ([`derive`]) is the single canonical Argon2id derivation
//!    the rest of the tools share, so a store made one way opens the other.
//! 3. [`prove`] opens the store with the recovered master and decrypts HEAD, the
//!    same proof `identity unlock` and `reconstitute open` use, so a wrong key
//!    fails here instead of launching a session onto a store it cannot read.
//!
//! [`boot`] runs all three and returns a [`Booted`]: the store, the proven master,
//! and how it was unlocked. Launching the actual desktop off that master is the one
//! part that needs a screen and a GPU, so it stays in the `horizon` binary behind
//! the compositor backends, eye-verified on hardware, exactly the headless split
//! the rest of Horizon uses. Everything in this crate is pure logic over a store on
//! disk, so it builds and is tested on every host with no device and no display.

mod error;

pub use error::{Error, Result};

use std::path::{Path, PathBuf};

use identity::{Authenticator, Keyslots};
use lifestream::{Lifestream, ObjectId};

// The store's salt file: the marker every tool checks ("is this a store?"), and the
// input the passphrase KDF reads. Its presence plus the object tier is what makes a
// directory a Horizon store.
const SALT_FILE: &str = "keysalt";
// The store's enrolled keyslots, one per device that can unlock it; absent until a
// key or token is enrolled.
const KEYSLOTS_FILE: &str = "keyslots";

/// How the master was unlocked at boot, for the boot log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// An enrolled keyslot, opened by a present authenticator (a security key, a
    /// token): the touch-to-boot path, no passphrase typed.
    Keyslot,
    /// The store passphrase: the fallback, when no key is present or enrolled.
    Passphrase,
}

impl Method {
    /// A one-word label for the boot log.
    pub fn label(self) -> &'static str {
        match self {
            Method::Keyslot => "security key / token",
            Method::Passphrase => "passphrase",
        }
    }
}

/// A booted identity: the store, its proven master, how it was unlocked, and what
/// HEAD proved to decrypt. The master never touches disk or the environment; the
/// session the caller launches holds it in memory exactly as the broker does.
pub struct Booted {
    /// The store that was opened.
    pub store: PathBuf,
    /// The 32-byte master, proven to open the store.
    pub master: [u8; 32],
    /// Which path unlocked it.
    pub method: Method,
    /// The store's HEAD, if it has one (a freshly initialized store has none).
    pub head: Option<ObjectId>,
    /// How many objects the store holds.
    pub objects: usize,
}

/// Does `dir` hold a Horizon store? The salt file is the marker every tool checks,
/// and `objects/` is the content tier, so both must be present. A path that has one
/// but not the other (a half-made or unrelated directory) is not a store.
pub fn is_store(dir: &Path) -> bool {
    dir.join(SALT_FILE).is_file() && dir.join("objects").is_dir()
}

/// Find the one identity store to boot under `root`: `root` itself if it is a
/// store, otherwise the single store among its immediate subdirectories (a Key
/// mounted at `root` holds the store in a known place under it). Zero stores, or
/// more than one, is an error: a boot must not guess which identity to open.
pub fn discover(root: &Path) -> Result<PathBuf> {
    if is_store(root) {
        return Ok(root.to_path_buf());
    }
    let read = std::fs::read_dir(root)
        .map_err(|e| Error::Discover(format!("read {}: {e}", root.display())))?;
    let mut found = Vec::new();
    for entry in read {
        let entry = entry.map_err(|e| Error::Discover(format!("read {}: {e}", root.display())))?;
        let path = entry.path();
        if path.is_dir() && is_store(&path) {
            found.push(path);
        }
    }
    found.sort();
    match found.len() {
        0 => Err(Error::Discover(format!(
            "no Horizon store at or under {}",
            root.display()
        ))),
        1 => Ok(found.pop().unwrap()),
        n => Err(Error::Discover(format!(
            "{n} stores under {} (name one with --store)",
            root.display()
        ))),
    }
}

/// The canonical Argon2id derivation of the master from a passphrase and the
/// store's salt. A store's `keysalt` plus its passphrase always yield the same
/// master, the one the Lifestream addresses with, the Constellation binds Noise to,
/// and Reconstitution splits; it is defined once here so every tool derives it
/// identically. Argon2 default parameters, matching how a store is initialized.
pub fn derive(passphrase: &str, salt: &[u8]) -> [u8; 32] {
    use argon2::Argon2;
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .expect("argon2 derive");
    key
}

/// The master from the store passphrase: read the store's salt and [`derive`]. The
/// fallback when no security key or token unlocks an enrolled keyslot.
pub fn passphrase_master(store: &Path, passphrase: &str) -> Result<[u8; 32]> {
    let salt = std::fs::read(store.join(SALT_FILE))
        .map_err(|e| Error::NotAStore(format!("{}: {e}", store.display())))?;
    Ok(derive(passphrase, &salt))
}

/// The store's enrolled keyslots, empty if none has been enrolled yet.
pub fn load_keyslots(store: &Path) -> Result<Keyslots> {
    match std::fs::read(store.join(KEYSLOTS_FILE)) {
        Ok(bytes) => Keyslots::decode(&bytes).map_err(|e| Error::Keyslots(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Keyslots::new()),
        Err(e) => Err(Error::Io(e)),
    }
}

/// Recover the store's master, trying an enrolled keyslot through a present
/// authenticator first and falling back to the passphrase. `auth` is the device a
/// holder presented (a FIDO2 key, a software token), or `None` for a passphrase-only
/// boot; when it is present and opens one of the store's enrolled keyslots, that
/// master is returned with no passphrase, which is the touch-to-boot path. Otherwise
/// `passphrase` is called for the typed secret and the master is derived from it, so
/// the prompt happens only when it is actually needed. Returns the master and which
/// path produced it. Does not open the store; callers [`prove`] it.
pub fn unlock(
    store: &Path,
    auth: Option<&mut (dyn Authenticator + '_)>,
    passphrase: impl FnOnce() -> Result<String>,
) -> Result<([u8; 32], Method)> {
    if let Some(auth) = auth {
        let slots = load_keyslots(store)?;
        if !slots.is_empty() {
            if let Ok(master) = slots.unlock_any(auth) {
                return Ok((master, Method::Keyslot));
            }
        }
    }
    let pass = passphrase()?;
    Ok((passphrase_master(store, &pass)?, Method::Passphrase))
}

/// Open the store with `master` and prove the key by decrypting HEAD, the same
/// proof `identity unlock` and `reconstitute open` use. A wrong master fails here
/// ([`Error::KeyMismatch`]) rather than launching a session onto a store it cannot
/// read. Returns the head and object count for the boot log. A store with no HEAD
/// (freshly initialized, never snapshotted) has nothing to decrypt, so the key is
/// accepted on opening alone, as the other tools also do.
pub fn prove(store: &Path, master: &[u8; 32]) -> Result<(Option<ObjectId>, usize)> {
    let ls = Lifestream::open(store, master)?;
    let head = ls.head()?;
    if let Some(h) = &head {
        ls.get(h).map_err(|_| Error::KeyMismatch)?;
    }
    Ok((head, ls.object_count()?))
}

/// The whole boot unlock: confirm the store, [`unlock`] the master (keyslot then
/// passphrase), and [`prove`] it opens the store. Everything the launched session
/// needs is in the returned [`Booted`]; the master stays in memory, never written.
pub fn boot(
    store: &Path,
    auth: Option<&mut (dyn Authenticator + '_)>,
    passphrase: impl FnOnce() -> Result<String>,
) -> Result<Booted> {
    if !is_store(store) {
        return Err(Error::NotAStore(store.display().to_string()));
    }
    let (master, method) = unlock(store, auth, passphrase)?;
    let (head, objects) = prove(store, &master)?;
    Ok(Booted {
        store: store.to_path_buf(),
        master,
        method,
        head,
        objects,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use identity::{enroll, SoftwareAuthenticator};
    use std::fs;

    // A passphrase whose Argon2id-derived master seeds the store, so the keyslot
    // path and the passphrase path resolve the same master.
    const PASS: &str = "correct horse battery staple";

    // Build a real store under `dir`: init it with the passphrase-derived master,
    // write the salt, and commit one snapshot so HEAD has something to decrypt.
    // Returns the master.
    fn make_store(dir: &Path) -> [u8; 32] {
        let salt = b"boot-unit-test-salt-0123456789ab";
        let master = derive(PASS, salt);
        let ls = Lifestream::init(dir, &master).unwrap();
        fs::write(dir.join(SALT_FILE), salt).unwrap();
        // A directory with one file, snapshotted and committed, gives the store a
        // HEAD generation that the master decrypts.
        let content = dir.join("content");
        fs::create_dir_all(&content).unwrap();
        fs::write(content.join("hello"), b"horizon").unwrap();
        let tree = ls.snapshot_dir(&content).unwrap();
        ls.commit(tree, vec![], "first").unwrap();
        master
    }

    // Persist a software token as one of the store's keyslots.
    fn enroll_token(store: &Path, master: &[u8; 32], seed: [u8; 32]) {
        let mut auth = SoftwareAuthenticator::new(seed);
        let mut slots = load_keyslots(store).unwrap();
        slots.add(enroll(&mut auth, master).unwrap());
        fs::write(store.join(KEYSLOTS_FILE), slots.encode()).unwrap();
    }

    #[test]
    fn is_store_needs_the_salt_and_the_objects() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        let master = make_store(&store);
        assert!(is_store(&store));
        let _ = master;

        // A bare directory is not a store.
        let empty = dir.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert!(!is_store(&empty));

        // Salt without the object tier is not a store either.
        let half = dir.path().join("half");
        fs::create_dir_all(&half).unwrap();
        fs::write(half.join(SALT_FILE), b"x").unwrap();
        assert!(!is_store(&half));
    }

    #[test]
    fn discover_finds_the_store_at_or_under_the_root() {
        let dir = tempfile::tempdir().unwrap();

        // root is itself a store.
        let direct = dir.path().join("direct");
        make_store(&direct);
        assert_eq!(discover(&direct).unwrap(), direct);

        // root holds exactly one store under it (the Key-mount case).
        let mount = dir.path().join("mount");
        let store = mount.join("identity");
        make_store(&store);
        // a sibling non-store directory should be ignored
        fs::create_dir_all(mount.join("not-a-store")).unwrap();
        assert_eq!(discover(&mount).unwrap(), store);
    }

    #[test]
    fn discover_refuses_to_guess_between_two_stores() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        make_store(&mount.join("a"));
        make_store(&mount.join("b"));
        assert!(matches!(discover(&mount), Err(Error::Discover(_))));

        let empty = dir.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert!(matches!(discover(&empty), Err(Error::Discover(_))));
    }

    #[test]
    fn derive_is_deterministic_and_salt_separated() {
        let a = derive(PASS, b"salt-one");
        assert_eq!(a, derive(PASS, b"salt-one"));
        assert_ne!(a, derive(PASS, b"salt-two"));
        assert_ne!(a, derive("other", b"salt-one"));
    }

    #[test]
    fn unlock_uses_the_token_without_the_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        let master = make_store(&store);
        enroll_token(&store, &master, [1u8; 32]);

        // The enrolled token unlocks the master and the passphrase is never asked.
        let mut token = SoftwareAuthenticator::new([1u8; 32]);
        let (key, method) = unlock(&store, Some(&mut token), || {
            panic!("passphrase must not be requested when the token unlocks")
        })
        .unwrap();
        assert_eq!(key, master);
        assert_eq!(method, Method::Keyslot);
    }

    #[test]
    fn unlock_falls_back_to_the_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        let master = make_store(&store);
        enroll_token(&store, &master, [1u8; 32]);

        // A token that was never enrolled matches no slot, so the passphrase is used.
        let mut stranger = SoftwareAuthenticator::new([9u8; 32]);
        let (key, method) = unlock(&store, Some(&mut stranger), || Ok(PASS.to_string())).unwrap();
        assert_eq!(key, master);
        assert_eq!(method, Method::Passphrase);

        // With no authenticator at all, the passphrase is the only path.
        let (key, method) = unlock(&store, None, || Ok(PASS.to_string())).unwrap();
        assert_eq!(key, master);
        assert_eq!(method, Method::Passphrase);
    }

    #[test]
    fn boot_unlocks_and_proves_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        let master = make_store(&store);
        enroll_token(&store, &master, [2u8; 32]);

        // Boot with the token: no passphrase, HEAD decrypts, master matches.
        let mut token = SoftwareAuthenticator::new([2u8; 32]);
        let booted = boot(&store, Some(&mut token), || {
            panic!("passphrase must not be requested")
        })
        .unwrap();
        assert_eq!(booted.master, master);
        assert_eq!(booted.method, Method::Keyslot);
        assert!(booted.head.is_some());
        assert!(booted.objects > 0);
        assert_eq!(booted.store, store);
    }

    #[test]
    fn boot_rejects_a_path_that_is_not_a_store() {
        let dir = tempfile::tempdir().unwrap();
        let not = dir.path().join("nope");
        std::fs::create_dir_all(&not).unwrap();
        assert!(matches!(
            boot(&not, None, || Ok(PASS.to_string())),
            Err(Error::NotAStore(_))
        ));
    }

    #[test]
    fn prove_rejects_a_wrong_master() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        make_store(&store);
        // A master that is not the store's must fail the HEAD decrypt.
        assert!(matches!(prove(&store, &[0u8; 32]), Err(Error::KeyMismatch)));
    }
}
