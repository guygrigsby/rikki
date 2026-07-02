fn f(n int) int { return f(n + 1) }

fn main() {
    print(f(0))
}
