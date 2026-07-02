fn apply(f fn(int) int, x int) int {
    return f(x)
}
fn add3(x int) int {
    return x + 3
}
fn main() {
    print(apply(add3, 10))
    g := add3
    print(g(1))
    adder := fn(x int) int { return x + 100 }
    print(apply(adder, 1))
}
