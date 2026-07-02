import py "json"

fn poke(obj py) (error?) {
    check obj.keys()
    return none
}

fn main() {
    obj, err := json.loads("{nope")
    if err != none {
        print(err.pytype)
    }
    perr := poke(obj)
    if perr != none {
        print(perr.pytype)
    }
}
