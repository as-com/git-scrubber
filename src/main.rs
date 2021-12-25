use std::{collections::HashMap, path::PathBuf};

use clap::Parser;
use git2::{Commit, Oid, Repository, Tree};
use indicatif::{ProgressBar, ProgressStyle};

#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Args {
    path: PathBuf,
    reference: String,
    target: PathBuf,
}

// fn copy_commits(commit: Commit, target_repo: &Repository, tree: &Tree) -> Oid {
//     let mut copied_parent_commits = Vec::with_capacity(commit.parent_count());
//     for parent in commit.parents() {
//         let oid = copy_commits(parent, target_repo, tree);
//         copied_parent_commits.push(target_repo.find_commit(oid).unwrap());
//     }

//     let parent_references = copied_parent_commits.iter().map(|c| c).collect::<Vec<_>>();

//     target_repo.commit(None, &commit.author(), &commit.committer(), "redacted", tree, &parent_references).unwrap()
// }

fn main() {
    let args = Args::parse();

    let repo = Repository::open(args.path).expect("failed to open repository");
    let reference = repo
        .resolve_reference_from_short_name(&args.reference)
        .expect("failed to open reference");
    let first_commit = reference
        .peel_to_commit()
        .expect("failed to peel to commit");

    let target_repo = Repository::init(args.target).expect("failed to create target repo");
    let empty_tree = target_repo.treebuilder(None).unwrap().write().unwrap();
    let empty_tree = target_repo.find_tree(empty_tree).unwrap();

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
                    &commit.author(),
                    &commit.committer(),
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
