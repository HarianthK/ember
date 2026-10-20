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

#[test]
fn slice_takes_part_of_a_list_or_a_string() {
    let src = "let xs = [10, 20, 30, 40, 50]
print(slice(xs, 1, 3), slice(xs, 3), slice(xs, -2), slice(xs, 0, -1))
print(xs)";
    // The original is untouched: a slice is a new list.
    assert_eq!(
        out(src),
        [
            "[20, 30] [40, 50] [40, 50] [10, 20, 30, 40]",
            "[10, 20, 30, 40, 50]"
        ]
    );
    // Trimmed to fit rather than an error, and a backwards range is just empty.
    assert_eq!(
        out("let xs = [1, 2, 3]
print(slice(xs, 0, 10), slice(xs, -10, 2), slice(xs, 2, 1))"),
        ["[1, 2, 3] [1, 2] []"]
    );
    assert_eq!(
        out(r#"print(slice("héllo", 1, 3), slice("héllo", -3))"#),
        ["él llo"]
    );
    assert!(err("slice([1], 0.5)").contains("slice needs whole numbers, not 0.5"));
    assert!(err("slice([1])").contains("slice takes 2 or 3 arguments, but was given 1"));
    assert!(err("slice(5, 0)").contains("slice needs a list or a string, not a number"));
}

#[test]
fn find_upper_and_lower_work_in_characters() {
    assert_eq!(
        out(r#"print(find("hello world", "o"), find("hello", "xyz"), find("abc", ""))"#),
        ["4 -1 0"]
    );
    // Counted in characters, not bytes: the é is one position, so the answer slices correctly.
    assert_eq!(
        out(r#"let s = "héllo wörld"
let at = find(s, "wörld")
print(at, slice(s, at))"#),
        ["6 wörld"]
    );
    assert_eq!(
        out(r#"print(upper("Straße"), lower("ÉCOLE"), upper(""))"#),
        ["STRASSE école "]
    );
    assert!(err(r#"find("a", 1)"#).contains("find needs a string to look for, not a number"));
    assert!(err("upper(nil)").contains("upper needs a string, not a nil"));
}

#[test]
fn map_filter_and_reduce_take_functions() {
    let src = "let xs = [1, 2, 3, 4, 5]
print(map(xs, fn(x) { return x * x }))
print(filter(xs, fn(x) { return x % 2 == 1 }))
print(reduce(xs, fn(total, x) { return total + x }, 0))";
    assert_eq!(out(src), ["[1, 4, 9, 16, 25]", "[1, 3, 5]", "15"]);
    // Closures carry what they captured into the prelude's loop, and the results compose.
    let src = r#"let at_least = 3
let big = filter([1, 5, 2, 8, 3], fn(x) { return x >= at_least })
print(big, reduce(map(big, str), fn(a, b) { return a + b }, ""))"#;
    assert_eq!(out(src), [r#"[5, 8, 3] 583"#]);
    // They walk whatever for walks: a string's characters, a map's keys.
    assert_eq!(
        out(r#"print(map("ab", upper), map({"y": 1, "x": 2}, upper))"#),
        [r#"["A", "B"] ["X", "Y"]"#]
    );
    // The input is never changed: map and filter build new lists.
    assert_eq!(
        out("let xs = [1, 2]
map(xs, fn(x) { return x + 1 })
print(xs)"),
        ["[1, 2]"]
    );
}

#[test]
fn a_mistake_inside_map_is_traced_through_it() {
    let e = err("let xs = [1]
map(xs, 5)");
    // The fault is inside the prelude, and the error says so, so its line is never read
    // as a line of a program that may only be two lines long.
    assert!(
        e.contains("a number cannot be called at prelude line"),
        "{e}"
    );
    assert!(
        e.contains("in map, prelude line") && e.contains("in script, line 2"),
        "{e}"
    );
}

#[test]
fn min_and_max_take_a_list_or_several_values() {
    assert_eq!(
        out("print(min([3, -1, 2]), max([3, -1, 2]), min(4, 2, 8), max(4, 2, 8))"),
        ["-1 3 2 8"]
    );
    // Strings by code point, as sort orders them; a single item is its own answer.
    assert_eq!(
        out(r#"print(min("pear", "apple"), max(["b", "a", "c"]), min([7]))"#),
        ["apple c 7"]
    );
    // The list is read, not changed.
    assert_eq!(out("let xs = [2, 1]\nmax(xs)\nprint(xs)"), ["[2, 1]"]);
    assert!(err("min([])").contains("min needs at least one value"));
    assert!(err("max()").contains("max needs at least one value"));
    assert!(err("min(5)").contains("min needs a list, or two or more values, not a number"));
    assert!(err(r#"max([1, "a"])"#).contains("max needs all numbers or all strings"));
    assert!(err("min([nil])").contains("min needs all numbers or all strings"));
}

#[test]
fn abs_drops_the_sign() {
    assert_eq!(out("print(abs(-3), abs(2.5), abs(0))"), ["3 2.5 0"]);
    assert!(err(r#"abs("-1")"#).contains("abs needs a number, not a string"));
}

#[test]
fn format_fills_each_place_with_the_next_value() {
    assert_eq!(
        out(r#"print(format("{} + {} = {}", 1, 2, 1 + 2))"#),
        ["1 + 2 = 3"]
    );
    // Each value is shown exactly as str shows it: a string bare, a list with quoted strings.
    assert_eq!(
        out(r#"print(format("{}: {} {}", "items", [1, "a"], nil))"#),
        [r#"items: [1, "a"] nil"#]
    );
    // Doubled braces are the braces themselves, and a template with no places is just itself.
    assert_eq!(
        out(r#"print(format("{{}} {{{}}}", 7), format("plain"))"#),
        ["{} {7} plain"]
    );
    assert!(err(r#"format("{} and {}", 1)"#).contains("more {} places than the 1 values"));
    assert!(err(r#"format("{}", 1, 2)"#).contains("given 2 values but has only 1 {} places"));
    assert!(err(r#"format("a { b", 1)"#).contains("lone {"));
    assert!(err(r#"format("}", 1)"#).contains("lone }"));
    assert!(err("format(5)").contains("format needs a string to fill in, not a number"));
    assert!(err("format()").contains("format needs a string to fill in"));
}

#[test]
fn sort_by_orders_by_a_key_and_keeps_ties_in_order() {
    let src = r#"let people = [{"name": "cy", "age": 30}, {"name": "al", "age": 25}, {"name": "bo", "age": 30}]
let names = fn(ps) { return map(ps, fn(p) { return p.name }) }
print(names(sort_by(people, fn(p) { return p.age })))
print(names(sort_by(sort_by(people, fn(p) { return p.name }), fn(p) { return p.age })))
print(names(people))"#;
    // Ties keep their order, so sorting by name and then by age sorts by both; the input is untouched.
    assert_eq!(
        out(src),
        [
            r#"["al", "cy", "bo"]"#,
            r#"["al", "bo", "cy"]"#,
            r#"["cy", "al", "bo"]"#
        ]
    );
    assert_eq!(
        out(
            r#"print(sort_by(["pear", "fig", "apple"], len), sort_by([3, 1, 2], fn(x) { return -x }), sort_by([], len), sort_by("cab", upper))"#
        ),
        [r#"["fig", "pear", "apple"] [3, 2, 1] [] ["a", "b", "c"]"#]
    );
    // A hundred and one numbers in a scrambled order come out exactly as the native sort puts them.
    let src = "let xs = map(range(101), fn(i) { return (i * 37) % 101 })
let ys = slice(xs, 0)
sort(ys)
print(str(sort_by(xs, fn(x) { return x })) == str(ys))";
    assert_eq!(out(src), ["true"]);
    // Keys compare as < does, so a mix of numbers and strings is the same error < gives.
    let e = err(r#"sort_by([1, "a"], fn(x) { return x })"#);
    assert!(e.contains("< needs two numbers or two strings"), "{e}");
    assert!(e.contains("in sort_by, prelude line"), "{e}");
}

#[test]
fn round_goes_to_whole_numbers_or_to_places() {
    assert_eq!(
        out("print(round(2.5), round(-2.5), round(2.4), round(7), round(-0.4))"),
        ["3 -3 2 7 0"]
    );
    assert_eq!(
        out("print(round(3.14159, 2), round(3.14159, 0), round(1234, -2), round(1250, -2))"),
        ["3.14 3 1200 1300"]
    );
    // Scaled in floating point, as JavaScript does it: 2.675 * 100 lands on 267.5 exactly.
    assert_eq!(out("print(round(2.675, 2))"), ["2.68"]);
    assert!(err("round()").contains("round takes 1 or 2 arguments, but was given 0"));
    assert!(
        err("round(1.5, 0.5)")
            .contains("round needs a whole number of places from -15 to 15, not 0.5")
    );
    assert!(err("round(1.5, 16)").contains("from -15 to 15, not 16"));
    assert!(err(r#"round("3")"#).contains("round needs a number, not a string"));
}

#[test]
fn reverse_turns_the_list_itself_around() {
    // In place, like sort: another name for the list sees it reversed, and it returns nil.
    assert_eq!(
        out("let a = [1, 2, 3]\nlet b = a\nprint(reverse(a), b)\nlet e = []\nreverse(e)\nprint(e)"),
        ["nil [3, 2, 1]", "[]"]
    );
    assert!(err(r#"reverse("abc")"#).contains("reverse needs a list, not a string"));
}

#[test]
fn find_also_looks_through_a_list() {
    // The first equal item, by ==, as in; lists inside compare as == does, by identity.
    assert_eq!(
        out(r#"let inner = [1]
print(find([5, 7, 5], 5), find(["a", "b"], "b"), find([1, 2], 3), find([], 1), find([inner], inner), find([[1]], [1]))"#),
        ["0 1 -1 -1 0 -1"]
    );
    assert!(err("find(5, 1)").contains("find needs a string or a list, not a number"));
}

#[test]
fn sum_adds_up_a_list_of_numbers() {
    assert_eq!(
        out("print(sum([1, 2, 3.5]), sum([]), sum(range(101)))"),
        ["6.5 0 5050"]
    );
    assert!(
        err(r#"sum([1, "2"])"#)
            .contains("sum needs a list of numbers, not one with a string in it")
    );
    assert!(err("sum(5)").contains("sum needs a list, not a number"));
}
