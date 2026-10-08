// Who buys: the best customers, where they are, and how often they come back.

require module sys/std/list
require module sys/std/map
require module sys/std/ord
require module sys/std/set
require module sys/std/u64
require module local/store/db
require module local/store/sales

// The customers who spent the most, with what they spent and over how many
// orders.
fun top_customers(n: index): ![(u64, u32, u64)]
    var spent: %{u32 = u64} = %{}
    var orders: %{u32 = u64} = %{}
    var o: index = 0
    loop while o .< db.order_count()
        if db.is_sale(o)!
            let customer = db.order_customer(o)!
            let _ = sales.add(mut spent, customer, sales.order_revenue(o)!)!
            let _ = sales.add(mut orders, customer, 1)!
        end if
        set o +!= 1
    end loop

    var rows: [(u64, u32, u64)] = []
    let customers = map.keys(ref spent)
    var i: index = 0
    loop while i .< list.len(ref customers)
        let customer = customers[i]!
        call list.push(mut rows, (map.get_or(ref spent, ref customer, 0), customer, map.get_or(ref orders, ref customer, 0)))
        set i +!= 1
    end loop
    ret ok list.take(ref list.reversed(ref ord.sorted(ref rows)), n)
end fun

// Revenue and buying customers by country, the biggest first.
fun by_country(): ![(u64, string, u64)]
    var revenue: %{string = u64} = %{}
    var buyers: %{string = #{u32}} = %{}
    var o: index = 0
    loop while o .< db.order_count()
        if db.is_sale(o)!
            let customer = db.order_customer(o)!
            let country = db.customer_country(customer)!
            let _ = sales.add(mut revenue, country@, sales.order_revenue(o)!)!
            // Added to where it is, since reading the set out and putting it
            // back copied and freed the whole of it for every sale.
            if not (map.contains_key(ref buyers, ref country))
                set buyers[country@] = #{}
            end if
            let _ = set.insert(mut buyers[country]!, customer)
        end if
        set o +!= 1
    end loop

    var rows: [(u64, string, u64)] = []
    let countries = map.keys(ref revenue)
    var i: index = 0
    loop while i .< list.len(ref countries)
        let country = countries[i]!@
        let r = map.get_or(ref revenue, ref country, 0)
        let b = set.len(ref buyers[country]!)
        call list.push(mut rows, (r, country, u64.from_index(b)))
        set i +!= 1
    end loop
    ret ok list.reversed(ref ord.sorted(ref rows))
end fun

// For each tier: customers in it, how many bought, and what they spent.
fun by_tier(): ![(string, u64, u64, u64)]
    var members: %{string = u64} = %{}
    var c: index = 0
    loop while c .< db.customer_count()
        let _ = sales.add(mut members, db.customer_tier(db.customer_id_at(c)!)!, 1)!
        set c +!= 1
    end loop

    var spent: %{string = u64} = %{}
    var buyers: %{u32 = bool} = %{}
    var buying: %{string = u64} = %{}
    var o: index = 0
    loop while o .< db.order_count()
        if db.is_sale(o)!
            let customer = db.order_customer(o)!
            let tier = db.customer_tier(customer)!
            let _ = sales.add(mut spent, tier@, sales.order_revenue(o)!)!
            if map.insert_if_absent(mut buyers, customer, true)
                let _ = sales.add(mut buying, tier@, 1)!
            end if
        end if
        set o +!= 1
    end loop

    var rows: [(string, u64, u64, u64)] = []
    let tiers = ["gold", "silver", "bronze"]
    var i: index = 0
    loop while i .< list.len(ref tiers)
        let tier = tiers[i]!@
        let m = map.get_or(ref members, ref tier, 0)
        let b = map.get_or(ref buying, ref tier, 0)
        let s = map.get_or(ref spent, ref tier, 0)
        call list.push(mut rows, (tier, m, b, s))
        set i +!= 1
    end loop
    ret ok rows
end fun

// How customers come back: those who bought, those who bought more than
// once, and the days between one order and the next, summed and counted.
fun loyalty(): !(u64, u64, u64, u64)
    // Every sale as (customer, day), sorted, puts each customer's orders
    // together and in the order they came.
    var visits: [(u32, u32)] = []
    var o: index = 0
    loop while o .< db.order_count()
        if db.is_sale(o)!
            call list.push(mut visits, (db.order_customer(o)!, db.order_day(o)!))
        end if
        set o +!= 1
    end loop
    let visits = ord.sorted(ref visits)

    var buyers: u64 = 0
    var repeat: u64 = 0
    var gap_days: u64 = 0
    var gaps: u64 = 0
    var i: index = 0
    loop while i .< list.len(ref visits)
        let customer = visits[i]!.0
        let first = i
        var j = i +! 1
        loop while j .< list.len(ref visits)
            if visits[j]!.0 != customer
                break
            end if
            let gap: u64 = (visits[j]!.1 -! visits[j -! 1]!.1)@
            set gap_days +!= gap
            set gaps +!= 1
            set j +!= 1
        end loop
        set buyers +!= 1
        if j -! first .> 1
            set repeat +!= 1
        end if
        set i = j
    end loop
    ret ok (buyers, repeat, gap_days, gaps)
end fun
