fn maybe() int? {
    return 3
}
fn main() {
    x := maybe()
    if x != none {
        if true {
            x = none
        }
        y := x + 1
        print(y)
    }
}
