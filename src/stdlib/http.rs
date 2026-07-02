use std::time::Duration;

use indexmap::IndexMap;
use ureq::Agent;

use crate::interp::{Fault, Interp};
use crate::value::{ErrVal, MapKey, Value};

fn zero_response() -> Value {
    let mut fields = IndexMap::new();
    fields.insert("status".to_string(), Value::Int(0));
    fields.insert("body".to_string(), Value::Str(String::new()));
    fields.insert("headers".to_string(), Value::Map(IndexMap::new()));
    Value::Struct {
        name: "Response".into(),
        fields,
    }
}

fn fail(msg: String) -> Value {
    Value::Tuple(vec![
        zero_response(),
        Value::Err(ErrVal {
            msg,
            ..Default::default()
        }),
    ])
}

struct Req {
    method: String,
    url: String,
    body: Option<String>,
    headers: Vec<(String, String)>,
}

pub fn call(interp: &mut Interp, name: &str, args: Vec<Value>) -> Result<Value, Fault> {
    if name == "stream" {
        return stream(interp, args);
    }
    let bad = |i: &Interp| Err(i.fault(format!("http.{name}: bad arguments")));
    let (ctx, req) = match (name, args.as_slice()) {
        ("get", [Value::Ctx(c), Value::Str(url)]) => (
            c.clone(),
            Req {
                method: "GET".into(),
                url: url.clone(),
                body: None,
                headers: vec![],
            },
        ),
        ("post", [Value::Ctx(c), Value::Str(url), Value::Str(body)]) => (
            c.clone(),
            Req {
                method: "POST".into(),
                url: url.clone(),
                body: Some(body.clone()),
                headers: vec![],
            },
        ),
        (
            "request",
            [Value::Ctx(c), Value::Struct {
                name: sname,
                fields,
            }],
        ) if sname == "Request" => {
            let (Some(Value::Str(method)), Some(Value::Str(url)), Some(Value::Str(body))) =
                (fields.get("method"), fields.get("url"), fields.get("body"))
            else {
                return bad(interp);
            };
            let mut headers = vec![];
            if let Some(Value::Map(h)) = fields.get("headers") {
                for (k, v) in h {
                    if let (MapKey::Str(k), Value::Str(v)) = (k, v) {
                        headers.push((k.clone(), v.clone()));
                    }
                }
            }
            let body = if body.is_empty() && method == "GET" {
                None
            } else {
                Some(body.clone())
            };
            (
                c.clone(),
                Req {
                    method: method.clone(),
                    url: url.clone(),
                    body,
                    headers,
                },
            )
        }
        _ => return bad(interp),
    };

    // ctx checks before any I/O
    if let Some(e) = ctx.err() {
        return Ok(fail(format!("http.{name} {}: {}", req.url, e.msg)));
    }
    let timeout = ctx.remaining().unwrap_or(Duration::from_secs(30));

    let agent: Agent = Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .user_agent("rikki/0.1")
        .build()
        .into();

    let mut builder = ureq::http::Request::builder()
        .method(req.method.as_str())
        .uri(&req.url);
    for (k, v) in &req.headers {
        builder = builder.header(k, v);
    }
    let request = match builder.body(req.body.unwrap_or_default()) {
        Ok(r) => r,
        Err(e) => return Ok(fail(format!("http.{name} {}: {e}", req.url))),
    };
    let mut resp = match agent.run(request) {
        Ok(r) => r,
        Err(e) => return Ok(fail(format!("http.{name} {}: {e}", req.url))),
    };

    let status = resp.status().as_u16() as i64;
    let mut headers = IndexMap::new();
    for (k, v) in resp.headers() {
        headers.insert(
            MapKey::Str(k.as_str().to_string()),
            Value::Str(v.to_str().unwrap_or("").to_string()),
        );
    }
    let body = match resp.body_mut().read_to_string() {
        Ok(b) => b,
        Err(e) => return Ok(fail(format!("http.{name} {}: {e}", req.url))),
    };

    let mut fields = IndexMap::new();
    fields.insert("status".to_string(), Value::Int(status));
    fields.insert("body".to_string(), Value::Str(body));
    fields.insert("headers".to_string(), Value::Map(headers));
    Ok(Value::Tuple(vec![
        Value::Struct {
            name: "Response".into(),
            fields,
        },
        Value::NoneV,
    ]))
}

/// POST with a per-line callback: the handler sees each response line as it
/// arrives (SSE-friendly); the returned Response carries the accumulated
/// body, since capture-by-value closures cannot collect state themselves.
fn stream(interp: &mut Interp, args: Vec<Value>) -> Result<Value, Fault> {
    let [Value::Ctx(ctx), Value::Str(url), Value::Str(body), handler @ Value::Fn(_)] =
        args.as_slice()
    else {
        return Err(interp.fault("http.stream: bad arguments"));
    };
    if let Some(e) = ctx.err() {
        return Ok(fail(format!("http.stream {url}: {}", e.msg)));
    }
    let timeout = ctx.remaining().unwrap_or(Duration::from_secs(300));
    let agent: Agent = Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .user_agent("rikki/0.1")
        .build()
        .into();
    let mut resp = match agent.post(url).send(body.as_str()) {
        Ok(r) => r,
        Err(e) => return Ok(fail(format!("http.stream {url}: {e}"))),
    };
    let status = resp.status().as_u16() as i64;
    let mut headers = IndexMap::new();
    for (k, v) in resp.headers() {
        headers.insert(
            MapKey::Str(k.as_str().to_string()),
            Value::Str(v.to_str().unwrap_or("").to_string()),
        );
    }
    let mut full = String::new();
    {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(resp.body_mut().as_reader());
        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(e) => return Ok(fail(format!("http.stream {url}: {e}"))),
            };
            interp.call_value(handler, vec![Value::Str(line.clone())])?;
            full.push_str(&line);
            full.push('\n');
        }
    }
    let mut fields = IndexMap::new();
    fields.insert("status".to_string(), Value::Int(status));
    fields.insert("body".to_string(), Value::Str(full));
    fields.insert("headers".to_string(), Value::Map(headers));
    Ok(Value::Tuple(vec![
        Value::Struct { name: "Response".into(), fields },
        Value::NoneV,
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Program;
    use crate::stdlib::ctx::CtxInner;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    fn serve_once(response: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf);
                let _ = sock.write_all(response.as_bytes());
            }
        });
        format!("http://{addr}/")
    }

    fn bg() -> Value {
        Value::Ctx(Arc::new(CtxInner {
            deadline: None,
            interrupted: None,
        }))
    }

    fn run(name: &str, args: Vec<Value>) -> Value {
        let prog = Program::default();
        let mut i = Interp::new(&prog);
        call(&mut i, name, args).map_err(|f| f.msg).unwrap()
    }

    #[test]
    fn get_ok() {
        let url = serve_once(
            "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nx-test: yes\r\nconnection: close\r\n\r\nok",
        );
        let v = run("get", vec![bg(), Value::Str(url)]);
        let Value::Tuple(ts) = v else { panic!() };
        assert!(
            matches!(ts[1], Value::NoneV),
            "unexpected error: {:?}",
            ts[1]
        );
        let Value::Struct { fields, .. } = &ts[0] else {
            panic!()
        };
        assert!(matches!(fields["status"], Value::Int(200)));
        assert!(matches!(&fields["body"], Value::Str(b) if b == "ok"));
        let Value::Map(h) = &fields["headers"] else {
            panic!()
        };
        assert!(matches!(&h[&MapKey::Str("x-test".into())], Value::Str(s) if s == "yes"));
    }

    #[test]
    fn non_2xx_is_a_response_not_an_error() {
        let url =
            serve_once("HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
        let v = run("get", vec![bg(), Value::Str(url)]);
        let Value::Tuple(ts) = v else { panic!() };
        assert!(matches!(ts[1], Value::NoneV));
        let Value::Struct { fields, .. } = &ts[0] else {
            panic!()
        };
        assert!(matches!(fields["status"], Value::Int(404)));
    }

    #[test]
    fn connection_refused_is_error_value() {
        // bind and drop to get a dead port
        let addr = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap()
        };
        let v = run("get", vec![bg(), Value::Str(format!("http://{addr}/"))]);
        let Value::Tuple(ts) = v else { panic!() };
        assert!(matches!(ts[1], Value::Err(_)));
    }

    #[test]
    fn expired_ctx_never_dials() {
        let expired = Value::Ctx(Arc::new(CtxInner {
            deadline: Some(std::time::Instant::now()),
            interrupted: None,
        }));
        let v = run(
            "get",
            vec![expired, Value::Str("http://127.0.0.1:1/".into())],
        );
        let Value::Tuple(ts) = v else { panic!() };
        match &ts[1] {
            Value::Err(e) => assert!(e.msg.contains("deadline"), "{}", e.msg),
            v => panic!("{v:?}"),
        }
    }

    #[test]
    fn post_sends_body() {
        let url =
            serve_once("HTTP/1.1 201 Created\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
        let v = run(
            "post",
            vec![bg(), Value::Str(url), Value::Str("payload".into())],
        );
        let Value::Tuple(ts) = v else { panic!() };
        assert!(matches!(ts[1], Value::NoneV));
        let Value::Struct { fields, .. } = &ts[0] else {
            panic!()
        };
        assert!(matches!(fields["status"], Value::Int(201)));
    }
}
