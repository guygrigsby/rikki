fn boom() (int, error?) {
    return 0, error.new("kaboom")
}
fn hello() (int, error?) {
    return 7, none
}
fn run() (int, error?) {
    n := check hello()
    m := check boom()
    return n + m, none
}
fn main() {
    v, err := run()
    if err != none {
        print("got: " + err.msg)
        return
    }
    print(v)
}
