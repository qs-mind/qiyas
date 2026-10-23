//! Dimensional constraints' expressions: numbers, + - * /, parentheses, names of other
//! parameters, sin/cos/tan (degrees), sqrt, abs and pi; a name depending on itself, through
//! others or straight, is refused.

use std::collections::BTreeMap;

use qiyas_param::{evaluate_all, Expr, ExprError};

fn params(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn numbers_operators_and_parentheses_follow_the_usual_order() {
    let none = |_: &str| None;
    let eval = |t: &str| Expr::parse(t).unwrap().eval(&none).unwrap();
    assert_eq!(eval("1.5"), 1.5);
    assert_eq!(eval("2 + 3 * 4"), 14.0);
    assert_eq!(eval("(2 + 3) * 4"), 20.0);
    assert_eq!(eval("10 / 4 - 1"), 1.5);
    assert_eq!(eval("-2 * -(3)"), 6.0);
    assert_eq!(eval("8 - 2 - 1"), 5.0);
    assert!((eval("sin(30)") - 0.5).abs() < 1e-12);
    assert!((eval("cos(60) + sqrt(16) + abs(-1)") - 5.5).abs() < 1e-12);
    assert!((eval("2*pi") - std::f64::consts::TAU).abs() < 1e-12);
    assert_eq!(eval("1e3"), 1000.0);
}

#[test]
fn bad_expressions_say_what_is_wrong() {
    for bad in ["", "2 +", "(1", "1)", "2 3", "foo(1)", "1 $ 2"] {
        assert!(
            matches!(Expr::parse(bad), Err(ExprError::Syntax(_))),
            "{bad:?} parsed"
        );
    }
    let none = |_: &str| None;
    assert!(matches!(
        Expr::parse("1/0").unwrap().eval(&none),
        Err(ExprError::NotANumber(_))
    ));
    assert_eq!(
        Expr::parse("2*d2 + d3").unwrap().names(),
        vec!["d2".to_string(), "d3".to_string()]
    );
}

#[test]
fn a_parameter_follows_the_ones_it_uses() {
    let values = evaluate_all(&params(&[("d1", "2*d2 + 1.5"), ("d2", "3")])).unwrap();
    assert_eq!(values["d1"], 7.5);
    let values = evaluate_all(&params(&[("d1", "2*d2 + 1.5"), ("d2", "5")])).unwrap();
    assert_eq!(values["d1"], 11.5);
    // A chain, in any order of names.
    let values = evaluate_all(&params(&[
        ("a", "b + 1"),
        ("b", "c * 2"),
        ("c", "d3"),
        ("d3", "4"),
    ]))
    .unwrap();
    assert_eq!(values["a"], 9.0);
}

#[test]
fn circular_and_unknown_references_are_refused() {
    let err = evaluate_all(&params(&[("d1", "d2 + 1"), ("d2", "d1 * 2")])).unwrap_err();
    assert!(
        matches!(err, ExprError::Circular(ref names) if names.contains(&"d1".to_string())),
        "{err:?}"
    );
    let err = evaluate_all(&params(&[("d1", "d1 + 1")])).unwrap_err();
    assert!(matches!(err, ExprError::Circular(_)), "{err:?}");
    let err = evaluate_all(&params(&[("d1", "d9 + 1")])).unwrap_err();
    assert_eq!(err, ExprError::Unknown("d9".into()));
}

#[test]
fn each_parameter_is_worked_out_on_its_own_so_one_broken_one_spoils_only_its_users() {
    use qiyas_param::evaluate_each;
    let values = evaluate_each(&params(&[
        ("d1", "2*d2"),
        ("d2", "d9 + 1"),
        ("d3", "4"),
        ("d4", "foo(1)"),
        ("a", "b"),
        ("b", "a"),
        ("d5", "d3 / 2"),
    ]));
    assert_eq!(values["d3"], Ok(4.0));
    assert_eq!(values["d5"], Ok(2.0));
    assert_eq!(values["d2"], Err(ExprError::Unknown("d9".into())));
    assert_eq!(values["d1"], Err(ExprError::Uses("d2".into())));
    assert!(matches!(values["d4"], Err(ExprError::Syntax(_))));
    assert!(matches!(values["a"], Err(ExprError::Circular(_))));
    assert!(matches!(values["b"], Err(ExprError::Circular(_))));
}

#[test]
fn expressions_are_kept_in_ascii() {
    assert_eq!(qiyas_param::expr::normalize(" ٦٫٥ * ۲ "), "6.5 * 2");
}

#[test]
fn powers_functions_and_rounding_have_defined_precedence() {
    let eval = |text| Expr::parse(text).unwrap().eval(&|_| None).unwrap();
    for (text, expected) in [
        ("2^3^2", 512.0),
        ("2**3", 8.0),
        ("-2^2", -4.0),
        ("(-2)^2", 4.0),
        ("2^-2", 0.25),
        ("min(3, 1 + 1, 9)", 2.0),
        ("max(-5, -2, -8)", -2.0),
        ("min(4)", 4.0),
        ("round(١٢٫٣٤٥, ٢)", 12.35),
        ("round(1250, -2)", 1300.0),
        ("round(-2.5)", -3.0),
        ("ceil(-1.2) + floor(1.9)", 0.0),
    ] {
        assert_eq!(eval(text), expected, "{text}");
    }
}

#[test]
fn typed_comparisons_logic_and_conditionals_are_lazy() {
    use qiyas_param::expr::Value;
    let resolver = |name: &str| match name {
        "kind" => Some(Value::Text("duct".into())),
        "geometry.length" => Some(Value::Number(4.0)),
        "insulated" => Some(Value::Bool(true)),
        _ => panic!("An unused branch resolved {name}"),
    };
    for text in [
        "1 < 2 and 3 >= 3",
        "1 <= 1 and 3 > 2",
        "1 = 1 and 2 != 3 and 4 <> 5",
        "false or true and true",
        "not 3 < 2",
        "!false && true || missing",
        "true or missing",
        "not (false and missing)",
        "kind == 'duct' and insulated",
        "'a' < 'b'",
    ] {
        assert_eq!(
            Expr::parse(text).unwrap().eval_with(&resolver),
            Ok(Value::Bool(true)),
            "{text}"
        );
    }
    assert_eq!(
        Expr::parse("if(geometry.length > 0, geometry.length^2, 1/0)")
            .unwrap()
            .eval_with(&resolver),
        Ok(Value::Number(16.0))
    );
    assert_eq!(
        Expr::parse("if(false, missing, 'لا خصم')")
            .unwrap()
            .eval_with(&resolver),
        Ok(Value::Text("لا خصم".into()))
    );
    assert_eq!(
        Expr::parse("if(0, 1/0, 7)").unwrap().eval(&|_| None),
        Ok(7.0)
    );
}

#[test]
fn lookup_uses_exact_typed_keys_and_reports_missing_data() {
    use qiyas_param::expr::{Resolver, Value};
    struct Tables;
    impl Resolver for Tables {
        fn resolve(&self, name: &str) -> Option<Value> {
            (name == "product").then(|| Value::Text("كود ١٢".into()))
        }
        fn lookup(&self, table: &str, key: &Value, column: &str) -> Option<Value> {
            match (table, key, column) {
                ("products", Value::Text(key), "thickness") if key == "كود ١٢" => {
                    Some(Value::Number(0.02))
                }
                ("levels", Value::Number(key), "name") if *key == 2.0 => {
                    Some(Value::Text("L2".into()))
                }
                _ => None,
            }
        }
    }
    for key in ["product", "'كود ١٢'"] {
        let text = format!("lookup('products', {key}, 'thickness') * 3");
        assert_eq!(
            Expr::parse(&text).unwrap().eval_with(&Tables),
            Ok(Value::Number(0.06))
        );
    }
    assert_eq!(
        Expr::parse("lookup('levels', ٢, 'name')")
            .unwrap()
            .eval_with(&Tables),
        Ok(Value::Text("L2".into()))
    );
    for text in [
        "lookup('unknown', product, 'thickness')",
        "lookup('levels', '2', 'name')",
        "lookup('products', product, 'unknown')",
    ] {
        assert!(
            matches!(
                Expr::parse(text).unwrap().eval_with(&Tables),
                Err(ExprError::Lookup(_))
            ),
            "{text}"
        );
    }
}

#[test]
fn strings_are_not_numeric_and_all_evaluated_numbers_must_be_finite() {
    use qiyas_param::expr::Value;
    let none = |_: &str| None::<Value>;
    for text in [
        "'2' + 1",
        "true * 3",
        "2 == '2'",
        "min(1, false)",
        "if('false', 1, 2)",
        "round(1, 0.5)",
        "round(1, 309)",
        "lookup(1, 2, 'column')",
    ] {
        assert!(
            matches!(
                Expr::parse(text).unwrap().eval_with(&none),
                Err(ExprError::Type(_))
            ),
            "{text}"
        );
    }
    for text in [
        "1 / 0",
        "sqrt(-1)",
        "(-1)^0.5",
        "1e309",
        "min(1, 1/0)",
        "1 / 0 > 2",
        "if(1e309, 1, 2)",
    ] {
        assert!(
            matches!(
                Expr::parse(text).unwrap().eval_with(&none),
                Err(ExprError::NotANumber(_))
            ),
            "{text}"
        );
    }
    assert!(Expr::parse("x")
        .unwrap()
        .eval_with(&|_: &str| Some(Value::Number(f64::NAN)))
        .is_err());
}

#[test]
fn function_arity_and_deep_input_are_rejected_without_panicking() {
    for text in [
        "if(true, 1)",
        "min()",
        "max()",
        "round()",
        "round(1, 2, 3)",
        "sqrt(1, 2)",
        "lookup('x', 'key')",
        "min(1,)",
        "1 < 2 < 3",
        "'unclosed",
        "'bad\\q'",
        "a..b",
    ] {
        assert!(
            matches!(Expr::parse(text), Err(ExprError::Syntax(_))),
            "{text}"
        );
    }
    for text in [
        format!("{}1{}", "(".repeat(1000), ")".repeat(1000)),
        format!("{}1", "-".repeat(1000)),
        std::iter::repeat_n("1", 1000)
            .collect::<Vec<_>>()
            .join(" + "),
    ] {
        assert!(matches!(Expr::parse(&text), Err(ExprError::Syntax(_))));
    }
}

#[test]
fn typed_expression_display_round_trips_and_names_keep_source_order() {
    use qiyas_param::expr::Value;
    let text = "if(a.x > b and not c, lookup('جدول ١', d, 'name'), \"line\\nquoted \\\"name\\\"\")";
    let expr = Expr::parse(text).unwrap();
    assert_eq!(expr.names(), ["a.x", "b", "c", "d"]);
    assert_eq!(Expr::parse(&expr.to_string()).unwrap(), expr);
    assert_eq!(
        qiyas_param::expr::normalize(" ١ + if(true, '١', \"۲\") "),
        "1 + if(true, '١', \"۲\")"
    );
    assert_eq!(
        Expr::parse("\"a\\\\b\\t'c'\"")
            .unwrap()
            .eval_with(&|_: &str| None),
        Ok(Value::Text("a\\b\t'c'".into()))
    );
    for reserved in ["IF", "lookup", "false", "TRUE", "and", "or", "not", "round"] {
        assert!(!qiyas_param::expr::is_name(reserved));
    }
    assert!(qiyas_param::expr::is_name("floor_width"));
}
