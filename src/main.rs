use ember::ast::print_stmts;
use ember::compiler::compile;
use ember::parser::parse;
use ember::vm::Vm;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.iter().find(|a| !a.starts_with("--")) else {
        eprintln!("usage: ember FILE.em [--parse | --dis]");
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
