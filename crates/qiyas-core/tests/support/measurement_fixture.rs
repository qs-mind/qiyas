use qiyas_core::{Outcome, Session, Source};
use serde_json::{json, Value};

pub fn run(s: &mut Session, command: Value) -> Outcome {
    s.run_input(&command, Source::User).unwrap().outcome
}
pub fn fixture(s: &mut Session) -> (u64, Vec<u64>) {
    run(
        s,
        json!({"type":"define_object_type","definition":{"name":"Surface","category":"Test trade","geometry":"path","properties":{
        "rise":{"kind":"number","unit":"length","default":2.8},
        "width":{"kind":"number","unit":"length","default":0.2},
        "copies":{"kind":"number","unit":"count","default":3},
        "grid":{"kind":"text","default":"A/1-2"}
    },"quantities":{"gross":"geometry.length * rise"}}}),
    );
    run(
        s,
        json!({"type":"define_object_type","definition":{"name":"Void","category":"Test void","geometry":"point","properties":{
        "width":{"kind":"number","unit":"length","default":1.2},
        "rise":{"kind":"number","unit":"length","default":2.0},
        "edge":{"kind":"bool","default":false},
        "grid":{"kind":"text","default":"A/1"}
    },"quantities":{"opening":"width * rise"}}}),
    );
    let u = 1.0 / s.drawing.units.to_metres();
    let Outcome::Added { handles } = run(
        s,
        json!({"type":"place_objects","objects":[{"type":"Surface","points":[[0,0],[4.2*u,0]]}]}),
    ) else {
        panic!()
    };
    let host = handles[0];
    let Outcome::Added { handles } = run(
        s,
        json!({"type":"place_objects","objects":[
            {"type":"Void","host":host,"points":[[u,0]],"props":{"width":0.4,"rise":1.0}},
            {"type":"Void","host":host,"points":[[2.0*u,0]]},
            {"type":"Void","host":host,"points":[[4.0*u,0]],"props":{"width":0.2,"rise":1.0,"edge":true}}
        ]}),
    ) else {
        panic!()
    };
    (host, handles)
}
fn cite(clause: &str) -> Value {
    json!({"source":"Invented test method; not a published standard", "clause":clause})
}
pub fn method(name: &str, linear: bool) -> Value {
    let terms = json!([{"label":"Gross surface", "operation":"add", "timesing":"copies", "factors":["geometry.length","rise"],"citation":cite("G")}]);
    let deduction = json!({"element_type":"Void","when":"void.opening > 0.5","boundary":{"when":"void.edge", "deduct_when":"true", "citation":cite("B")},"term":{"label":"Opening", "operation":"deduct", "factors":["void.width","void.rise"],"citation":cite("D")}});
    let mut rule = json!({"id":"surface", "element_type":"Surface", "trade":"Test trade", "unit":"m2", "terms":terms,"deductions":[deduction],"citation":cite("Q"),"coverage":[{"text":"Test coverage only", "citation":cite("C")}],"rounding":{"decimals":3,"mode":"nearest","citation":cite("R")}});
    if linear {
        rule["bands"] = json!([{"when":"width < 0.3", "unit":"m", "citation":cite("U"),
            "terms":[{"label":"Linear band", "operation":"add", "timesing":"copies", "factors":["geometry.length"],"citation":cite("L")}],
            "deductions":[{"element_type":"Void","when":"void.opening >= 0.4", "boundary":{"when":"void.edge", "deduct_when":"false", "citation":cite("B2")},
                "term":{"label":"Opening width", "operation":"deduct", "factors":["void.width"],"citation":cite("D2")}}]}]);
    }
    json!({"version":1,"name":name,"title":name,"based_on":"Synthetic only","source_note":"Test fixture","rules":[rule]})
}
pub fn mappings() -> Value {
    json!([{"rule":"surface","code":"S-{unit}","description_en":"Surface {width} wide","description_ar":"سطح بعرض {width}","unit":"{unit}","section":"Test section", "trade":"{trade}","citation":cite("M")}])
}
pub fn apply(s: &mut Session, method: Value) {
    run(
        s,
        json!({"type":"measurement","kind":"set_mappings","mappings":mappings()}),
    );
    run(
        s,
        json!({"type":"measurement","kind":"set_method","scope":"settings","method":method}),
    );
}
