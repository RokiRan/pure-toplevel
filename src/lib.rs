#[macro_use]
extern crate napi_derive;

use std::collections::HashSet;
use lazy_static::lazy_static;
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

lazy_static! {
    // 缓存的 TypeScript 帮助函数名称集合
    static ref TSLIB_HELPERS: HashSet<String> = {
        let mut helpers = HashSet::new();
        // 添加常见的 TypeScript 帮助函数
        helpers.extend([
            "__createBinding".to_string(),
            "__setModuleDefault".to_string(),
            "__importStar".to_string(),
            "__importDefault".to_string(),
        ]);
        helpers
    };
}

fn is_tslib_helper_name(name: &str) -> bool {
    // 检查是否为 TypeScript 帮助函数
    let name_parts: Vec<&str> = name.split('$').collect();
    
    // 处理特殊的帮助函数命名规则
    match name_parts.len() {
        1 => TSLIB_HELPERS.contains(name),
        2 => {
            // 检查第二部分是否为数字
            name_parts[1].parse::<i32>().is_ok() && 
            TSLIB_HELPERS.contains(name_parts[0])
        }
        _ => false
    }
}

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
}

impl PureFunctionVisitor {
    fn new(source_map: Lrc<SourceMap>, comments: Lrc<SingleThreadedComments>) -> Self {
        Self {
            in_top_level: true,
            source_map,
            comments,
        }
    }

    fn is_pure_candidate(&self, call: &CallExpr) -> bool {
        // 如果不是顶层表达式，不添加 PURE 注解
        if !self.in_top_level {
            return false;
        }

        match &call.callee {
            Callee::Expr(expr) => {
                match &**expr {
                    // 排除有参数的函数表达式
                    Expr::Arrow(arrow_expr) if !call.args.is_empty() => false,
                    
                    // 检查标识符是否为 TypeScript 帮助函数
                    Expr::Ident(ident) => {
                        !is_tslib_helper_name(&ident.sym.to_string())
                    }
                    
                    // 其他情况默认为纯函数
                    _ => true,
                }
            }
            _ => false,
        }
    }

    fn is_pure_new_expression(&self, _new_expr: &NewExpr) -> bool {
        // 检查 new 表达式是否为顶层且可以添加 PURE 注解
        self.in_top_level
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
pub fn create_plugin(node: JsObject) -> Result<bool> {
    // 解析 Node.js 传入的 AST 节点
    let node_type: String = node.get_named_property("type")?;

    match node_type.as_str() {
        "CallExpression" => {
            // 解析调用表达式
            let callee: JsObject = node.get_named_property("callee")?;
            let callee_type: String = callee.get_named_property("type")?;
            
            // 检查是否为 TypeScript 帮助函数
            if callee_type == "Identifier" {
                let name: String = callee.get_named_property("name")?;
                if is_tslib_helper_name(&name) {
                    return Ok(false);
                }
            }

            // 检查是否有参数
            let args: JsObject = node.get_named_property("arguments")?;
            let args_length: u32 = args.get_array_length()?;
            
            // 对于没有参数的函数调用，返回 true
            Ok(args_length == 0)
        },
        "NewExpression" => {
            // 对于 new 表达式，总是返回 true
            Ok(true)
        },
        _ => Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tslib_helper_detection() {
        // 测试 TypeScript 帮助函数检测
        assert!(is_tslib_helper_name("__importStar"));
        assert!(is_tslib_helper_name("__importStar$1"));
        assert!(!is_tslib_helper_name("__importStar$abc"));
        assert!(!is_tslib_helper_name("custom_function"));
    }

    #[test]
    fn test_transform_top_level_calls() -> Result<()> {
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
    fn test_transform_nested_calls() -> Result<()> {
        let test_cases = vec![
            "function test() { foo(); }",
            "class Test { method() { bar(); } }",
        ];

        for source in test_cases {
            let result = transform(source.to_string())?;
            assert!(!result.contains("/*#__PURE__*/"), "Failed for source: {}", source);
        }

        Ok(())
    }

    #[test]
    fn test_transform_tslib_helpers() -> Result<()> {
        let test_cases = vec![
            "__importStar(module);",
            "__createBinding(exports, module, 'key');",
        ];

        for source in test_cases {
            let result = transform(source.to_string())?;
            assert!(!result.contains("/*#__PURE__*/"), "Failed for source: {}", source);
        }

        Ok(())
    }

    #[test]
    fn test_create_plugin_node_js_integration() -> Result<()> {
        // 模拟 Node.js 传入的 AST 节点
        let test_cases = vec![
            // 简单的函数调用
            (r#"{"type": "CallExpression", "callee": {"type": "Identifier", "name": "foo"}, "arguments": []}"#, true),
            
            // TypeScript 帮助函数
            (r#"{"type": "CallExpression", "callee": {"type": "Identifier", "name": "__importStar"}, "arguments": []}"#, false),
            
            // 带参数的函数调用
            (r#"{"type": "CallExpression", "callee": {"type": "Identifier", "name": "bar"}, "arguments": [1]}"#, false),
            
            // new 表达式
            (r#"{"type": "NewExpression", "callee": {"type": "Identifier", "name": "MyClass"}}"#, true),
        ];

        for (input, expected) in test_cases {
            let node: JsObject = napi::bindgen_prelude::JsObject::from_str(input)?;
            let result = create_plugin(node)?;
            assert_eq!(result, expected, "Failed for input: {}", input);
        }

        Ok(())
    }
}
