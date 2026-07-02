fn double(x int) int {
    return x * 2
}
fn main() {
    nums := [1, 2, 3, 4]
    big := nums.map(double).filter(fn(x) { x > 2 }).sum()
    print(big)
}
