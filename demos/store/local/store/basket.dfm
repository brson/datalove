// What sells together: pairs of products bought in the same order.
//
// The heaviest query in the report, as every pair of lines in every sale is
// counted.

require module sys/std/list
require module sys/std/map
require module sys/std/ord
require module local/store/db
require module local/store/sales

// The pairs bought together most often, with how often, the lesser id first.
fun top_pairs(n: index): ![(u64, u32, u32)]
    var together: %{(u32, u32) = u64} = %{}
    var o: index = 0
    loop while o .< db.order_count()
        if db.is_sale(o)!
            let lines = db.line_count(o)!
            var a: index = 0
            loop while a .< lines
                let first = db.line_product(o, a)!
                var b = a +! 1
                loop while b .< lines
                    let second = db.line_product(o, b)!
                    let pair = ord.sorted_pair(first, second)
                    let _ = sales.add(mut together, ref pair, 1)!
                    set b +!= 1
                end loop
                set a +!= 1
            end loop
        end if
        set o +!= 1
    end loop

    var rows: [(u64, u32, u32)] = []
    let pairs = map.keys(ref together)
    var i: index = 0
    loop while i .< list.len(ref pairs)
        let pair = pairs[i]!
        call list.push(mut rows, (map.get_or(ref together, ref pair, 0), pair.0, pair.1))
        set i +!= 1
    end loop
    ret ok list.take(ref list.reversed(ref ord.sorted(ref rows)), n)
end fun

// How many sales held more than one product, of all sales.
fun multi_item(): !(u64, u64)
    var multi: u64 = 0
    var all: u64 = 0
    var o: index = 0
    loop while o .< db.order_count()
        if db.is_sale(o)!
            set all +!= 1
            if db.line_count(o)! .> 1
                set multi +!= 1
            end if
        end if
        set o +!= 1
    end loop
    ret ok (multi, all)
end fun
