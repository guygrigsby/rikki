import py "json"

fn main() (error?) {
    obj := check json.loads("{\"a\": [1, 2, 3]}")
    xs := check []int(obj["a"])
    print(xs.sum())
    _, err := json.loads("{nope")
    if err != none {
        print(err.pytype)
    }
    return none
}
