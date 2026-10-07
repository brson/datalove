// Turning numbers into the text of a report.

require module sys/std/list
require module sys/std/string

// A whole number, as `string` writes it.
fun digits(n: u64): string
    let wide: int = n@
    ret string.from_int(ref wide)
end fun

// A count, with commas between the thousands: 1234567 is "1,234,567".
fun count(n: u64): !string
    if n .< 1000
        ret ok digits(n)
    end if
    let high = n /! 1000
    let low = n -! high *! 1000
    var out = count(high)!
    call string.push_str(mut out, ref ",")
    if low .< 10
        call string.push_str(mut out, ref "00")
    else
        if low .< 100
            call string.push_str(mut out, ref "0")
        end if
    end if
    call string.push_str(mut out, ref digits(low))
    ret ok out
end fun

// Cents as dollars: 123456 is "$1,234.56".
fun money(cents: u64): !string
    let dollars = cents /! 100
    let rest = cents -! dollars *! 100
    var out = "$"
    call string.push_str(mut out, ref count(dollars)!)
    call string.push_str(mut out, ref ".")
    if rest .< 10
        call string.push_str(mut out, ref "0")
    end if
    call string.push_str(mut out, ref digits(rest))
    ret ok out
end fun

// A share of a whole as a percentage to a tenth: 1 of 8 is "12.5%".
fun percent(part: u64, whole: u64): !string
    if whole == 0
        ret ok "-"
    end if
    // Rounded to the nearest tenth of a percent.
    let tenths = (part *! 1000 +! whole /! 2) /! whole
    let points = tenths /! 10
    var out = digits(points)
    call string.push_str(mut out, ref ".")
    call string.push_str(mut out, ref digits(tenths -! points *! 10))
    call string.push_str(mut out, ref "%")
    ret ok out
end fun

// A ratio of two counts to a tenth, as an average: 7 over 2 is "3.5".
fun ratio(num: u64, den: u64): !string
    if den == 0
        ret ok "-"
    end if
    let tenths = (num *! 10 +! den /! 2) /! den
    let whole = tenths /! 10
    var out = digits(whole)
    call string.push_str(mut out, ref ".")
    call string.push_str(mut out, ref digits(tenths -! whole *! 10))
    ret ok out
end fun

// Text padded on the right to a width, or cut to it.
fun left(ref text: string, width: index): !string
    let len = string.char_count(ref text)
    if len .> width
        if string.slice(ref text, 0, width -! 1) |cut|
            var out = cut
            call string.push_str(mut out, ref "~")
            ret ok out
        end if
    end if
    var out = text@
    if len .< width
        call string.push_str(mut out, ref string.repeat(ref " ", width -! len))
    end if
    ret ok out
end fun

// Text padded on the left to a width, for numbers.
fun right(ref text: string, width: index): !string
    let len = string.char_count(ref text)
    if len >= width
        ret ok text@
    end if
    var out = string.repeat(ref " ", width -! len)
    call string.push_str(mut out, ref text)
    ret ok out
end fun

// A rule as wide as a table.
fun rule(width: index): string
    ret string.repeat(ref "-", width)
end fun
