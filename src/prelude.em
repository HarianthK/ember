// The part of the standard library written in ember itself: these take functions, and a
// native written in Rust cannot call back into ember code. Every VM runs this at start.

fn map(xs, f) {
  let out = []
  for x in xs { push(out, f(x)) }
  return out
}

fn filter(xs, keep) {
  let out = []
  for x in xs { if keep(x) { push(out, x) } }
  return out
}

fn reduce(xs, f, start) {
  let total = start
  for x in xs { total = f(total, x) }
  return total
}

// A new list of xs ordered by key(x), each key worked out once. Equal keys keep their order,
// so sorting by one field and then another sorts by both. Keys compare as < does.
fn sort_by(xs, key) {
  fn merge(ps) {
    if len(ps) <= 1 { return ps }
    let half = floor(len(ps) / 2)
    let a = merge(slice(ps, 0, half))
    let b = merge(slice(ps, half))
    let out = []
    let i = 0
    let j = 0
    while i < len(a) and j < len(b) {
      // Only a strictly smaller key from the right goes first, which is what keeps it stable.
      if b[j][0] < a[i][0] {
        push(out, b[j])
        j = j + 1
      } else {
        push(out, a[i])
        i = i + 1
      }
    }
    return out + slice(a, i) + slice(b, j)
  }
  let pairs = map(xs, fn(x) { return [key(x), x] })
  return map(merge(pairs), fn(p) { return p[1] })
}
