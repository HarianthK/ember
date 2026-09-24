use ember::ast::print_stmts;
use ember::parser::parse;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: ember FILE.em");
        return ExitCode::FAILURE;
    };
    let src = match std::fs::read_to_string(path) {
        Ok(src) => src,
        Err(e) => {
            eprintln!("ember: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    match parse(&src) {
        // Until the compiler lands, running a file means printing how it was understood.
        Ok(program) => {
            println!("{}", print_stmts(&program, 0));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{path}: {e}");
            ExitCode::FAILURE
        }
    }
}
