use ember::parser::Parser;
use ember::run;

fn out(src: &str) -> Vec<String> {
    run(src).unwrap_or_else(|e| panic!("{src}: {e}"))
}

fn err(src: &str) -> String {
    match run(src) {
        Ok(output) => panic!("{src} should have failed, printed {output:?}"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn either_end_can_be_left_out_or_counted_from_the_end() {
    let xs = "let xs = [1, 2, 3, 4, 5]\n";
    assert_eq!(
        out(&format!("{xs}print(xs[1:3], xs[:2], xs[3:])")),
        ["[2, 3] [1, 2] [4, 5]"]
    );
    assert_eq!(
        out(&format!("{xs}print(xs[-2:], xs[:-1], xs[-4:-2])")),
        ["[4, 5] [1, 2, 3, 4] [2, 3]"]
    );
    // Past either end is cut to fit, and an end before the start is empty, as slice() does.
    assert_eq!(
        out(&format!("{xs}print(xs[0:99], xs[-99:1], xs[4:1], xs[5:])")),
        ["[1, 2, 3, 4, 5] [1] [] []"]
    );
    assert_eq!(
        out(&format!("{xs}let i = 1\nprint(xs[i + 1:len(xs) - i])")),
        ["[3, 4]"]
    );
}

#[test]
fn a_whole_slice_is_a_copy() {
    assert_eq!(
        out("let xs = [1, 2]\nlet ys = xs[:]\npush(ys, 3)\nprint(xs, ys)"),
        ["[1, 2] [1, 2, 3]"]
    );
}

#[test]
fn strings_slice_by_character() {
    assert_eq!(
        out(r#"print("héllo"[1:3], "héllo"[-3:], "abc"[:])"#),
        ["él llo abc"]
    );
}

#[test]
fn the_ends_are_worked_out_once_left_to_right() {
    let src = "fn at(n) { print(n)\nreturn n }\nprint([7, 8, 9][at(1):at(2)])";
    assert_eq!(out(src), ["1", "2", "[8]"]);
}

// A list nothing names, of lists: under EMBER_STRESS_GC=1 the items must survive the
// collection after the old list is dropped, held now only by the slice.
#[test]
fn a_slice_of_a_list_nothing_else_holds() {
    let src =
        "fn make() { return [[1], [2, 3], {\"k\": [4]}] }\nlet ys = make()[1:]\nprint(ys, ys[1].k)";
    assert_eq!(out(src), [r#"[[2, 3], {"k": [4]}] [4]"#]);
}

#[test]
fn slices_print_back_as_written() {
    for src in ["xs[1:2]", "xs[:n]", "xs[1:]", "xs[:]", "f(x)[a + 1:b][0]"] {
        let e = Parser::new(src)
            .expect("lexes")
            .expression()
            .expect("parses");
        assert_eq!(ember::ast::print_expr(&e), src.replace("a + 1", "(a + 1)"));
    }
}

#[test]
fn what_cannot_be_sliced_says_so() {
    assert!(err(r#"print({"a": 1}[0:1])"#).contains("slice needs a list or a string, not a map"));
    assert!(err("print([1, 2][0.5:])").contains("slice needs whole numbers, not 0.5"));
    assert!(err("let xs = [1]\nxs[0:1] = 2").contains("cannot assign"));
    assert!(err("print([1][1:2:3])").contains("line 1"));
}
