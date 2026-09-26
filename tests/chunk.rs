use ember::chunk::{Chunk, Op, Value};
use ember::lexer::Span;

fn at(line: u32) -> Span {
    Span { line, col: 1 }
}

#[test]
fn constants_are_shared() {
    let mut chunk = Chunk::new();
    let a = chunk.constant(Value::Number(1.5));
    let b = chunk.constant(Value::Str("x".into()));
    let c = chunk.constant(Value::Number(1.5));
    assert_eq!(a, c, "the same constant should be stored once");
    assert_ne!(a, b);
    assert_eq!(chunk.constants.len(), 2);
}

#[test]
fn every_instruction_keeps_its_line() {
    let mut chunk = Chunk::new();
    let k = chunk.constant(Value::Number(2.0));
    chunk.push(Op::Constant(k), at(1));
    chunk.push(Op::Neg, at(1));
    chunk.push(Op::Return, at(3));
    assert_eq!(chunk.code.len(), chunk.spans.len());
    assert_eq!(chunk.span(2).line, 3);
}

#[test]
fn disassembly_reads_like_a_listing() {
    let mut chunk = Chunk::new();
    let k = chunk.constant(Value::Number(1.2));
    chunk.push(Op::Constant(k), at(7));
    chunk.push(Op::Neg, at(7));
    chunk.push(Op::Return, at(8));
    let text = chunk.disassemble();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "0000    7 CONSTANT        0 (1.2)");
    assert_eq!(
        lines[1], "0001    | NEG",
        "a repeated line should show as |"
    );
    assert_eq!(lines[2], "0002    8 RETURN");
}

#[test]
fn only_nil_and_false_are_false() {
    assert!(!Value::Nil.truthy());
    assert!(!Value::Bool(false).truthy());
    assert!(Value::Number(0.0).truthy());
    assert!(Value::Str(String::new()).truthy());
}
