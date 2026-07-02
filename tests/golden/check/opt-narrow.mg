struct User {
    name str
}
fn find() User? {
    return User{name: "guy"}
}
fn main() {
    u := find()
    if u != none {
        print(u.name)
    }
}
