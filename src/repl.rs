//! Line-at-a-time repl. Unchecked in v1: lines go straight to the evaluator,
//! whose faults are reported and survived. Each line's AST is leaked to get
//! the 'static lifetime the persistent interpreter needs; a repl process
//! leaks its history by design.
//! ponytail: plain stdin, no line editing; rustyline if it ever grates.

use std::io::{BufRead, Write};

use crate::ast::{Decl, Program};
use crate::builtins::render;
use crate::interp::Interp;
use crate::parser;

pub fn run() {
    let empty: &'static Program = Box::leak(Box::default());
    let mut interp = Interp::new(empty);
    interp.repl_init();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    eprintln!("mongoose repl (v1: unchecked). ctrl-d to exit.");
    loop {
        eprint!("> ");
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        eval_line(&mut interp, line);
        print!("{}", interp.take_out());
        out.flush().ok();
    }
}

fn eval_line(interp: &mut Interp<'static>, line: &str) {
    // declaration?
    let head = line.split_whitespace().next().unwrap_or("");
    if matches!(head, "fn" | "struct" | "import") {
        match parser::parse(line) {
            Ok(prog) => {
                let prog: &'static Program = Box::leak(Box::new(prog));
                for d in &prog.decls {
                    if let Err(f) = interp.repl_decl(d) {
                        eprintln!("error: {}", f.msg);
                    }
                }
            }
            Err(d) => eprintln!("error: {d}"),
        }
        return;
    }
    // statement: parse it inside a throwaway function body
    let wrapped = format!("fn __repl__() {{\n{line}\n}}\n");
    let prog = match parser::parse(&wrapped) {
        Ok(p) => p,
        Err(d) => {
            eprintln!("error: {d}");
            return;
        }
    };
    let prog: &'static Program = Box::leak(Box::new(prog));
    let Some(Decl::Fn(f)) = prog.decls.first() else {
        eprintln!("error: could not parse line");
        return;
    };
    for stmt in &f.body {
        match interp.repl_stmt(stmt) {
            Ok(Some(v)) => println!("{}", render(&v)),
            Ok(None) => {}
            Err(fault) => {
                eprintln!("error: {}", fault.msg);
                return;
            }
        }
    }
}
