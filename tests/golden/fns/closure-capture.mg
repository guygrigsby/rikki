fn main() {
    n := 1
    f := fn() int { return n }
    n = 2
    print(f())
}
