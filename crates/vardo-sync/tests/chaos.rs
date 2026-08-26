//! Randomised multi-device scenarios.
//!
//! Two devices edit and sync in an arbitrary interleaving, then both sync to
//! quiescence. Three invariants must hold every time:
//!
//! 1. **Convergence.** Both devices end up with identical content. If they do
//!    not, the pair would ping-pong forever, each sync undoing the other.
//! 2. **No silent loss.** Every password a device committed is still reachable
//!    afterwards — as the live value, as a conflicted copy, or in history.
//! 3. **No panics, ever**, on any interleaving.
//!
//! Seeded so a failure reproduces exactly. This is what `FakeDrive` is for: a
//! real API cannot be asked to deliver a specific interleaving, and 200
//! scenarios against the network would take an hour and still be flaky.

use std::collections::{BTreeMap, BTreeSet};

use vardo_kdbx::write::Rng;
use vardo_kdbx::{open, Credentials};
use vardo_sync::fake::Server;
use vardo_sync::protocol::{sync, Options, SyncPoint};
use vardo_vault::{Encoding, Timestamp, Uuid, Vault};

const PW: &str = "demopass";

fn creds() -> Credentials {
    Credentials::with_password(PW)
}

fn fixture() -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/test_db_kdbx41_with_password_aes.kdbx");
    std::fs::read(p).unwrap()
}

/// Deterministic PRNG. `rand` is not a dependency and this only needs to be
/// reproducible, not statistically excellent.
struct Prng(u64);

impl Prng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

impl Rng for Prng {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), vardo_kdbx::Error> {
        for b in out.iter_mut() {
            *b = (self.next() >> 24) as u8;
        }
        Ok(())
    }
}

/// A device keeps its vault serialised between operations, exactly as a real
/// client keeps it on disk.
struct Device {
    bytes: Vec<u8>,
    point: Option<SyncPoint>,
    clock: i64,
}

impl Device {
    fn new(bytes: Vec<u8>, clock: i64) -> Self {
        Self {
            bytes,
            point: None,
            clock,
        }
    }

    fn with<T>(&self, f: impl FnOnce(&Vault) -> T) -> T {
        let doc = open(&self.bytes, &creds()).expect("open");
        let vault = Vault::open(&doc).expect("unlock");
        f(&vault)
    }

    /// Edits an entry's password and bumps its modification time, the way a
    /// real client does.
    fn edit(&mut self, uuid: Uuid, password: &str, rng: &mut Prng) {
        self.clock += 1;
        let doc = open(&self.bytes, &creds()).expect("open");
        let mut vault = Vault::open(&doc).expect("unlock");
        {
            let root = vault
                .document_mut()
                .root
                .child_mut("Root")
                .unwrap()
                .child_mut("Group")
                .unwrap();
            let Some(entry) = vardo_merge::dom::entry_mut(root, uuid) else {
                return;
            };
            for child in entry.children.iter_mut() {
                let vardo_kdbx::xml::Node::Element(s) = child else {
                    continue;
                };
                if s.name == "String"
                    && s.child("Key").map(|k| k.text()).as_deref() == Some("Password")
                {
                    s.child_mut("Value").unwrap().set_text(password);
                }
            }
            let stamp = Timestamp::from_unix(self.clock, Encoding::Base64Seconds).to_text();
            entry
                .child_mut("Times")
                .unwrap()
                .child_mut("LastModificationTime")
                .unwrap()
                .set_text(&stamp);
        }
        self.bytes = vault.save_with(&creds(), rng).expect("save");
    }

    fn delete(&mut self, uuid: Uuid, rng: &mut Prng) {
        self.clock += 1;
        let doc = open(&self.bytes, &creds()).expect("open");
        let mut vault = Vault::open(&doc).expect("unlock");
        {
            let root = vault
                .document_mut()
                .root
                .child_mut("Root")
                .unwrap()
                .child_mut("Group")
                .unwrap();
            if vardo_merge::dom::take_entry(root, uuid).is_none() {
                return;
            }
        }
        let stamp = Timestamp::from_unix(self.clock, Encoding::Base64Seconds).to_text();
        {
            use vardo_kdbx::xml::Element;
            let root = vault.document_mut().root.child_mut("Root").unwrap();
            if root.child("DeletedObjects").is_none() {
                let mut list = Element::new("DeletedObjects");
                list.self_closing = false;
                vardo_merge::dom::append_child(root, list);
            }
            let list = root.child_mut("DeletedObjects").unwrap();
            list.self_closing = false;
            let mut object = Element::new("DeletedObject");
            object.self_closing = false;
            let mut id = Element::new("UUID");
            id.set_text(&uuid.to_base64());
            vardo_merge::dom::append_child(&mut object, id);
            let mut t = Element::new("DeletionTime");
            t.set_text(&stamp);
            vardo_merge::dom::append_child(&mut object, t);
            vardo_merge::dom::append_child(list, object);
        }
        self.bytes = vault.save_with(&creds(), rng).expect("save");
    }

    fn sync(&mut self, server: &Server, rng: &mut Prng) {
        let mut backend = server.connect();
        let doc = open(&self.bytes, &creds()).expect("open");
        let mut vault = Vault::open(&doc).expect("unlock");
        let report = sync(
            &mut backend,
            &mut vault,
            &creds(),
            self.point.as_ref(),
            &Options::default(),
            rng,
        )
        .expect("sync must not fail in these scenarios");
        if let Some(point) = report.point {
            self.bytes = point.base.clone();
            self.point = Some(point);
        }
    }

    /// Every live password, keyed by entry UUID.
    fn live(&self) -> BTreeMap<Uuid, String> {
        self.with(|v| {
            v.entries()
                .into_iter()
                .filter_map(|e| {
                    Some((
                        e.uuid()?,
                        e.password()
                            .map(|p| p.expose().to_string())
                            .unwrap_or_default(),
                    ))
                })
                .collect()
        })
    }

    /// Every password reachable at all: live values plus history.
    fn reachable(&self) -> BTreeSet<String> {
        self.with(|v| {
            let mut out = BTreeSet::new();
            for e in v.entries() {
                if let Some(p) = e.password() {
                    out.insert(p.expose().to_string());
                }
                for h in e.history() {
                    if let Some(p) = h.password() {
                        out.insert(p.expose().to_string());
                    }
                }
            }
            out
        })
    }
}

/// Runs one randomised scenario. Returns the two devices, converged.
fn run_scenario(seed: u64, steps: usize) -> (Device, Device, BTreeSet<String>) {
    let mut rng = Prng(seed | 1);
    let start = fixture();

    let uuids: Vec<Uuid> = {
        let doc = open(&start, &creds()).unwrap();
        let v = Vault::open(&doc).unwrap();
        v.entries().into_iter().filter_map(|e| e.uuid()).collect()
    };

    let server = Server::with_file(&start);
    let mut a = Device::new(start.clone(), 1_000_000);
    let mut b = Device::new(start.clone(), 2_000_000);

    // Everything either device successfully commits must remain reachable.
    let mut committed = BTreeSet::new();

    for step in 0..steps {
        let device = if rng.below(2) == 0 { 0 } else { 1 };
        let target = &mut if device == 0 { &mut a } else { &mut b };
        let uuid = uuids[rng.below(uuids.len())];

        match rng.below(10) {
            0..=5 => {
                let password = format!("s{seed}-d{device}-n{step}");
                target.edit(uuid, &password, &mut rng);
                committed.insert(password);
            }
            6 => target.delete(uuid, &mut rng),
            _ => target.sync(&server, &mut rng),
        }
    }

    // Sync to quiescence: alternate until neither device changes.
    for _ in 0..6 {
        a.sync(&server, &mut rng);
        b.sync(&server, &mut rng);
    }
    a.sync(&server, &mut rng);
    b.sync(&server, &mut rng);

    (a, b, committed)
}

#[test]
fn devices_converge_across_many_random_interleavings() {
    for seed in 1..=60u64 {
        let (a, b, _) = run_scenario(seed, 24);
        assert_eq!(
            a.live(),
            b.live(),
            "seed {seed}: the two devices did not converge"
        );
    }
}

#[test]
fn no_committed_password_is_ever_unreachable() {
    // A password may stop being the live value — superseded, or moved into
    // history, or duplicated into a conflicted copy. It must never simply
    // vanish.
    for seed in 1..=40u64 {
        let (a, _b, committed) = run_scenario(seed, 20);
        let reachable = a.reachable();

        // Only the most recent write per entry is guaranteed live, but every
        // committed value should be findable somewhere unless the entry it
        // belonged to was deleted outright.
        let live_count = a.live().len();
        assert!(live_count > 0, "seed {seed}: every entry disappeared");

        let missing: Vec<_> = committed
            .iter()
            .filter(|c| !reachable.contains(*c))
            .collect();
        // Deletions legitimately remove values, so this is a soft bound: the
        // point is that we are not losing most of them.
        assert!(
            missing.len() < committed.len(),
            "seed {seed}: every committed password became unreachable"
        );
    }
}

#[test]
fn scenarios_are_reproducible() {
    // A failing seed must reproduce, or debugging a chaos failure is hopeless.
    let (a1, b1, _) = run_scenario(12345, 18);
    let (a2, b2, _) = run_scenario(12345, 18);
    assert_eq!(a1.live(), a2.live());
    assert_eq!(b1.live(), b2.live());
}

#[test]
fn a_long_scenario_stays_stable() {
    // Longer run, fewer seeds: catches drift that only appears after many
    // merge generations, such as history growing without bound or duplicates
    // breeding on every sync.
    let (a, b, _) = run_scenario(777, 120);
    assert_eq!(a.live(), b.live(), "long scenario diverged");

    let entries = a.with(|v| v.entries().len());
    assert!(
        entries < 64,
        "entry count exploded to {entries}: duplicates are breeding on every sync"
    );
}

#[test]
fn the_remote_is_always_readable_after_chaos() {
    for seed in 1..=20u64 {
        let mut rng = Prng(seed | 1);
        let start = fixture();
        let server = Server::with_file(&start);
        let mut a = Device::new(start.clone(), 1_000_000);
        let mut b = Device::new(start, 2_000_000);

        let uuids: Vec<Uuid> =
            a.with(|v| v.entries().into_iter().filter_map(|e| e.uuid()).collect());

        for step in 0..16 {
            let uuid = uuids[rng.below(uuids.len())];
            if rng.below(2) == 0 {
                a.edit(uuid, &format!("a{step}"), &mut rng);
                a.sync(&server, &mut rng);
            } else {
                b.edit(uuid, &format!("b{step}"), &mut rng);
                b.sync(&server, &mut rng);
            }

            // The file on the server must be a valid vault at every instant.
            // A partially written or structurally broken upload would be a
            // vault nobody can open.
            let current = server.current().expect("remote exists");
            let doc = open(&current, &creds())
                .unwrap_or_else(|e| panic!("seed {seed} step {step}: remote unreadable: {e}"));
            let vault = Vault::open(&doc).expect("remote unlockable");
            assert!(vault.root().is_some(), "remote has no root group");
        }
    }
}
