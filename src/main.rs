use ember::ast::print_stmts;
use ember::compiler::compile;
use ember::parser::parse;
use ember::vm::Vm;
use ember::{needs_more, repl_line};
use std::io::{BufRead, Write};
use std::process::ExitCode;

// One VM for the whole session, so what one line defines the next can use.
fn repl() -> ExitCode {
    println!("ember: type an expression or a statement; an empty line cancels an unfinished one");
    let mut vm = Vm::new();
    vm.echo = true;
    let mut pending = String::new();
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        print!("{}", if pending.is_empty() { "> " } else { ". " });
        std::io::stdout().flush().ok();
        let Some(Ok(line)) = lines.next() else {
            println!();
            return ExitCode::SUCCESS;
        };
        if !pending.is_empty() && line.trim().is_empty() {
            pending.clear();
            continue;
        }
        pending.push_str(&line);
        pending.push('\n');
        if needs_more(&pending) {
            continue;
        }
        match repl_line(&mut vm, &pending) {
            Ok(Some(shown)) => println!("{shown}"),
            Ok(None) => {}
            Err(e) => eprintln!("{e}"),
        }
        vm.output.clear();
        pending.clear();
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.iter().find(|a| !a.starts_with("--")) else {
        if args.is_empty() {
            return repl();
        }
        eprintln!("usage: ember [FILE.em [--parse | --dis]]");
        return ExitCode::FAILURE;
    };
    let src = match std::fs::read_to_string(path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("ember: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let fail = |e: &dyn std::fmt::Display| {
        eprintln!("{path}: {e}");
        ExitCode::FAILURE
    };
    let program = match parse(&src) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    if args.iter().any(|a| a == "--parse") {
        println!("{}", print_stmts(&program, 0));
        return ExitCode::SUCCESS;
    }
    let script = match compile(&program) {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };
    if args.iter().any(|a| a == "--dis") {
        print!("{}", script.chunk.disassemble());
        return ExitCode::SUCCESS;
    }
    let mut vm = Vm::new();
    vm.echo = true;
    match vm.run(script) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => fail(&e),
    }
}
