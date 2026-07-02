import py "json"

fn main() (error?) {
    s := check str(check json.dumps([1, 2, 3]))
    print(s)
    n, err := int(check json.loads("\"x\""))
    if err != none {
        print("no int")
        print(n)
    }
    a := check json.loads("40")
    b := check (a + json.loads("2"))
    print(check int(b))
    m := check json.loads("{\"k\": 7}")
    v := check int(m["k"])
    print(v)
    print(a)
    return none
}
