#![no_main]

use libfuzzer_sys::fuzz_target;
use qiyas_param::Expr;

fuzz_target!(|text: &str| {
    if text.len() > 16 * 1024 {
        return;
    }
    if let Ok(expr) = Expr::parse(text) {
        let _ = expr.names();
        let _ = expr.eval(&|_| Some(1.0));
        let _ = expr.eval(&|_| None);
        let _ = Expr::parse(&expr.to_string());
    }
});
