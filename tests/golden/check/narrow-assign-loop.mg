fn maybe() int? {
    return 3
}
fn main() {
    x := maybe()
    if x != none {
        for i in range(2) {
            y := x + 1
            print(y)
            x = none
        }
    }
}
