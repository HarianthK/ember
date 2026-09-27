// Everything the language can do, in one file that runs.
let greeting = "hello"

fn fib(n) {
  if n < 2 { return n }
  return fib(n - 1) + fib(n - 2)
}
print(greeting, "fib(20) is", fib(20))

let squares = []
for n in [1, 2, 3, 4] {
  squares = squares + [n * n]
}
print("squares", squares)

let person = {"name": "ada", "born": 1815}
person.name = "Ada"
print(person)

let counter = fn(start) {
  let n = start
  return fn() {
    n = n + 1
    return n
  }
}
let next = counter(10)
next()
print("counted to", next())

let words = ["to", "be", "or", "not", "to", "be"]
let counts = {}
for w in words {
  counts[w] = (has(counts, w) and counts[w] or 0) + 1
}
print("word counts", counts)

let i = 0
let done = false
while i < 10 and not done {
  i = i + 1
  done = i * i > 20
}
print("first square over 20 is", i, "squared")
