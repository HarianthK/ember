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
fn str_is_what_print_shows_and_num_reads_it_back() {
    assert_eq!(out(r#"print(str(1.5) + "!")"#), ["1.5!"]);
    assert_eq!(out(r#"print(len(str([1, "a"])))"#), ["8"]);
    assert_eq!(
        out(r#"print(num("42") + 1, num(" 2.5 "), num(7))"#),
        ["43 2.5 7"]
    );
    assert!(err(r#"num("four")"#).contains(r#"num could not read "four" as a number"#));
    // "inf" parses as a float in Rust, but is not a number anyone typed on purpose.
    assert!(err(r#"num("inf")"#).contains("could not read"));
    assert!(err("num(nil)").contains("num needs a string, not a nil"));
}

#[test]
fn range_counts_up_to_but_not_including_the_end() {
    assert_eq!(out("print(range(4))"), ["[0, 1, 2, 3]"]);
    assert_eq!(out("print(range(2, 5))"), ["[2, 3, 4]"]);
    assert_eq!(out("print(range(0), range(5, 2))"), ["[] []"]);
    let src = "let total = 0
for i in range(1, 101) { total = total + i }
print(total)";
    assert_eq!(out(src), ["5050"]);
    assert!(err("range(1.5)").contains("range needs whole numbers, not 1.5"));
    assert!(err("range()").contains("range takes 1 or 2 arguments, but was given 0"));
}

#[test]
fn pop_makes_a_list_a_stack() {
    let src = "let stack = [1, 2]
push(stack, 3)
print(pop(stack), pop(stack), stack)";
    assert_eq!(out(src), ["3 2 [1]"]);
    assert!(err("pop([])").contains("pop needs a list with something in it"));
    assert!(err("pop(\"ab\")").contains("pop needs a list, not a string"));
}

#[test]
fn join_and_split_undo_each_other() {
    assert_eq!(out(r#"print(join(["a", "b", "c"], "-"))"#), ["a-b-c"]);
    assert_eq!(out(r#"print(join([1, 2, 3], ", "))"#), ["1, 2, 3"]);
    assert_eq!(
        out(r#"print(split("a,b,,c", ","))"#),
        [r#"["a", "b", "", "c"]"#]
    );
    assert_eq!(
        out(r#"print(split("héllo", ""))"#),
        [r#"["h", "é", "l", "l", "o"]"#]
    );
    assert_eq!(
        out(r#"print(join(split("one two three", " "), "_"))"#),
        ["one_two_three"]
    );
    assert!(err(r#"join("ab", "")"#).contains("join needs a list, not a string"));
    assert!(err(r#"split("ab", 1)"#).contains("split needs a string to split on, not a number"));
}

#[test]
fn floor_gives_whole_number_division() {
    assert_eq!(
        out("print(7 / 2, floor(7 / 2), floor(-7 / 2))"),
        ["3.5 3 -4"]
    );
    assert!(err("floor(\"3\")").contains("floor needs a number, not a string"));
}

#[test]
fn a_word_count_uses_the_library_together() {
    let src = r#"let text = "the cat and the hat and the bat"
let counts = {}
for word in split(text, " ") {
  if has(counts, word) { counts[word] = counts[word] + 1 } else { counts[word] = 1 }
}
let lines = []
for word in keys(counts) {
  push(lines, word + "=" + str(counts[word]))
}
print(join(lines, " "))"#;
    assert_eq!(out(src), ["and=2 bat=1 cat=1 hat=1 the=3"]);
}

#[test]
fn sort_orders_the_list_itself() {
    assert_eq!(
        out("let xs = [3, -1, 2.5, 10, 0]
sort(xs)
print(xs)"),
        ["[-1, 0, 2.5, 3, 10]"]
    );
    assert_eq!(
        out(r#"let words = ["pear", "Apple", "fig", "apple"]
sort(words)
print(words)"#),
        [r#"["Apple", "apple", "fig", "pear"]"#]
    );
    // In place, so every name for the list sees it sorted, and it returns nil like push.
    assert_eq!(
        out("let a = [2, 1]
let b = a
print(sort(a), b)"),
        ["nil [1, 2]"]
    );
    assert_eq!(
        out("let e = []
sort(e)
print(e)"),
        ["[]"]
    );
    assert!(err(r#"sort([1, "a"])"#).contains("sort needs a list of all numbers or all strings"));
    assert!(err("sort(\"cba\")").contains("sort needs a list, not a string"));
}
