use ember::ast::{Expr, Stmt, print_expr, print_stmts};
use ember::lexer::{Tok, tokenize};
use ember::parser::{Parser, parse};

fn expr(src: &str) -> String {
    let mut parser = Parser::new(src).expect("lexes");
    let e = parser.expression().expect("parses");
    print_expr(&e)
}

fn kinds(src: &str) -> Vec<Tok> {
    tokenize(src)
        .expect("lexes")
        .into_iter()
        .map(|t| t.tok)
        .collect()
}

#[test]
fn lexes_the_awkward_bits() {
    assert_eq!(
        kinds("a==b"),
        vec![
            Tok::Name("a".into()),
            Tok::Eq,
            Tok::Name("b".into()),
            Tok::Eof
        ]
    );
    // A two-character operator must not be read as two one-character ones.
    assert_eq!(
        kinds("a=b"),
        vec![
            Tok::Name("a".into()),
            Tok::Assign,
            Tok::Name("b".into()),
            Tok::Eof
        ]
    );
    assert_eq!(
        kinds("x<=1"),
        vec![
            Tok::Name("x".into()),
            Tok::LessEq,
            Tok::Number(1.0),
            Tok::Eof
        ]
    );
    assert_eq!(kinds("1.5"), vec![Tok::Number(1.5), Tok::Eof]);
    // A dot that is not part of a number is a field access.
    assert_eq!(
        kinds("a.b"),
        vec![
            Tok::Name("a".into()),
            Tok::Dot,
            Tok::Name("b".into()),
            Tok::Eof
        ]
    );
    assert_eq!(kinds("// gone\n1"), vec![Tok::Number(1.0), Tok::Eof]);
    assert_eq!(kinds(r#""a\nb""#), vec![Tok::Str("a\nb".into()), Tok::Eof]);
    // Keywords are only keywords on their own.
    assert_eq!(kinds("iffy"), vec![Tok::Name("iffy".into()), Tok::Eof]);
}

#[test]
fn lexer_says_where_it_gave_up() {
    let err = tokenize("let x = 1\nlet y = @").unwrap_err();
    assert_eq!(err.at.line, 2);
    assert_eq!(err.at.col, 9);
    assert!(err.message.contains('@'), "{}", err.message);
    assert!(
        tokenize("\"never closed")
            .unwrap_err()
            .message
            .contains("never closed")
    );
}

#[test]
fn precedence_is_what_arithmetic_says() {
    assert_eq!(expr("1 + 2 * 3"), "(1 + (2 * 3))");
    assert_eq!(expr("1 * 2 + 3"), "((1 * 2) + 3)");
    assert_eq!(expr("(1 + 2) * 3"), "((1 + 2) * 3)");
    // Left associative, so subtraction does not silently become right associative.
    assert_eq!(expr("10 - 3 - 2"), "((10 - 3) - 2)");
    assert_eq!(expr("2 + 3 < 4 + 5"), "((2 + 3) < (4 + 5))");
    assert_eq!(expr("a or b and c"), "(a or (b and c))");
    assert_eq!(expr("not a == b"), "((not a) == b)");
    assert_eq!(expr("-x * 2"), "((-x) * 2)");
}

#[test]
fn assignment_is_right_associative_and_checked() {
    assert_eq!(expr("a = b = 1"), "(a = (b = 1))");
    assert_eq!(expr("xs[0] = 1"), "(xs[0] = 1)");
    assert_eq!(expr("p.x = 1"), "(p.x = 1)");
    let err = Parser::new("1 = 2").unwrap().expression().unwrap_err();
    assert!(err.message.contains("cannot assign"), "{}", err.message);
}

#[test]
fn calls_indexes_and_fields_chain() {
    assert_eq!(expr("f(1)(2)"), "f(1)(2)");
    assert_eq!(expr("xs[0].name"), "xs[0].name");
    assert_eq!(expr("f(a, b)[1]"), "f(a, b)[1]");
    assert_eq!(expr("[1, 2 + 3]"), "[1, (2 + 3)]");
    assert_eq!(expr(r#"{"a": 1}"#), "{\"a\": 1}");
}

#[test]
fn statements_parse() {
    let program = parse("let x = 1; x = x + 1; return x;").unwrap();
    assert_eq!(program.len(), 3);
    assert!(matches!(program[0], Stmt::Let { .. }));
    assert!(matches!(program[2], Stmt::Return { value: Some(_), .. }));

    let program = parse("if a { b(); } else if c { d(); }").unwrap();
    match &program[0] {
        Stmt::If { otherwise, .. } => {
            assert_eq!(otherwise.len(), 1);
            assert!(
                matches!(otherwise[0], Stmt::If { .. }),
                "else if should nest an if"
            );
        }
        other => panic!("expected an if, got {other:?}"),
    }

    // A named function is sugar for a let, so it can be used like any other value.
    match &parse("fn add(a, b) { return a + b; }").unwrap()[0] {
        Stmt::Let {
            name,
            value: Expr::Func { params, .. },
            ..
        } => {
            assert_eq!(name, "add");
            assert_eq!(params, &["a".to_string(), "b".to_string()]);
        }
        other => panic!("expected a let holding a function, got {other:?}"),
    }
}

#[test]
fn semicolons_are_optional_between_statements() {
    let with = parse("let x = 1;\nlet y = 2;").unwrap();
    let without = parse("let x = 1\nlet y = 2").unwrap();
    assert_eq!(with, without);
}

#[test]
fn errors_point_at_the_problem() {
    let err = parse("let x = ;").unwrap_err();
    assert!(
        err.message.contains("cannot start an expression"),
        "{}",
        err.message
    );
    assert_eq!(err.at.line, 1);

    let err = parse("fn f( {").unwrap_err();
    assert!(err.message.contains("expected a name"), "{}", err.message);

    let err = parse("if x { y();").unwrap_err();
    assert!(err.message.contains("never closed"), "{}", err.message);

    let err = parse("let 1 = 2").unwrap_err();
    assert!(err.message.contains("expected a name"), "{}", err.message);
}

// Printing a parse and parsing it back must print the same thing. Comparing the
// printed form, not the tree, because the reprinted source sits on different lines
// and the spans rightly differ. This catches a printer that loses grouping and a
// parser that reads its own output differently.
#[test]
fn printing_and_reparsing_is_stable() {
    let sources = [
        "let x = 1 + 2 * 3 - 4;",
        "fn fib(n) { if n < 2 { return n; } return fib(n - 1) + fib(n - 2); }",
        "let xs = [1, 2, 3]; for v in xs { total = total + v; }",
        "while not done and i < 10 { i = i + 1; }",
        r#"let m = {"a": 1, "b": [true, nil, -2.5]}; m.a = m["b"][0];"#,
        "let add = fn(a, b) { return a + b; }; add(1, 2);",
    ];
    for src in sources {
        let first = parse(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        let printed = print_stmts(&first, 0);
        let second = parse(&printed).unwrap_or_else(|e| panic!("reparsing {printed}: {e}"));
        assert_eq!(
            printed,
            print_stmts(&second, 0),
            "round trip changed the meaning of {src}"
        );
    }
}

// A function literal inside another used to print its body at the outer level.
#[test]
fn nested_functions_are_indented() {
    let program =
        parse("let counter = fn(start) { let n = start; return fn() { return n; }; };").unwrap();
    let printed = print_stmts(&program, 0);
    assert!(
        printed.contains("    return n;"),
        "inner body is not indented:\n{printed}"
    );
    assert_eq!(printed, print_stmts(&parse(&printed).unwrap(), 0));
}

// The lexer reads bytes. It used to turn each byte of a string into a character of its
// own, so é (two bytes) came out as "Ã©", and it counted columns in bytes too.
#[test]
fn non_ascii_text_survives_lexing() {
    assert_eq!(kinds("\"héllo\""), vec![Tok::Str("héllo".into()), Tok::Eof]);
    assert_eq!(kinds("\"日本\""), vec![Tok::Str("日本".into()), Tok::Eof]);
    assert_eq!(kinds("\"é\n\""), vec![Tok::Str("é\n".into()), Tok::Eof]);
    // Columns count characters: the @ is the fifth character on the line.
    let e = tokenize("\"é\" @").unwrap_err();
    assert_eq!(e.at.col, 5, "{e}");
}
