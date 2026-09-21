use std::borrow::Cow;
use std::path::Path;

use crate::tree::{self, Tree};
use crate::{Error, Result, manifest};

/// The manifest every skill directory must contain, as defined by the
/// [Agent Skills specification](https://agentskills.io/specification).
pub const SKILL_FILE: &str = manifest::FILE_NAME;

/// One embedded skill directory.
#[derive(Clone, Debug)]
pub struct Skill {
    name: String,
    description: String,
    dir: String,
    digest: String,
    tree: Tree,
}

impl Skill {
    /// The directory name the skill is installed under.
    ///
    /// It comes from the `name` frontmatter field, falling back to the source
    /// directory name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The `description` frontmatter field, if any.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The skill's path inside the source tree.
    #[must_use]
    pub fn dir(&self) -> &str {
        &self.dir
    }

    /// The SHA-256 of the skill's contents, recorded in the installed
    /// `SKILL.md`.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub(crate) fn tree(&self) -> &Tree {
        &self.tree
    }
}

/// A collection of embedded skills, in name order.
#[derive(Clone, Debug, Default)]
pub struct SkillSet {
    skills: Vec<Skill>,
}

impl SkillSet {
    /// Reads skills from an [`include_dir`] tree.
    ///
    /// The macro's root is the skills directory itself, which holds one
    /// directory per skill:
    ///
    /// ```no_run
    /// use include_dir::{Dir, include_dir};
    /// use skill_embed::SkillSet;
    ///
    /// static SKILLS_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/skills");
    ///
    /// let skills = SkillSet::from_include_dir(&SKILLS_DIR).expect("the embedded skills are valid");
    /// ```
    ///
    /// # Errors
    ///
    /// Fails when a skill carries no [`SKILL_FILE`], when two skills claim one
    /// name, or when a file an operating system left behind was embedded with
    /// them.
    #[cfg(feature = "include_dir")]
    pub fn from_include_dir(dir: &'static include_dir::Dir<'static>) -> Result<Self> {
        fn walk(dir: &'static include_dir::Dir<'static>, out: &mut Vec<tree::File>) {
            for file in dir.files() {
                out.push(tree::File {
                    path: file.path().to_string_lossy().replace('\\', "/"),
                    data: Cow::Borrowed(file.contents()),
                    executable: false,
                });
            }
            for sub in dir.dirs() {
                walk(sub, out);
            }
        }
        let mut files = Vec::new();
        walk(dir, &mut files);
        Self::from_tree_files(files)
    }

    /// Reads skills from a directory on disk.
    ///
    /// This is what a tool that does not embed its skills uses, and what a test
    /// uses to build the set a fixture describes.
    ///
    /// # Errors
    ///
    /// Fails for the reasons [`SkillSet::from_include_dir`] does, and when the
    /// directory cannot be read.
    pub fn read_dir(root: &Path) -> Result<Self> {
        let files = tree::read_dir_raw(root)
            .map_err(|e| Error::io(format!("read {}", root.display()), e))?;
        Self::from_tree_files(files)
    }

    /// Reads skills from an arbitrary set of files, each path separated with
    /// `/` and relative to the skills directory.
    ///
    /// It is what [`SkillSet::from_include_dir`] and [`SkillSet::read_dir`] are
    /// written in terms of, and what a tool embedding its skills some other way
    /// uses.
    ///
    /// # Errors
    ///
    /// Fails when a skill carries no [`SKILL_FILE`], when two skills claim one
    /// name, or when a file an operating system left behind was embedded with
    /// them.
    pub fn from_files(files: impl IntoIterator<Item = File>) -> Result<Self> {
        Self::from_tree_files(files.into_iter().map(|f| f.0).collect())
    }

    fn from_tree_files(files: Vec<tree::File>) -> Result<Self> {
        let junk: Vec<String> =
            files.iter().filter(|f| tree::is_junk(&f.path)).map(|f| f.path.clone()).collect();
        let whole = Tree::from_files(files);
        let mut skills = if whole.get(SKILL_FILE).is_some() {
            vec![read_skill(&whole, "")?]
        } else {
            let mut dirs: Vec<&str> = whole
                .files()
                .iter()
                .filter_map(|f| f.path.split_once('/').map(|(d, _)| d))
                .collect();
            dirs.dedup();
            dirs.iter()
                .filter(|d| whole.get(&format!("{d}/{SKILL_FILE}")).is_some())
                .map(|d| read_skill(&whole, d))
                .collect::<Result<Vec<_>>>()?
        };
        if skills.is_empty() {
            return Err(Error::Skills(format!("no {SKILL_FILE} found among the embedded files")));
        }
        // Refused rather than skipped, but only inside a skill. An installed
        // skill sits where a file browser can reach it, and one of these
        // appearing beside it is not the user editing the skill. An embedded
        // one is different: it was committed, and it ships to everyone.
        //
        // One that is not inside any skill ships too, and is never installed.
        // Failing the whole set over it would crash a user's binary over a file
        // the skills do not contain.
        let inside = |p: &String| {
            skills
                .iter()
                // The separator matters: `demo-other/x` is not inside `demo`.
                .any(|sk| sk.dir.is_empty() || p.starts_with(&format!("{}/", sk.dir)))
        };
        if let Some(path) = junk.iter().find(|p| inside(p)) {
            return Err(Error::Skills(format!(
                "{path} was left behind by an operating system and was embedded with a skill. \
                 Remove the file"
            )));
        }
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        check_names(&skills)?;
        Ok(Self { skills })
    }

    /// The skills in the set.
    #[must_use]
    pub fn skills(&self) -> &[Skill] {
        &self.skills
    }

    /// Finds a skill by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name == name)
    }

    /// The skill names, in the same order as [`SkillSet::skills`].
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.skills.iter().map(Skill::name)
    }

    /// How many skills the set holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Whether the set holds no skills.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }
}

/// One file on its way into a [`SkillSet`].
#[derive(Clone, Debug)]
pub struct File(tree::File);

impl File {
    /// Names a file by its path relative to the skills directory, separated
    /// with `/`.
    #[must_use]
    pub fn new(path: impl Into<String>, data: impl Into<Cow<'static, [u8]>>) -> Self {
        Self(tree::File { path: path.into(), data: data.into(), executable: false })
    }
}

fn read_skill(whole: &Tree, dir: &str) -> Result<Skill> {
    let path = if dir.is_empty() { SKILL_FILE.to_owned() } else { format!("{dir}/{SKILL_FILE}") };
    let src = whole.get(&path).ok_or_else(|| Error::Skills(format!("{path} is missing")))?;
    let fields = manifest::fields(src);
    let name = match fields.get("name") {
        Some(name) if !name.is_empty() => name.clone(),
        // A skill in a directory takes that directory's name. One at the root
        // of the tree has no directory to take a name from, so it has to carry
        // one.
        _ if dir.is_empty() => {
            return Err(Error::Skills(format!(
                "the {SKILL_FILE} at the root of the tree has no `name` field, \
                 and there is no directory to take one from"
            )));
        }
        _ => dir.rsplit('/').next().unwrap_or(dir).to_owned(),
    };
    check_name(&name).map_err(|e| Error::Skills(format!("{dir}: {e}")))?;

    let tree = if dir.is_empty() { whole.clone() } else { whole.subtree(dir) };
    Ok(Skill {
        digest: tree.digest(),
        description: fields.get("description").cloned().unwrap_or_default(),
        dir: dir.to_owned(),
        name,
        tree,
    })
}

/// Rejects anything that could escape the destination directory or be hidden
/// from the agent that reads it.
fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(format!("invalid skill name {name:?}"));
    }
    if name.contains(['/', '\\']) {
        return Err(format!("skill name {name:?} must not contain a path separator"));
    }
    if name.starts_with('.') {
        return Err(format!("skill name {name:?} must not start with a dot"));
    }
    Ok(())
}

/// Names are compared without case, because the file system they are installed
/// on usually is. Two skills differing only in case would install over each
/// other, and every run would flip the directory's contents while reporting
/// success.
fn check_names(skills: &[Skill]) -> Result<()> {
    let mut seen: Vec<(String, &Skill)> = Vec::new();
    for sk in skills {
        let key = sk.name.to_lowercase();
        if let Some((_, prev)) = seen.iter().find(|(k, _)| *k == key) {
            return Err(Error::Skills(if prev.name == sk.name {
                format!("{} and {} both declare the skill name {:?}", prev.dir, sk.dir, sk.name)
            } else {
                format!(
                    "{} and {} declare {:?} and {:?}, which differ only in case \
                     and install over each other on a case insensitive file system",
                    prev.dir, sk.dir, prev.name, sk.name
                )
            }));
        }
        seen.push((key, sk));
    }
    Ok(())
}
