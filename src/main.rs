use std::{collections::HashMap, path::PathBuf};

use clap::Parser;
use git2::{Repository, Signature};
use indicatif::{ProgressBar, ProgressStyle};

#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Args {
    /// Path to Git repository to read commits from
    path: PathBuf,

    /// Branch, tag, or commit to read commits from
    reference: String,

    /// Path where new Git repository will be created
    target: PathBuf,

    /// Redact user identities with a cryptographic hash
    #[clap(short = 'u', long)]
    redact_users: bool,

    /// Key to use when hashing user identities
    key: Option<String>,
}

fn maybe_redact_signature<'a>(
    opts: &Args,
    blake3_key: &[u8; 32],
    signature: Signature<'a>,
) -> Signature<'a> {
    if opts.redact_users {
        let mut hasher = blake3::Hasher::new_keyed(blake3_key);
        hasher.update(signature.name_bytes());
        let mut hasher_output = hasher.finalize_xof();
        let mut name_bytes = [0u8; 12];
        hasher_output.fill(&mut name_bytes);

        let mut hasher = blake3::Hasher::new_keyed(blake3_key);
        let email_string = signature
            .email()
            .map(|e| e.trim().to_lowercase().into_bytes());
        hasher.update(
            email_string
                .as_ref()
                .map(|e| e.as_slice())
                .unwrap_or(signature.email_bytes()),
        );
        let mut hasher_output = hasher.finalize_xof();
        let mut email_bytes = [0u8; 12];
        hasher_output.fill(&mut email_bytes);

        Signature::new(
            &hex::encode(&name_bytes),
            &format!("{}@redacted.invalid", hex::encode(&email_bytes)),
            &signature.when(),
        )
        .unwrap()
    } else {
        signature
    }
}

fn main() {
    let args = Args::parse();

    let repo = Repository::open(&args.path).expect("failed to open repository");
    let reference = repo
        .resolve_reference_from_short_name(&args.reference)
        .expect("failed to open reference");
    let first_commit = reference
        .peel_to_commit()
        .expect("failed to peel to commit");

    let target_repo = Repository::init(&args.target).expect("failed to create target repo");
    let empty_tree = target_repo.treebuilder(None).unwrap().write().unwrap();
    let empty_tree = target_repo.find_tree(empty_tree).unwrap();

    let blake3_key = blake3::derive_key(
        "git-scrubber",
        args.key.as_deref().unwrap_or("default_key").as_bytes(),
    );

    let spinner_style =
        ProgressStyle::default_spinner().template("{spinner} {pos:>7} @ {per_sec} {msg}");
    let spinner = ProgressBar::new_spinner().with_style(spinner_style);
    spinner.set_draw_rate(10);

    let mut commit_map = HashMap::new();
    let mut stack = vec![first_commit.id()];
    while let Some(commit_oid) = stack.pop() {
        let commit = repo.find_commit(commit_oid).unwrap();
        let parents_complete = commit.parent_ids().all(|p| commit_map.contains_key(&p));
        if parents_complete {
            let mut parents_mapped = Vec::with_capacity(commit.parent_count());
            for parent in commit.parents() {
                let oid = commit_map.get(&parent.id()).unwrap();
                parents_mapped.push(target_repo.find_commit(*oid).unwrap());
            }

            let parent_references = parents_mapped.iter().map(|c| c).collect::<Vec<_>>();

            let target_oid = target_repo
                .commit(
                    None,
                    &maybe_redact_signature(&args, &blake3_key, commit.author()),
                    &maybe_redact_signature(&args, &blake3_key, commit.committer()),
                    "redacted",
                    &empty_tree,
                    &parent_references,
                )
                .unwrap();
            commit_map.insert(commit.id(), target_oid);
            spinner.inc(1);
        } else {
            stack.push(commit.id());
            for parent in commit.parent_ids() {
                stack.push(parent);
            }
        }
    }

    let oid = commit_map[&first_commit.id()];
    target_repo
        .branch("master", &target_repo.find_commit(oid).unwrap(), true)
        .unwrap();
}
