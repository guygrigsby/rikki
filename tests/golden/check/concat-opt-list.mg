fn maybe() int? {
    return 3
}
fn main() {
    xs := [1]
    ys := [maybe()]
    zs := xs + ys
    y := zs[0] + 1
    print(y)
}
