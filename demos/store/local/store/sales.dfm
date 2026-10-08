// What sold: totals, and revenue broken down by product, category and month.
//
// An order counts as a sale when it went out and stayed out (`db.is_sale`).
// Revenue is after discount; margin is revenue less what the goods cost.

require module sys/std/list
require module sys/std/map
require module sys/std/ord
require module sys/std/u64
require module local/store/db

// The store's totals: orders, of which sales and returns, then units sold,
// revenue and cost.
fun overview(): !(u64, u64, u64, u64, u64, u64)
    var sales: u64 = 0
    var returns: u64 = 0
    var units: u64 = 0
    var revenue: u64 = 0
    var cost: u64 = 0
    var o: index = 0
    loop while o .< db.order_count()
        if db.is_returned(o)!
            set returns +!= 1
        end if
        if db.is_sale(o)!
            set sales +!= 1
            var l: index = 0
            loop while l .< db.line_count(o)!
                let qty: u64 = db.line_qty(o, l)!@
                set units +!= qty
                set revenue +!= db.line_revenue(o, l)!
                set cost +!= db.line_cost(o, l)!
                set l +!= 1
            end loop
        end if
        set o +!= 1
    end loop
    let orders = u64.from_index(db.order_count())
    ret ok (orders, sales, returns, units, revenue, cost)
end fun

// Revenue, cost and units by category, the biggest first.
fun by_category(): ![(u64, string, u64, u64)]
    var revenue: %{string = u64} = %{}
    var cost: %{string = u64} = %{}
    var units: %{string = u64} = %{}
    var o: index = 0
    loop while o .< db.order_count()
        if db.is_sale(o)!
            var l: index = 0
            loop while l .< db.line_count(o)!
                let category = db.product_category(db.line_product(o, l)!)!
                let qty: u64 = db.line_qty(o, l)!@
                let _ = add(mut revenue, category@, db.line_revenue(o, l)!)!
                let _ = add(mut cost, category@, db.line_cost(o, l)!)!
                let _ = add(mut units, category, qty)!
                set l +!= 1
            end loop
        end if
        set o +!= 1
    end loop

    // Sorted on revenue, with the category to break ties.
    var rows: [(u64, string, u64, u64)] = []
    let categories = map.keys(ref revenue)
    var i: index = 0
    loop while i .< list.len(ref categories)
        let category = categories[i]!@
        let r = map.get_or(ref revenue, ref category, 0)
        let c = map.get_or(ref cost, ref category, 0)
        let u = map.get_or(ref units, ref category, 0)
        call list.push(mut rows, (r, category, c, u))
        set i +!= 1
    end loop
    ret ok list.reversed(ref ord.sorted(ref rows))
end fun

// Revenue and sales by month of trading, oldest first. A month is thirty days.
fun by_month(): ![(u32, u64, u64)]
    var revenue: %{u32 = u64} = %{}
    var sales: %{u32 = u64} = %{}
    var o: index = 0
    loop while o .< db.order_count()
        if db.is_sale(o)!
            let month = db.order_day(o)! /! 30
            let _ = add(mut revenue, month, order_revenue(o)!)!
            let _ = add(mut sales, month, 1)!
        end if
        set o +!= 1
    end loop

    var rows: [(u32, u64, u64)] = []
    let months = map.keys(ref revenue)
    var i: index = 0
    loop while i .< list.len(ref months)
        let month = months[i]!
        call list.push(mut rows, (month, map.get_or(ref revenue, ref month, 0), map.get_or(ref sales, ref month, 0)))
        set i +!= 1
    end loop
    ret ok rows
end fun

// The products that sold the most units, with the units and the revenue.
fun top_products(n: index): ![(u64, u32, u64)]
    var units: %{u32 = u64} = %{}
    var revenue: %{u32 = u64} = %{}
    var o: index = 0
    loop while o .< db.order_count()
        if db.is_sale(o)!
            var l: index = 0
            loop while l .< db.line_count(o)!
                let product = db.line_product(o, l)!
                let qty: u64 = db.line_qty(o, l)!@
                let _ = add(mut units, product, qty)!
                let _ = add(mut revenue, product, db.line_revenue(o, l)!)!
                set l +!= 1
            end loop
        end if
        set o +!= 1
    end loop

    var rows: [(u64, u32, u64)] = []
    let products = map.keys(ref units)
    var i: index = 0
    loop while i .< list.len(ref products)
        let product = products[i]!
        call list.push(mut rows, (map.get_or(ref units, ref product, 0), product, map.get_or(ref revenue, ref product, 0)))
        set i +!= 1
    end loop
    ret ok list.take(ref list.reversed(ref ord.sorted(ref rows)), n)
end fun

// Lines returned and lines shipped, by category.
fun returns_by_category(): ![(string, u64, u64)]
    var returned: %{string = u64} = %{}
    var shipped: %{string = u64} = %{}
    var o: index = 0
    loop while o .< db.order_count()
        let is_return = db.is_returned(o)!
        let is_sale = db.is_sale(o)!
        var l: index = 0
        loop while l .< db.line_count(o)!
            let category = db.product_category(db.line_product(o, l)!)!
            if is_return
                let _ = add(mut returned, category@, 1)!
            end if
            if is_return or is_sale
                let _ = add(mut shipped, category@, 1)!
            end if
            set l +!= 1
        end loop
        set o +!= 1
    end loop

    var rows: [(string, u64, u64)] = []
    let categories = map.keys(ref shipped)
    var i: index = 0
    loop while i .< list.len(ref categories)
        let category = categories[i]!@
        let r = map.get_or(ref returned, ref category, 0)
        let s = map.get_or(ref shipped, ref category, 0)
        call list.push(mut rows, (category, r, s))
        set i +!= 1
    end loop
    ret ok rows
end fun

// What an order brought in, line by line.
fun order_revenue(o: index): !u64
    var total: u64 = 0
    var l: index = 0
    loop while l .< db.line_count(o)!
        set total +!= db.line_revenue(o, l)!
        set l +!= 1
    end loop
    ret ok total
end fun

// Add to a running total under a key, starting it at nothing.
fun add<K>(mut totals: %{K = u64}, key: K, amount: u64): !() with { K is ord, }
    let seen = map.get_or(ref totals, ref key, 0)
    call map.insert(mut totals, key, seen +! amount)
    ret ok ()
end fun
