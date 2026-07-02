fn parse_age(s str) (int, error?) {
    n, err := int(s)
    if err != none {
        return 0, error.wrap(err, "bad age")
    }
    return n, none
}
fn main() {
    a, err := parse_age("44")
    if err != none {
        print(err.msg)
        return
    }
    print(a)
    b, err2 := parse_age("nope")
    if err2 != none {
        print(err2.msg)
        c := err2.cause
        if c != none {
            print(c.msg)
        }
        return
    }
    print(b)
}
