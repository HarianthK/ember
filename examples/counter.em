fn make_counter() {
  let n = 0
  return fn() {
    n = n + 1
    return n
  }
}

let count = make_counter()
count()
count()
print("counted to", count())
