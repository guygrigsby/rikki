import "ctx"

fn main() {
    c := ctx.background()
    print(c.done())
    t := ctx.timeout(c, 0.0)
    print(t.done())
    e := t.err()
    if e != none {
        print(e.msg)
    }
}
