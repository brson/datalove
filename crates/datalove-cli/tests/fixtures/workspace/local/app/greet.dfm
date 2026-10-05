require module sys/std/string

fun greeting(ref name: string): string
  var out = "hello, "
  call string.push_str(mut out, ref name)
  ret out
end fun

fun answer(): int
  ret 42
end fun
