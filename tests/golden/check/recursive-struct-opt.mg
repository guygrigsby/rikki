struct Node {
    val: int
    next: Node?
}
fn main() {
    tail := Node{val: 2, next: none}
    head := Node{val: 1, next: tail}
    print(head.val)
    nx := head.next
    if nx != none {
        print(nx.val)
    }
}
