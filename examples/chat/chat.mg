// Chat with a local model over an OpenAI-compatible API.
//   tk chat.mg :8080            -> http://localhost:8080
//   tk chat.mg 10.0.0.5:11434 llama3.2
// Errors are handled, not just propagated: a failed request reports its
// cause chain and the session continues; three consecutive failures quit.
import "http"
import "ctx"
import py "json"

// Go-style address shorthand: ":8080" means localhost, scheme optional.
fn normalize(url str) str {
    u := url
    if u.starts_with(":") {
        u = "localhost" + u
    }
    if !u.starts_with("http://") && !u.starts_with("https://") {
        u = "http://" + u
    }
    return u
}

fn ask(c Ctx, base str, model str, history str) (str, error?) {
    body := sprintf("{\"model\": %q, \"messages\": %s}", model, history)
    resp, err := http.post(c, base + "/v1/chat/completions", body)
    if err != none {
        return "", error.wrap(err, "request failed")
    }
    if resp.status != 200 {
        detail := resp.body
        if len(detail) > 200 {
            detail = detail[0:200] + "..."
        }
        return "", error.new(sprintf("server returned %d: %s", resp.status, detail))
    }
    obj, jerr := json.loads(resp.body)
    if jerr != none {
        return "", error.wrap(jerr, "server sent invalid json")
    }
    reply, rerr := str(obj["choices"][0]["message"]["content"])
    if rerr != none {
        return "", error.wrap(rerr, "unexpected response shape")
    }
    return reply, none
}

fn main() (error?) {
    a := args()
    if len(a) == 0 {
        print("usage: tk chat.mg <url> [model]")
        return error.new("missing server url")
    }
    base := normalize(a[0])
    model := "default"
    if len(a) > 1 {
        model = a[1]
    }
    root := ctx.interrupt(ctx.background())
    history := [map[str, str]{"role": "system", "content": "You are concise."}]
    printf("chatting with %s (model %s); ctrl-d to quit\n", base, model)

    fails := 0
    for {
        line, ierr := input("> ")
        if ierr != none {
            // EOF is a normal way to leave, not a failure
            break
        }
        if line == "" {
            continue
        }
        history = history.append(map[str, str]{"role": "user", "content": line})
        hjson, jerr := str(json.dumps(history))
        if jerr != none {
            // our own state failed to encode: that is a bug, die loudly
            return error.wrap(jerr, "cannot encode chat history")
        }
        reply, err := ask(ctx.timeout(root, 120.0), base, model, hjson)
        if err != none {
            // recoverable: report the chain, forget the unanswered turn,
            // keep the session alive
            printf("!! %s\n", err.msg)
            cause := err.cause
            if cause != none {
                printf("   cause: %s\n", cause.msg)
            }
            history = history[0:len(history) - 1]
            fails = fails + 1
            if fails >= 3 {
                return error.wrap(err, "giving up after 3 consecutive failures")
            }
            continue
        }
        fails = 0
        print(reply)
        history = history.append(map[str, str]{"role": "assistant", "content": reply})
    }
    print("bye")
    return none
}
