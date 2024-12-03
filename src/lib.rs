#[macro_use]
extern crate napi_derive;

use std::collections::HashSet;
use napi::{
    bindgen_prelude::*,
    Env, JsObject, Result, CallContext,
};
use serde::{Deserialize, Serialize};
use swc_core::common::{
    comments::{Comment, CommentKind, Comments, SingleThreadedComments},
    sync::Lrc,
    FileName, SourceMap, SyntaxContext, Span,
};
use swc_core::ecma::{
    ast::*,
    codegen::{text_writer::JsWriter, Config, Emitter},
    parser::{lexer::Lexer, Parser, StringInput, Syntax},
    visit::{VisitMut, VisitMutWith},
};

#[derive(Debug, Serialize, Deserialize)]
struct BabelNode {
    #[serde(rename = "type")]
    node_type: String,
    // 其他字段根据需要添加
}

#[derive(Default)]
struct PureFunctionVisitor {
    in_top_level: bool,
    source_map: Lrc<SourceMap>,
    comments: Lrc<SingleThreadedComments>,
    pure_candidates: HashSet<String>,
}

impl PureFunctionVisitor {
    fn new(source_map: Lrc<SourceMap>, comments: Lrc<SingleThreadedComments>) -> Self {
        let mut pure_candidates = HashSet::new();
        // 预定义一些常见的纯函数
        pure_candidates.extend([
            "Object.create".to_string(),
            "Math.abs".to_string(),
            "Math.round".to_string(),
            "Math.floor".to_string(),
            "Math.ceil".to_string(),
            "Number".to_string(),
            "String".to_string(),
            "Boolean".to_string(),
        ]);

        Self {
            in_top_level: true,
            source_map,
            comments,
            pure_candidates,
        }
    }

    fn is_pure_candidate(&self, call: &CallExpr) -> bool {
        if !self.in_top_level {
            return false;
        }

        match &call.callee {
            Callee::Expr(expr) => {
                match &**expr {
                    Expr::Ident(_) => true, // 简单标识符
                    Expr::Member(member) => {
                        // 检查成员调用是否在预定义的纯函数列表中
                        match (&*member.obj, &member.prop) {
                            (Expr::Ident(obj), MemberProp::Ident(prop)) => {
                                let full_name = format!("{}.{}", obj.sym, prop.sym);
                                self.pure_candidates.contains(&full_name)
                            }
                            _ => false
                        }
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn has_pure_comment(&self, span: Span) -> bool {
        self.comments.with_leading(span.lo, |comments| {
            comments.iter().any(|comment| {
                comment.text.contains("/*#__PURE__*/") || 
                comment.text.contains("@__PURE__")
            })
        })
    }

    fn add_pure_comment(&self, call: &mut CallExpr) {
        let new_span = Span::new(
            call.span.lo,
            call.span.lo,
            SyntaxContext::empty(),
        );
        
        Comments::add_leading(
            &self.comments,
            new_span.lo,
            Comment {
                kind: CommentKind::Block,
                span: new_span,
                text: "#__PURE__".into(),
            },
        );
    }
}

struct PureAnnotator {
    visitor: PureFunctionVisitor,
}

impl VisitMut for PureAnnotator {
    fn visit_mut_call_expr(&mut self, call: &mut CallExpr) {
        if self.visitor.is_pure_candidate(call) && !self.visitor.has_pure_comment(call.span) {
            self.visitor.add_pure_comment(call);
        }
        call.visit_mut_children_with(self);
    }

    fn visit_mut_function(&mut self, n: &mut swc_core::ecma::ast::Function) {
        let old_top_level = self.visitor.in_top_level;
        self.visitor.in_top_level = false;
        n.visit_mut_children_with(self);
        self.visitor.in_top_level = old_top_level;
    }

    fn visit_mut_arrow_expr(&mut self, n: &mut ArrowExpr) {
        let old_top_level = self.visitor.in_top_level;
        self.visitor.in_top_level = false;
        n.visit_mut_children_with(self);
        self.visitor.in_top_level = old_top_level;
    }
}

fn parse_js(source: &str) -> Result<(Module, Lrc<SourceMap>, Lrc<SingleThreadedComments>)> {
    let source_map = Lrc::new(SourceMap::default());
    let comments = Lrc::new(SingleThreadedComments::default());
    
    let source_file = source_map.new_source_file(
        FileName::Anon,
        source.into(),
    );

    let lexer = Lexer::new(
        Syntax::Es(Default::default()),
        Default::default(),
        StringInput::from(&*source_file),
        Some(&comments),
    );

    let mut parser = Parser::new_from(lexer);
    let module = parser.parse_module().map_err(|e| {
        Error::from_reason(format!("Failed to parse JavaScript: {:?}", e))
    })?;
    
    Ok((module, source_map, comments))
}

fn generate_js(module: &Module, source_map: Lrc<SourceMap>, comments: Lrc<SingleThreadedComments>) -> Result<String> {
    let mut buf = vec![];
    let writer = JsWriter::new(source_map.clone(), "\n", &mut buf, None);
    let config = Config::default();
    let mut emitter = Emitter {
        cfg: config,
        comments: Some(&comments),
        cm: source_map,
        wr: writer,
    };

    emitter.emit_module(&module).map_err(|e| {
        Error::from_reason(format!("Failed to generate JavaScript: {:?}", e))
    })?;

    String::from_utf8(buf).map_err(|e| {
        Error::from_reason(format!("Failed to convert generated code to string: {}", e))
    })
}

#[napi]
pub fn transform(source: String) -> Result<String> {
    // 解析 JavaScript 代码
    let (mut module, source_map, comments) = parse_js(&source)?;
    
    // 创建并运行访问器
    let visitor = PureFunctionVisitor::new(source_map.clone(), comments.clone());
    let mut annotator = PureAnnotator { visitor };
    module.visit_mut_with(&mut annotator);
    
    // 生成修改后的代码
    generate_js(&module, source_map, comments)
}

#[napi]
pub fn create_plugin() -> () {
    // 空实现
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_simple_call() -> Result<()> {
        let source = "foo();";
        let result = transform(source.to_string())?;
        assert!(result.contains("/*#__PURE__*/"));
        Ok(())
    }

    #[test]
    fn test_transform_nested_call() -> Result<()> {
        let source = "function test() { foo(); }";
        let result = transform(source.to_string())?;
        assert!(!result.contains("/*#__PURE__*/"));
        Ok(())
    }

    #[test]
    fn test_transform_member_call() -> Result<()> {
        let source = "console.log();";
        let result = transform(source.to_string())?;
        assert!(!result.contains("/*#__PURE__*/"));
        Ok(())
    }

    #[test]
    fn test_transform_predefined_pure_functions() -> Result<()> {
        let test_cases = vec![
            "Object.create({});",
            "Math.abs(-5);",
            "Number(42);",
            "String(123);",
        ];

        for source in test_cases {
            let result = transform(source.to_string())?;
            assert!(result.contains("/*#__PURE__*/"), "Failed for source: {}", source);
        }

        Ok(())
    }

    #[test]
    fn test_transform_impure_functions() -> Result<()> {
        let test_cases = vec![
            "console.log(1);",
            "window.alert('test');",
            "document.createElement('div');",
        ];

        for source in test_cases {
            let result = transform(source.to_string())?;
            assert!(!result.contains("/*#__PURE__*/"), "Failed for source: {}", source);
        }

        Ok(())
    }
}
