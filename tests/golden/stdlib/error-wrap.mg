fn inner() (error?) {
    return error.new("root cause")
}
fn outer() (error?) {
    err := inner()
    if err != none {
        return error.wrap(err, "outer failed")
    }
    return none
}
fn main() {
    err := outer()
    if err != none {
        print(err.msg)
        c := err.cause
        if c != none {
            print(c.msg)
        }
    }
}
