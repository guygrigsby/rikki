fn boom() (int, error?) {
    return 0, error.new("kaboom")
}
fn main() {
    x := check boom()
    print(x)
}
