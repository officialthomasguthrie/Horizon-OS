// The planner seam: intent text in, a sequence of tool calls out. This is where
// the model lives. The real one is the LLM (llama.cpp tool-calling, behind a
// feature, weights- and GPU-gated, eye-verified on hardware later); the seam is
// the same one the display backends and the FIDO2 key sit behind, so the whole
// plan-check-execute core is proven headlessly against a deterministic planner.

use crate::error::{Error, Result};
use crate::tool::{Args, Catalog};

// One step of a plan: which tool to call, with what arguments, and why. The
// rationale is the model's own explanation, surfaced in the preview so a human
// sees the reasoning before anything runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub tool: String,
    pub args: Args,
    pub rationale: String,
}

impl Step {
    pub fn new(tool: impl Into<String>, args: Args, rationale: impl Into<String>) -> Step {
        Step {
            tool: tool.into(),
            args,
            rationale: rationale.into(),
        }
    }
}

// A plan: the intent it came from and the ordered steps that carry it out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub intent: String,
    pub steps: Vec<Step>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

// Turns an intent into a plan against the available tools. The one method is all
// the executor depends on, so the LLM backend and the rule planner are
// interchangeable.
pub trait Planner {
    fn plan(&self, intent: &str, catalog: &Catalog) -> Result<Plan>;
}

// A deterministic, dependency-free planner that parses a small grammar of
// intents. It is the dev and test seam, the analog of identity's
// SoftwareAuthenticator: enough to exercise the whole executor and demo with no
// model weights, while the real LLM planner implements the same trait.
//
// Grammar (case-insensitive leading verb):
//   list <dir>                       -> list_dir
//   read <file> | show <file>        -> read_file
//   find <query> in <dir>            -> find
//   search <dir> for <query>         -> find
//   move <src> to <dst>              -> move_file
//   delete <file> | remove <file>    -> delete_file
pub struct RulePlanner;

impl Planner for RulePlanner {
    fn plan(&self, intent: &str, catalog: &Catalog) -> Result<Plan> {
        let trimmed = intent.trim();
        let (verb, rest) = match trimmed.split_once(char::is_whitespace) {
            Some((v, r)) => (v.to_lowercase(), r.trim()),
            None => (trimmed.to_lowercase(), ""),
        };

        let step = match verb.as_str() {
            "list" | "ls" => Step::new(
                "list_dir",
                Args::new().with("path", rest),
                format!("list the entries under {rest}"),
            ),
            "read" | "show" | "cat" => Step::new(
                "read_file",
                Args::new().with("path", rest),
                format!("read {rest}"),
            ),
            "find" => {
                let (query, dir) = split_keyword(rest, " in ")
                    .ok_or_else(|| Error::Plan("find <query> in <dir>".into()))?;
                Step::new(
                    "find",
                    Args::new().with("dir", dir).with("query", query),
                    format!("search {dir} for {query:?}"),
                )
            }
            "search" => {
                let (dir, query) = split_keyword(rest, " for ")
                    .ok_or_else(|| Error::Plan("search <dir> for <query>".into()))?;
                Step::new(
                    "find",
                    Args::new().with("dir", dir).with("query", query),
                    format!("search {dir} for {query:?}"),
                )
            }
            "move" | "mv" => {
                let (src, dst) = split_keyword(rest, " to ")
                    .ok_or_else(|| Error::Plan("move <src> to <dst>".into()))?;
                Step::new(
                    "move_file",
                    Args::new().with("src", src).with("dst", dst),
                    format!("move {src} to {dst}"),
                )
            }
            "delete" | "remove" | "rm" => Step::new(
                "delete_file",
                Args::new().with("path", rest),
                format!("delete {rest}"),
            ),
            other => {
                return Err(Error::Plan(format!(
                    "no rule for {other:?}; known verbs: list, read, find, search, move, delete"
                )))
            }
        };

        // The rule planner only emits tools the catalog actually carries, so a
        // trimmed catalog cannot produce an unrunnable plan.
        if catalog.get(&step.tool).is_none() {
            return Err(Error::Plan(format!("tool {} is not available", step.tool)));
        }

        Ok(Plan {
            intent: trimmed.to_string(),
            steps: vec![step],
        })
    }
}

// Split on the first occurrence of a keyword (case-insensitive), trimming both
// sides. Returns None when the keyword is absent.
fn split_keyword<'a>(s: &'a str, kw: &str) -> Option<(&'a str, &'a str)> {
    let lower = s.to_lowercase();
    let at = lower.find(&kw.to_lowercase())?;
    let left = s[..at].trim();
    let right = s[at + kw.len()..].trim();
    if left.is_empty() || right.is_empty() {
        return None;
    }
    Some((left, right))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(intent: &str) -> Result<Plan> {
        RulePlanner.plan(intent, &Catalog::standard())
    }

    #[test]
    fn list_and_read_map_to_their_tools() {
        assert_eq!(plan("list /a/b").unwrap().steps[0].tool, "list_dir");
        assert_eq!(plan("LS /a/b").unwrap().steps[0].tool, "list_dir");
        assert_eq!(plan("read /a/file").unwrap().steps[0].tool, "read_file");
        assert_eq!(plan("show /a/file").unwrap().steps[0].tool, "read_file");
    }

    #[test]
    fn find_parses_query_and_dir_both_ways() {
        let a = plan("find cows in /farm").unwrap();
        assert_eq!(a.steps[0].tool, "find");
        assert_eq!(a.steps[0].args.get("query"), Some("cows"));
        assert_eq!(a.steps[0].args.get("dir"), Some("/farm"));
        let b = plan("search /farm for cows").unwrap();
        assert_eq!(b.steps[0].args.get("query"), Some("cows"));
        assert_eq!(b.steps[0].args.get("dir"), Some("/farm"));
    }

    #[test]
    fn move_parses_src_and_dst() {
        let p = plan("move /a/x to /b/x").unwrap();
        assert_eq!(p.steps[0].tool, "move_file");
        assert_eq!(p.steps[0].args.get("src"), Some("/a/x"));
        assert_eq!(p.steps[0].args.get("dst"), Some("/b/x"));
    }

    #[test]
    fn malformed_and_unknown_intents_fail() {
        assert!(plan("find cows").is_err()); // no " in "
        assert!(plan("move /a/x").is_err()); // no " to "
        assert!(plan("frobnicate /a").is_err()); // unknown verb
    }
}
