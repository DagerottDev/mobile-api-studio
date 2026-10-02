use rquickjs::{Context, Function, Runtime, Value};
use serde_json::Value as JsonValue;
use std::{
    io::{self, Read, Write},
    time::{Duration, Instant},
};

const MAX_INPUT: usize = 3 * 1024 * 1024;
const MAX_SCRIPT: usize = 64 * 1024;
const MAX_EVENT: usize = 2 * 1024 * 1024;
const MAX_OUTPUT: usize = 2 * 1024 * 1024;
const MEMORY_LIMIT: usize = 32 * 1024 * 1024;
const WALL_LIMIT: Duration = Duration::from_millis(100);
const ERROR: &str = r#"{"error":"script_failed"}"#;

fn fail() -> String {
    ERROR.to_owned()
}

fn run(input: &[u8], deadline: Instant) -> String {
    if input.len() > MAX_INPUT || Instant::now() >= deadline {
        return fail();
    }
    let Ok(request) = serde_json::from_slice::<JsonValue>(input) else {
        return fail();
    };
    if Instant::now() >= deadline {
        return fail();
    }
    let (Some(script), Some(event)) = (
        request.get("script").and_then(JsonValue::as_str),
        request.get("event"),
    ) else {
        return fail();
    };
    let Ok(event_json) = serde_json::to_string(event) else {
        return fail();
    };
    if script.len() > MAX_SCRIPT || event_json.len() > MAX_EVENT || Instant::now() >= deadline {
        return fail();
    }

    let Ok(runtime) = Runtime::new() else {
        return fail();
    };
    runtime.set_memory_limit(MEMORY_LIMIT);
    runtime.set_max_stack_size(256 * 1024);
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
    let Ok(context) = Context::full(&runtime) else {
        return fail();
    };

    let result = context.with(|ctx| -> rquickjs::Result<String> {
        // Keep references to trusted intrinsics before running user code.
        let json_obj: rquickjs::Object = ctx.globals().get("JSON")?;
        let parse: Function = json_obj.get("parse")?;
        let stringify: Function = json_obj.get("stringify")?;
        let event: Value = parse.call((event_json.as_str(),))?;
        ctx.eval::<(), _>(script)?;
        let transform: Function = ctx.globals().get("transform")?;
        let output: Value = transform.call((event,))?;
        if output.is_promise() {
            return Err(rquickjs::Error::new_from_js("promise", "object"));
        }
        stringify.call((output,))
    });
    match result {
        Ok(output) if output.len() <= MAX_OUTPUT && Instant::now() < deadline => {
            match serde_json::from_str::<JsonValue>(&output) {
                Ok(value) if value.is_object() => output,
                _ => fail(),
            }
        }
        _ => fail(),
    }
}

fn main() {
    let deadline = Instant::now() + WALL_LIMIT;
    let mut input = Vec::new();
    if io::stdin()
        .take((MAX_INPUT + 1) as u64)
        .read_to_end(&mut input)
        .is_err()
    {
        print_result(&fail());
        return;
    }
    print_result(&run(&input, deadline));
}

fn print_result(value: &str) {
    let _ = io::stdout().write_all(value.as_bytes());
    let _ = io::stdout().write_all(b"\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(script: &str, event: &str) -> String {
        let input =
            json!({"script": script, "event": serde_json::from_str::<JsonValue>(event).unwrap()})
                .to_string();
        run(input.as_bytes(), Instant::now() + WALL_LIMIT)
    }

    fn call_until(script: &str, event: &str, limit: Duration) -> String {
        let input =
            json!({"script": script, "event": serde_json::from_str::<JsonValue>(event).unwrap()})
                .to_string();
        run(input.as_bytes(), Instant::now() + limit)
    }

    #[test]
    fn worker_limits_and_contract() {
        assert_eq!(
            call(
                "function transform(e) { e.changed = true; return e; }",
                r#"{"x":1}"#
            ),
            r#"{"x":1,"changed":true}"#
        );
        assert_eq!(
            call("function transform() { while (true) {} }", "{}"),
            ERROR
        );
        assert_eq!(
            call_until(
                "Array(100000000).fill(1); function transform() { return {ok:true}; }",
                "{}",
                Duration::from_secs(2),
            ),
            ERROR
        );
        assert_eq!(
            call(
                "function transform() { return { console: typeof console, process: typeof process, require: typeof require, fetch: typeof fetch, imports: typeof globalThis.import, os: typeof os, std: typeof std }; }",
                "{}"
            ),
            r#"{"console":"undefined","process":"undefined","require":"undefined","fetch":"undefined","imports":"undefined","os":"undefined","std":"undefined"}"#
        );
        assert_eq!(call(&" ".repeat(MAX_SCRIPT + 1), "{}"), ERROR);
        assert_eq!(
            call(
                "function transform(e) { return e; }",
                &format!(r#""{}""#, "x".repeat(MAX_EVENT + 1))
            ),
            ERROR
        );
        let oversized = format!(r#"{{"script":"","event":"{}"}}"#, "x".repeat(MAX_INPUT));
        assert_eq!(
            run(oversized.as_bytes(), Instant::now() + WALL_LIMIT),
            ERROR
        );
        assert_eq!(
            call(
                "function transform() { return {x:'x'.repeat(3*1024*1024)}; }",
                "{}"
            ),
            ERROR
        );
        assert_eq!(call("function transform() { return []; }", "{}"), ERROR);
        assert_eq!(
            call("async function transform(e) { return e; }", "{}"),
            ERROR
        );
        assert_eq!(run(b"not json", Instant::now() + WALL_LIMIT), ERROR);
    }
}
