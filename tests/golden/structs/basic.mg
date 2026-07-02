struct User {
    name str
    age int
}
fn birthday(u User) User {
    u.age = u.age + 1
    return u
}
fn main() {
    u := User{name: "guy", age: 44}
    print(u.name)
    print(u.age)
    older := birthday(u)
    print(older.age)
    print(u.age)
    u.name = "g"
    print(u.name)
    print(u)
    us := [User{name: "a", age: 1}, User{name: "b", age: 2}]
    print(us[1].name)
    printf("%v\n", us[0])
}
