//! Adapted from the listener part of <https://github.com/YarnSpinnerTool/YarnSpinner/blob/da39c7195107d8211f21c263e4084f773b84eaff/YarnSpinner.Compiler/Compiler.cs>

use crate::prelude::*;
use antlr_rust::parser_rule_context::ParserRuleContext;
use antlr_rust::token::Token;
use antlr_rust::tree::{ParseTreeListener, ParseTreeVisitorCompat};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use yarn_slinger_core::prelude::*;

mod emit;
use crate::parser::generated::yarnspinnerparser::{
    BodyContext, HeaderContext, NodeContext, YarnSpinnerParserContextType,
};
use crate::prelude::generated::yarnspinnerparser::BodyContextAttrs;
use crate::prelude::generated::yarnspinnerparserlistener::YarnSpinnerParserListener;
use crate::visitors::{CodeGenerationVisitor, KnownTypes};
pub(crate) use emit::*;
use yarn_slinger_core::prelude::instruction::OpCode;

pub(crate) struct CompilerListener<'input> {
    pub(crate) debug_infos: Rc<RefCell<Vec<DebugInfo>>>,
    /// The program being generated by the compiler.
    pub(crate) program: Rc<RefCell<Program>>,
    /// the list of nodes we have to ensure we track visitation
    pub(crate) tracking_nodes: Rc<RefCell<HashSet<String>>>,
    pub(crate) diagnostics: Rc<RefCell<Vec<Diagnostic>>>,
    pub(crate) types: KnownTypes,
    /// The current node to which instructions are being added.
    pub(crate) current_node: Option<Node>,
    /// The current debug information that describes [`current_node`].
    current_debug_info: DebugInfo,
    /// Whether we are currently parsing the
    /// current node as a 'raw text' node, or as a fully syntactic node.
    is_current_node_raw_text: bool,
    file: FileParseResult<'input>,
    label_count: usize,
}

impl<'input> CompilerListener<'input> {
    pub(crate) fn new(
        tracking_nodes: HashSet<String>,
        types: KnownTypes,
        file: FileParseResult<'input>,
    ) -> Self {
        Self {
            file,
            types,
            tracking_nodes: Rc::new(RefCell::new(tracking_nodes)),
            current_node: Default::default(),
            current_debug_info: Default::default(),
            is_current_node_raw_text: Default::default(),
            diagnostics: Default::default(),
            program: Default::default(),
            label_count: Default::default(),
            debug_infos: Default::default(),
        }
    }

    /// Generates a unique label name to use in the program.
    ///
    /// ## Params
    /// - `commentary` Any additional text to append to the end of the label.
    pub(crate) fn register_label<'b>(&mut self, commentary: impl Into<Option<&'b str>>) -> String {
        let commentary = commentary.into().unwrap_or_default();
        let label = format!("L{}{}", self.label_count, commentary);
        self.label_count += 1;
        label
    }
}

impl<'input> ParseTreeListener<'input, YarnSpinnerParserContextType> for CompilerListener<'input> {}

impl<'input> YarnSpinnerParserListener<'input> for CompilerListener<'input> {
    fn enter_node(&mut self, _ctx: &NodeContext<'input>) {
        // we have found a new node set up the currentNode var ready to hold it and otherwise continue
        self.current_node = Some(Node::default());
        self.current_debug_info = Default::default();
        self.is_current_node_raw_text = false;
    }

    fn exit_node(&mut self, ctx: &NodeContext<'input>) {
        let name = &self.current_node.as_ref().unwrap().name.clone();
        if name.is_empty() {
            // We don't have a name for this node. We can't emit code for it.
            self.diagnostics.borrow_mut().push(
                Diagnostic::from_message("Missing title header for node")
                    .with_file_name(self.file.name.clone())
                    .with_parser_context(ctx, self.file.tokens()),
            );
        } else {
            if !self.program.borrow().nodes.contains_key(name) {
                self.program
                    .borrow_mut()
                    .nodes
                    .insert(name.clone(), self.current_node.clone().unwrap());
            } else {
                // Duplicate node name! We'll have caught this during the
                // declarations pass, so no need to issue an error here.
            }
            self.current_debug_info.node_name = name.clone();
            self.current_debug_info.file_name = self.file.name.clone();
            self.debug_infos
                .borrow_mut()
                .push(self.current_debug_info.clone());
        }
        self.current_node = None;
        self.is_current_node_raw_text = false;
    }

    fn exit_header(&mut self, ctx: &HeaderContext<'input>) {
        // have finished with the header so about to enter the node body
        // and all its statements do the initial setup required before
        // compiling that body statements eg emit a new startlabel
        let header_key = ctx.header_key.as_ref().unwrap().get_text();
        let current_node = self.current_node.as_mut().unwrap();

        // Use the header value if provided, else fall back to the
        // empty string. This means that a header like "foo: \n" will
        // be stored as 'foo', '', consistent with how it was typed.
        // That is, it's not null, because a header was provided, but
        // it was written as an empty line.
        let header_value = ctx
            .header_value
            .as_ref()
            .map(|v| v.get_text())
            .unwrap_or_default()
            .to_owned();
        match header_key {
            "title" => {
                // Set the name of the node
                current_node.name = header_value.clone();
            }
            "tags" => {
                // Split the list of tags by spaces, and use that
                let tags = header_value.split(' ').map(|s| s.to_owned());
                current_node.tags.extend(tags);
                if current_node.tags.contains(&"rawText".to_owned()) {
                    // This is a raw text node. Flag it as such for future compilation.
                    self.is_current_node_raw_text = true;
                }
            }
            _ => {}
        }
        let header = Header {
            key: header_key.to_owned(),
            value: header_value,
        };
        current_node.headers.push(header);
    }

    fn enter_body(&mut self, ctx: &BodyContext<'input>) {
        // ok so something in here needs to be a bit different
        // also need to emit tracking code here for when we fall out of a node that needs tracking?
        // or should do I do in inside the codegenvisitor?

        // if it is a regular node
        if !self.is_current_node_raw_text {
            // This is the start of a node that we can jump to. Add a
            // label at this point
            let label = self.register_label(None);
            let current_node = self.current_node.as_mut().unwrap();
            current_node
                .labels
                .insert(label, current_node.instructions.len() as i32);
            let track = (self.tracking_nodes.borrow().contains(&current_node.name))
                .then(|| Library::generate_unique_visited_variable_for_node(&current_node.name));

            let mut visitor = CodeGenerationVisitor::new(self, track);
            for statement in ctx.statement_all() {
                visitor.visit(statement.as_ref());
            }
        } else {
            // We are a rawText node. Don't compile it; instead, note the string
            let current_node = self.current_node.as_mut().unwrap();
            current_node.source_text_string_id = get_line_id_for_node_name(&current_node.name).0;
        }
    }

    fn exit_body(&mut self, ctx: &BodyContext<'input>) {
        // this gives us the final increment at the end of the node
        // this is for when we visit and complete a node without a jump
        // theoretically this does mean that there might be redundant increments
        // but I don't think it will matter because a jump always prevents
        // the extra increment being reached
        // a bit inelegant to do it this way but the codegen visitor doesn't exit a node
        // will do for now, shouldn't be hard to refactor this later
        let track = {
            let name = &self.current_node.as_ref().unwrap().name;
            (self.tracking_nodes.borrow().contains(name))
                .then(|| Library::generate_unique_visited_variable_for_node(name))
        };
        if let Some(track) = track {
            CodeGenerationVisitor::generate_tracking_code(self, track);
        }
        // We have exited the body; emit a 'stop' opcode here.
        self.emit(Emit::from_op_code(OpCode::Stop).with_source(Position {
            line: (ctx.stop().line as usize).saturating_sub(1),
            character: 0,
        }));
    }
}
