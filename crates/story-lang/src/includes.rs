//! Include splicing (language.md §1): a compilation unit is one root file
//! plus its transitive includes; the included declarations splice in at the
//! include point; an include cycle is [E231].

use crate::ast::TopItem;
use crate::lexer;
use crate::parser::Parser;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use story_content::{code, Diagnostic};

pub fn load(path: &Path, diags: &mut Vec<Diagnostic>) -> Vec<TopItem> {
    let mut stack = Vec::new();
    load_inner(path, &mut stack, diags)
}

fn load_inner(path: &Path, stack: &mut Vec<PathBuf>, diags: &mut Vec<Diagnostic>) -> Vec<TopItem> {
    let display = path.display().to_string();
    let file: Arc<str> = Arc::from(display.as_str());
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            diags.push(Diagnostic::error(
                code::E201,
                &file,
                1,
                1,
                format!("cannot read file: {e}"),
            ));
            return Vec::new();
        }
    };
    let mut lines = Vec::new();
    lines.extend(lexer::lex(&file, &src, diags));
    let items = {
        let mut p = Parser::new(file, lines, diags);
        p.parse_file()
    };

    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    stack.push(canon);
    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let mut out = Vec::new();
    for item in items {
        match item {
            TopItem::Include(inc) => {
                let child = dir.join(&inc.path.1);
                let child_canon = child.canonicalize().unwrap_or(child.clone());
                if stack.contains(&child_canon) {
                    diags.push(inc.span.error(
                        code::E231,
                        format!(
                            "include cycle: `{}` is already on the include path",
                            inc.path.1
                        ),
                    ));
                } else {
                    out.extend(load_inner(&child, stack, diags));
                }
            }
            other => out.push(other),
        }
    }
    stack.pop();
    out
}
