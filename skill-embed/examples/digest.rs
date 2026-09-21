//! Prints the digest of every skill under a directory.
//!
//! It is how the digest is compared against another implementation of it, and
//! against an installed copy, without a test having to embed the answer.

fn main() {
    let dir = std::env::args().nth(1).expect("usage: digest <skills dir>");
    let set = skill_embed::SkillSet::read_dir(std::path::Path::new(&dir)).expect("valid skills");
    for sk in set.skills() {
        println!("{} {}", sk.name(), sk.digest());
    }
}
