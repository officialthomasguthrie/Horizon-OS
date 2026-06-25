// The tool layer: the typed, permissioned actions Aura is allowed to take. Each
// tool maps an intent step to one or more Weave capabilities (a `Need`), so the
// only way Aura touches a file is through a resource the broker checked and
// logged. This is the "capabilities, not pixels" rule from docs/05: a tool
// either works or fails cleanly, is individually scoped, and is auditable.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use weave::{Lease, Resource, Rights};

use crate::error::{Error, Result};

// How much of a file `read_file` returns and how far `find` descends, so a tool
// on a huge tree stays bounded.
const READ_CAP: usize = 64 * 1024;
const FIND_DEPTH: usize = 8;

// Effect classes drive the safety rails. A Read tool runs once its capability is
// held; a Write tool mutates (reversible through a Lifestream snapshot); a
// Destructive tool also needs explicit confirmation before the executor runs it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Effect {
    Read,
    Write,
    Destructive,
}

impl Effect {
    pub fn label(self) -> &'static str {
        match self {
            Effect::Read => "read",
            Effect::Write => "write",
            Effect::Destructive => "destructive",
        }
    }
    pub fn is_destructive(self) -> bool {
        matches!(self, Effect::Destructive)
    }
}

// One capability a tool invocation needs: a resource and the rights over it. The
// executor brokers each Need (weave `access`) before the tool runs, so a tool
// never acts on authority the broker did not check and record.
#[derive(Clone, Debug)]
pub struct Need {
    pub resource: Resource,
    pub rights: Rights,
}

impl Need {
    pub fn new(resource: Resource, rights: Rights) -> Need {
        Need { resource, rights }
    }
}

// One declared parameter of a tool, the schema a planner (or the LLM later) sees
// when it decides how to call the tool.
pub struct ParamSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub required: bool,
}

// The string-keyed arguments of a tool call. Paths and queries are all the
// built-in tools need; a richer typed schema is the planner's concern, not the
// executor's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args(BTreeMap<String, String>);

impl Args {
    pub fn new() -> Args {
        Args(BTreeMap::new())
    }
    pub fn with(mut self, k: impl Into<String>, v: impl Into<String>) -> Args {
        self.0.insert(k.into(), v.into());
        self
    }
    pub fn set(&mut self, k: impl Into<String>, v: impl Into<String>) {
        self.0.insert(k.into(), v.into());
    }
    pub fn get(&self, k: &str) -> Option<&str> {
        self.0.get(k).map(|s| s.as_str())
    }
    pub fn require(&self, k: &'static str) -> Result<&str> {
        self.get(k).ok_or(Error::MissingArg(k))
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

// What a tool produced. Kept as data the caller renders, not printed text, so a
// CLI, the Glass surface, or a test all read the same outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Listing(Vec<String>),
    Text(String),
    Matches(Vec<String>),
    Moved { from: String, to: String },
    Deleted(String),
}

impl Outcome {
    pub fn summary(&self) -> String {
        match self {
            Outcome::Listing(es) => format!("{} entries", es.len()),
            Outcome::Text(t) => format!("{} bytes", t.len()),
            Outcome::Matches(ms) => format!("{} matches", ms.len()),
            Outcome::Moved { from, to } => format!("moved {from} -> {to}"),
            Outcome::Deleted(p) => format!("deleted {p}"),
        }
    }
}

// A typed, permissioned action. The executor calls `required` to learn what
// capabilities the invocation needs, brokers them, and only then calls `run`
// with the leases that prove each was authorized.
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn params(&self) -> &'static [ParamSpec];
    fn effect(&self) -> Effect;
    // The capabilities this specific invocation needs. An argument error here is
    // surfaced as a blocked step, not a panic.
    fn required(&self, args: &Args) -> Result<Vec<Need>>;
    // Execute, given the leases proving each Need was brokered. The built-ins
    // re-open by path (the userland approximation); a tool over a real broker fd
    // would act through the lease, exactly as weave's Lease comment describes.
    fn run(&self, args: &Args, leases: &[Lease]) -> Result<Outcome>;
}

// Resolve a path argument to an absolute path without requiring it to exist, so
// a File resource is deterministic and `covers` compares like-for-like. (move
// and delete name paths that may or may not be there yet.)
fn path_arg(args: &Args, key: &'static str) -> Result<PathBuf> {
    let raw = args.require(key)?;
    if raw.is_empty() {
        return Err(Error::BadArg(key, "empty path".into()));
    }
    std::path::absolute(raw).map_err(|e| Error::BadArg(key, e.to_string()))
}

// The registry of tools Aura may call. The standard set is read-mostly file
// tools plus the two destructive ones; richer tools (net, device, service) are
// added the same way as the system grows.
pub struct Catalog {
    tools: Vec<Box<dyn Tool>>,
}

impl Catalog {
    pub fn empty() -> Catalog {
        Catalog { tools: Vec::new() }
    }

    pub fn standard() -> Catalog {
        let mut c = Catalog::empty();
        c.add(Box::new(ListDir));
        c.add(Box::new(ReadFile));
        c.add(Box::new(Find));
        c.add(Box::new(MoveFile));
        c.add(Box::new(DeleteFile));
        c
    }

    pub fn add(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|t| t.as_ref())
    }

    pub fn tools(&self) -> impl Iterator<Item = &dyn Tool> {
        self.tools.iter().map(|t| t.as_ref())
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    // The catalog the planner (and the LLM later) reads: one line per tool with
    // its effect, parameters, and description.
    pub fn describe(&self) -> String {
        let mut s = String::new();
        for t in self.tools() {
            let ps: Vec<String> = t
                .params()
                .iter()
                .map(|p| {
                    if p.required {
                        format!("<{}>", p.name)
                    } else {
                        format!("[{}]", p.name)
                    }
                })
                .collect();
            s.push_str(&format!(
                "{} {} ({}): {}\n",
                t.name(),
                ps.join(" "),
                t.effect().label(),
                t.description()
            ));
        }
        s
    }
}

impl Default for Catalog {
    fn default() -> Catalog {
        Catalog::standard()
    }
}

// list_dir: the names directly under a directory.
struct ListDir;
impl Tool for ListDir {
    fn name(&self) -> &'static str {
        "list_dir"
    }
    fn description(&self) -> &'static str {
        "list the entries directly under a directory"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[ParamSpec {
            name: "path",
            description: "the directory to list",
            required: true,
        }]
    }
    fn effect(&self) -> Effect {
        Effect::Read
    }
    fn required(&self, args: &Args) -> Result<Vec<Need>> {
        Ok(vec![Need::new(
            Resource::file(path_arg(args, "path")?),
            Rights::READ,
        )])
    }
    fn run(&self, args: &Args, _leases: &[Lease]) -> Result<Outcome> {
        let path = path_arg(args, "path")?;
        let mut names = Vec::new();
        for e in fs::read_dir(&path).map_err(Error::io)? {
            let e = e.map_err(Error::io)?;
            names.push(e.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        Ok(Outcome::Listing(names))
    }
}

// read_file: the contents of a file, capped so a huge file stays bounded.
struct ReadFile;
impl Tool for ReadFile {
    fn name(&self) -> &'static str {
        "read_file"
    }
    fn description(&self) -> &'static str {
        "read the text contents of a file"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[ParamSpec {
            name: "path",
            description: "the file to read",
            required: true,
        }]
    }
    fn effect(&self) -> Effect {
        Effect::Read
    }
    fn required(&self, args: &Args) -> Result<Vec<Need>> {
        Ok(vec![Need::new(
            Resource::file(path_arg(args, "path")?),
            Rights::READ,
        )])
    }
    fn run(&self, args: &Args, _leases: &[Lease]) -> Result<Outcome> {
        let path = path_arg(args, "path")?;
        let bytes = fs::read(&path).map_err(Error::io)?;
        let take = bytes.len().min(READ_CAP);
        let text = String::from_utf8_lossy(&bytes[..take]).into_owned();
        Ok(Outcome::Text(text))
    }
}

// find: files under a directory whose name or text contents match a query, the
// lexical stand-in for the semantic search the embedding index brings later.
struct Find;
impl Tool for Find {
    fn name(&self) -> &'static str {
        "find"
    }
    fn description(&self) -> &'static str {
        "find files under a directory by name or contents (lexical)"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[
            ParamSpec {
                name: "dir",
                description: "the directory to search",
                required: true,
            },
            ParamSpec {
                name: "query",
                description: "the text to match",
                required: true,
            },
        ]
    }
    fn effect(&self) -> Effect {
        Effect::Read
    }
    fn required(&self, args: &Args) -> Result<Vec<Need>> {
        Ok(vec![Need::new(
            Resource::file(path_arg(args, "dir")?),
            Rights::READ,
        )])
    }
    fn run(&self, args: &Args, _leases: &[Lease]) -> Result<Outcome> {
        let dir = path_arg(args, "dir")?;
        let query = args.require("query")?.to_lowercase();
        let mut hits = Vec::new();
        walk(&dir, FIND_DEPTH, &mut |p: &PathBuf| {
            let name_match = p
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase().contains(&query))
                .unwrap_or(false);
            let body_match = !name_match
                && fs::read(p)
                    .ok()
                    .map(|b| {
                        let take = b.len().min(READ_CAP);
                        String::from_utf8_lossy(&b[..take])
                            .to_lowercase()
                            .contains(&query)
                    })
                    .unwrap_or(false);
            if name_match || body_match {
                hits.push(p.to_string_lossy().into_owned());
            }
        })
        .map_err(Error::io)?;
        hits.sort();
        Ok(Outcome::Matches(hits))
    }
}

fn walk(dir: &PathBuf, depth: usize, f: &mut impl FnMut(&PathBuf)) -> std::io::Result<()> {
    if depth == 0 {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            walk(&p, depth - 1, f)?;
        } else {
            f(&p);
        }
    }
    Ok(())
}

// move_file: rename a file. Destructive, and it needs two capabilities: read and
// write at the source, write at the destination, so a "move" cannot be smuggled
// past a grant that only covers the source.
struct MoveFile;
impl Tool for MoveFile {
    fn name(&self) -> &'static str {
        "move_file"
    }
    fn description(&self) -> &'static str {
        "move a file from one path to another"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[
            ParamSpec {
                name: "src",
                description: "the file to move",
                required: true,
            },
            ParamSpec {
                name: "dst",
                description: "where to move it",
                required: true,
            },
        ]
    }
    fn effect(&self) -> Effect {
        Effect::Destructive
    }
    fn required(&self, args: &Args) -> Result<Vec<Need>> {
        Ok(vec![
            Need::new(
                Resource::file(path_arg(args, "src")?),
                Rights::READ | Rights::WRITE,
            ),
            Need::new(Resource::file(path_arg(args, "dst")?), Rights::WRITE),
        ])
    }
    fn run(&self, args: &Args, _leases: &[Lease]) -> Result<Outcome> {
        let src = path_arg(args, "src")?;
        let dst = path_arg(args, "dst")?;
        fs::rename(&src, &dst).map_err(Error::io)?;
        Ok(Outcome::Moved {
            from: src.to_string_lossy().into_owned(),
            to: dst.to_string_lossy().into_owned(),
        })
    }
}

// delete_file: remove a file. Destructive, so the executor will not run it
// without explicit confirmation even once the capability is held.
struct DeleteFile;
impl Tool for DeleteFile {
    fn name(&self) -> &'static str {
        "delete_file"
    }
    fn description(&self) -> &'static str {
        "delete a file"
    }
    fn params(&self) -> &'static [ParamSpec] {
        &[ParamSpec {
            name: "path",
            description: "the file to delete",
            required: true,
        }]
    }
    fn effect(&self) -> Effect {
        Effect::Destructive
    }
    fn required(&self, args: &Args) -> Result<Vec<Need>> {
        Ok(vec![Need::new(
            Resource::file(path_arg(args, "path")?),
            Rights::WRITE,
        )])
    }
    fn run(&self, args: &Args, _leases: &[Lease]) -> Result<Outcome> {
        let path = path_arg(args, "path")?;
        fs::remove_file(&path).map_err(Error::io)?;
        Ok(Outcome::Deleted(path.to_string_lossy().into_owned()))
    }
}
