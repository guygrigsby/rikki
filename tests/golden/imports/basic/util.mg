struct Pair {
    a: int
    b: int
}
fn double(x: int) int {
    return twice(x)
}
fn twice(x: int) int {
    return x * 2
}
fn make(a: int, b: int) Pair {
    return Pair{a: a, b: b}
}
