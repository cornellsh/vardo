//! Properties of merging, checked over many random pairs of edits.
//!
//! Each case starts from one real vault, applies a few random edits, deletes
//! and moves to a local and a remote copy, and seals both through the real
//! writer. Two properties must hold for every case:
//!
//! - Merging the same remote a second time changes nothing.
//! - Merging the remote into the local and the local into the remote ends in
//!   the same vault, when both sides resolve conflicts toward the same copy.
//!
//! Random but seeded: a failure prints its seed and the edits, and
//! `MERGE_SEED=<n>` reruns exactly that case.

mod harness;

use std::collections::BTreeMap;

use harness::{Sealed, SeededRng, Variant, PW};
use vardo_kdbx::Credentials;
use vardo_merge::{apply, dom, index, plan, ConflictPolicy};
use vardo_vault::Vault;

const FIXTURE: &str = "test_db_kdbx41_with_password_aes.kdbx";
const GROUPS: [&str; 2] = ["Group with tags", "Group that was moved"];
const CASES: u64 = 48;

/// What a vault holds, in a form two vaults can be compared by: each entry's
/// fields and its group, by UUID.
type Snapshot = BTreeMap<String, (Vec<(String, String, bool)>, Option<String>)>;

#[derive(Debug, Clone)]
enum Op {
    Edit {
        title: String,
        value: String,
        at: i64,
    },
    Delete {
        title: String,
        at: i64,
    },
    Move {
        title: String,
        group: &'static str,
        at: i64,
    },
}

struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }
}

/// Up to four edits on distinct entries. Times fall in a four-second window so
/// that equal seconds, which the merge treats as a conflict, come up often.
fn random_ops(rng: &mut Lcg, titles: &[String], side: &str) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut used: Vec<&String> = Vec::new();
    for _ in 0..=rng.below(4) {
        let title = &titles[rng.below(titles.len() as u64) as usize];
        if used.contains(&title) {
            continue;
        }
        used.push(title);
        // After every time in the fixture, as a real edit would be.
        let at = 2_000_000_000 + rng.below(4) as i64;
        ops.push(match rng.below(4) {
            0 => Op::Delete {
                title: title.clone(),
                at,
            },
            1 => Op::Move {
                title: title.clone(),
                group: GROUPS[rng.below(2) as usize],
                at,
            },
            _ => Op::Edit {
                title: title.clone(),
                value: format!("{side}-{}", rng.below(1000)),
                at,
            },
        });
    }
    ops
}

fn variant(ops: &[Op]) -> Variant {
    let mut v = Variant::open(FIXTURE);
    for op in ops {
        match op {
            Op::Edit { title, value, at } => {
                v.set_field(title, "UserName", value);
                v.set_time(title, "LastModificationTime", *at);
            }
            Op::Delete { title, at } => v.delete_entry(title, *at),
            Op::Move { title, group, at } => v.move_entry(title, group, *at),
        }
    }
    v
}

/// Plans `remote` into `local` against `base`, applies it, and seals the
/// result through the real writer.
fn merge(base: &Sealed, local: &Sealed, remote: &Sealed, policy: ConflictPolicy) -> Sealed {
    let (bv, rv) = (base.vault(), remote.vault());
    let mut lv = local.vault();
    let p = plan(Some(&bv), &lv, &rv).expect("one fixture, one root");
    apply(&p, &mut lv, &rv, policy).expect("apply");
    let bytes = lv
        .save_with(&Credentials::with_password(PW), &mut SeededRng(99))
        .expect("save");
    Sealed::from_bytes(bytes)
}

fn snapshot(v: &Vault) -> Snapshot {
    let root = v.root().map(|g| g.element().clone());
    v.entries()
        .iter()
        .filter_map(|e| {
            let uuid = e.uuid()?;
            let parent = root
                .as_ref()
                .and_then(|r| dom::parent_of(r, uuid))
                .map(|p| format!("{p:?}"));
            Some((format!("{uuid:?}"), (index::content(e).to_vec(), parent)))
        })
        .collect()
}

fn cases() -> Vec<u64> {
    match std::env::var("MERGE_SEED") {
        Ok(seed) => vec![seed.parse().expect("MERGE_SEED is a number")],
        Err(_) => (1..=CASES).collect(),
    }
}

fn titles(base: &Sealed) -> Vec<String> {
    let mut all: Vec<String> = base.vault().entries().iter().map(|e| e.title()).collect();
    // The harness finds entries by title, so only unambiguous ones are usable.
    let counts = all.iter().fold(BTreeMap::new(), |mut m, t| {
        *m.entry(t.clone()).or_insert(0) += 1;
        m
    });
    all.retain(|t| !t.is_empty() && counts[t] == 1);
    all
}

#[test]
fn merging_the_same_remote_twice_changes_nothing_the_second_time() {
    let base = Variant::open(FIXTURE).seal(1);
    let titles = titles(&base);
    for seed in cases() {
        let mut rng = Lcg(seed);
        let (lo, ro) = (
            random_ops(&mut rng, &titles, "local"),
            random_ops(&mut rng, &titles, "remote"),
        );
        let (local, remote) = (variant(&lo).seal(2), variant(&ro).seal(3));
        for policy in [ConflictPolicy::KeepLocal, ConflictPolicy::KeepRemote] {
            let once = merge(&base, &local, &remote, policy);
            let twice = merge(&base, &once, &remote, policy);
            assert_eq!(
                snapshot(&once.vault()),
                snapshot(&twice.vault()),
                "seed {seed}, {policy:?}\nlocal: {lo:?}\nremote: {ro:?}"
            );
        }
    }
}

#[test]
fn merging_in_either_direction_converges() {
    let base = Variant::open(FIXTURE).seal(1);
    let titles = titles(&base);
    for seed in cases() {
        let mut rng = Lcg(seed);
        let (lo, ro) = (
            random_ops(&mut rng, &titles, "local"),
            random_ops(&mut rng, &titles, "remote"),
        );
        let (local, remote) = (variant(&lo).seal(2), variant(&ro).seal(3));
        // Both resolve conflicts toward the remote copy.
        let here = merge(&base, &local, &remote, ConflictPolicy::KeepRemote);
        let there = merge(&base, &remote, &local, ConflictPolicy::KeepLocal);
        assert_eq!(
            snapshot(&here.vault()),
            snapshot(&there.vault()),
            "seed {seed}\nlocal: {lo:?}\nremote: {ro:?}"
        );
    }
}
