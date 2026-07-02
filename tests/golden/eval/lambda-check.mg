fn boom() (int, error?) { return 0, error.new("boom") }

fn main() {
    f := fn() (int, error?) {
        v := check boom()
        return v + 1, none
    }
    a, b := f()
    if b != none {
        print(a)
        print(b.msg)
    }
}
