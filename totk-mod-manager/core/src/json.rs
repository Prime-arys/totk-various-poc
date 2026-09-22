//! A tiny JSON writer: the C++ side parses what the library returns with
//! nlohmann::json, which borealis already ships.

use totk_merge::prelude::*;

pub struct Json {
    out: String,
    /// Whether the next value in the current object/array needs a comma.
    needs_comma: Vec<bool>,
}

impl Json {
    pub fn new() -> Json {
        Json {
            out: String::new(),
            needs_comma: Vec::new(),
        }
    }

    fn separator(&mut self) {
        if let Some(needs) = self.needs_comma.last_mut() {
            if *needs {
                self.out.push(',');
            }
            *needs = true;
        }
    }

    fn key(&mut self, key: &str) {
        self.separator();
        push_string(&mut self.out, key);
        self.out.push(':');
        // The value that follows must not add a comma of its own.
        if let Some(needs) = self.needs_comma.last_mut() {
            *needs = false;
        }
    }

    fn after_value(&mut self) {
        if let Some(needs) = self.needs_comma.last_mut() {
            *needs = true;
        }
    }

    pub fn begin_object(&mut self) -> &mut Self {
        self.separator();
        self.out.push('{');
        self.needs_comma.push(false);
        self
    }

    pub fn end_object(&mut self) -> &mut Self {
        self.needs_comma.pop();
        self.out.push('}');
        self.after_value();
        self
    }

    pub fn end_array(&mut self) -> &mut Self {
        self.needs_comma.pop();
        self.out.push(']');
        self.after_value();
        self
    }

    pub fn object_field(&mut self, key: &str) -> &mut Self {
        self.key(key);
        self.out.push('{');
        self.needs_comma.push(false);
        self
    }

    pub fn array_field(&mut self, key: &str) -> &mut Self {
        self.key(key);
        self.out.push('[');
        self.needs_comma.push(false);
        self
    }

    pub fn string(&mut self, value: &str) -> &mut Self {
        self.separator();
        push_string(&mut self.out, value);
        self
    }

    pub fn number(&mut self, value: i64) -> &mut Self {
        self.separator();
        self.out.push_str(&value.to_string());
        self
    }

    pub fn field_str(&mut self, key: &str, value: &str) -> &mut Self {
        self.key(key);
        push_string(&mut self.out, value);
        self.after_value();
        self
    }

    pub fn field_opt_str(&mut self, key: &str, value: Option<&str>) -> &mut Self {
        match value {
            Some(value) => self.field_str(key, value),
            None => {
                self.key(key);
                self.out.push_str("null");
                self.after_value();
                self
            }
        }
    }

    pub fn field_bool(&mut self, key: &str, value: bool) -> &mut Self {
        self.key(key);
        self.out.push_str(if value { "true" } else { "false" });
        self.after_value();
        self
    }

    pub fn field_num(&mut self, key: &str, value: i64) -> &mut Self {
        self.key(key);
        self.out.push_str(&value.to_string());
        self.after_value();
        self
    }

    /// A number with one decimal, for durations.
    pub fn field_tenths(&mut self, key: &str, value: f32) -> &mut Self {
        self.key(key);
        let tenths = (value * 10.0) as i64;
        self.out.push_str(&format!("{}.{}", tenths / 10, tenths % 10));
        self.after_value();
        self
    }

    pub fn finish(self) -> String {
        self.out
    }
}

fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_nested_values() {
        let mut json = Json::new();
        json.begin_object()
            .field_str("name", "a \"b\"\n")
            .field_bool("ok", true)
            .array_field("list")
            .string("x")
            .number(2)
            .begin_object()
            .field_num("n", 3)
            .end_object()
            .end_array()
            .object_field("empty")
            .end_object()
            .field_tenths("seconds", 12.34)
            .end_object();
        assert_eq!(
            json.finish(),
            r#"{"name":"a \"b\"\n","ok":true,"list":["x",2,{"n":3}],"empty":{},"seconds":12.3}"#
        );
    }
}
