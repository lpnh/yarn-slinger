pub use crate::compiler::compilation_job::*;
use crate::prelude::generated::yarnspinnerparser::*;
use crate::prelude::StringTableManager;
use crate::{
    output::*,
    prelude::generated::{
        yarnspinnerparser::{DialogueContext, YarnSpinnerParserContextType},
        yarnspinnerparserlistener,
    },
};
use antlr_rust::token::Token;
use std::borrow::BorrowMut;
use std::collections::HashMap;
use std::rc::Rc;

mod compilation_job;

/// Compile Yarn code, as specified by a compilation job.
pub fn compile(compilation_job: CompilationJob) -> CompilationResult {
    // TODO: other steps
    let compiler_steps: Vec<&dyn CompilerStep> = vec![&add_built_in_types, &create_string_tables];

    let initial = CompilationResult {
        program: None,
        string_table: Default::default(),
        declarations: None,
        contains_implicit_string_tags: false,
        file_tags: Default::default(),
        diagnostics: vec![],
        debug_info: Default::default(),
    };

    compiler_steps
        .into_iter()
        .fold(initial, |acc, curr| curr.apply(&compilation_job, acc))
}

pub(crate) fn get_line_id_tag<'a>(
    hashtag_contexts: &[Rc<HashtagContextAll>],
) -> Option<Rc<HashtagContextAll>> {
    hashtag_contexts
        .iter()
        .find(|h| h.text.expect("Hashtag held no text").get_text() == "line:")
        .cloned()
}

trait CompilerStep {
    fn apply(&self, job: &CompilationJob, previous: CompilationResult) -> CompilationResult;
}

impl<F> CompilerStep for F
where
    F: Fn(&CompilationJob, CompilationResult) -> CompilationResult,
{
    fn apply(&self, job: &CompilationJob, previous: CompilationResult) -> CompilationResult {
        self(job, previous)
    }
}

fn add_built_in_types(job: &CompilationJob, previous: CompilationResult) -> CompilationResult {
    previous
}

fn create_string_tables(job: &CompilationJob, previous: CompilationResult) -> CompilationResult {
    // TODO:
    // # LastLineBeforeOptionsVisitor not done

    previous
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn can_call_compile_without_crash() {
        compile(CompilationJob {
            files: vec![],
            library: None,
            compilation_type: CompilationType::FullCompilation,
            variable_declarations: vec![],
        });
    }
}
