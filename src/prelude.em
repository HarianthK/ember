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
