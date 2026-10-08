// The report, as lines of text, section by section.

require module sys/std/list
require module sys/std/string
require module sys/std/u64
require module local/store/db
require module local/store/fmt
require module local/store/sales
require module local/store/people
require module local/store/basket

fun lines(): ![string]
    var out: [string] = []
    call list.extend(mut out, overview()!)
    call list.extend(mut out, categories()!)
    call list.extend(mut out, months()!)
    call list.extend(mut out, products()!)
    call list.extend(mut out, customers()!)
    call list.extend(mut out, countries()!)
    call list.extend(mut out, tiers()!)
    call list.extend(mut out, baskets()!)
    ret ok out
end fun

fun heading(ref title: string): [string]
    ret ["", title@, fmt.rule(string.char_count(ref title))]
end fun

// A row of cells, the first on the left and the rest, numbers, on the right.
fun row(ref cells: [string], ref widths: [index]): !string
    ret text_row(ref cells, ref widths, 1)
end fun

// A row whose first `texts` cells are text, on the left, and the rest numbers.
fun text_row(ref cells: [string], ref widths: [index], texts: index): !string
    var out = ""
    var i: index = 0
    loop while i .< list.len(ref cells)
        if i .> 0
            call string.push_str(mut out, ref "  ")
        end if
        if i .< texts
            call string.push_str(mut out, ref fmt.left(ref cells[i]!, widths[i]!)!)
        else
            call string.push_str(mut out, ref fmt.right(ref cells[i]!, widths[i]!)!)
        end if
        set i +!= 1
    end loop
    ret ok out
end fun

fun overview(): ![string]
    let totals = sales.overview()!
    let margin = totals.4 -! totals.5
    var out = heading(ref "Overview")
    let widths: [index] = [24, 16]
    call list.push(mut out, row(ref ["orders", fmt.count(totals.0)!], ref widths)!)
    call list.push(mut out, row(ref ["  sold", fmt.count(totals.1)!], ref widths)!)
    call list.push(mut out, row(ref ["  returned", fmt.count(totals.2)!], ref widths)!)
    call list.push(mut out, row(ref ["units sold", fmt.count(totals.3)!], ref widths)!)
    call list.push(mut out, row(ref ["revenue", fmt.money(totals.4)!], ref widths)!)
    call list.push(mut out, row(ref ["gross margin", fmt.money(margin)!], ref widths)!)
    call list.push(mut out, row(ref ["margin rate", fmt.percent(margin, totals.4)!], ref widths)!)
    call list.push(mut out, row(ref ["average sale", fmt.money(totals.4 /! totals.1)!], ref widths)!)
    call list.push(mut out, row(ref ["return rate", fmt.percent(totals.2, totals.0)!], ref widths)!)
    ret ok out
end fun

fun categories(): ![string]
    let rows = sales.by_category()!
    let returns = sales.returns_by_category()!
    var out = heading(ref "Categories")
    let widths: [index] = [12, 16, 8, 8, 10]
    call list.push(mut out, row(ref ["category", "revenue", "share", "margin", "returns"], ref widths)!)
    let total = sales.overview()!.4
    var i: index = 0
    loop while i .< list.len(ref rows)
        let revenue = rows[i]!.0
        let category = rows[i]!.1@
        let margin = revenue -! rows[i]!.2
        var returned: u64 = 0
        var shipped: u64 = 0
        var j: index = 0
        loop while j .< list.len(ref returns)
            if string.eq(ref returns[j]!.0, ref category)
                set returned = returns[j]!.1
                set shipped = returns[j]!.2
            end if
            set j +!= 1
        end loop
        call list.push(mut out, row(ref [
            category,
            fmt.money(revenue)!,
            fmt.percent(revenue, total)!,
            fmt.percent(margin, revenue)!,
            fmt.percent(returned, shipped)!,
        ], ref widths)!)
        set i +!= 1
    end loop
    ret ok out
end fun

// Revenue by month, each with its change on the month before.
fun months(): ![string]
    let rows = sales.by_month()!
    var out = heading(ref "Months")
    let widths: [index] = [8, 16, 8, 10]
    call list.push(mut out, row(ref ["month", "revenue", "sales", "change"], ref widths)!)
    var previous: u64 = 0
    var i: index = 0
    loop while i .< list.len(ref rows)
        let month: u64 = rows[i]!.0@
        let revenue = rows[i]!.1
        var label = "m"
        call string.push_str(mut label, ref fmt.digits(month +! 1))
        call list.push(mut out, row(ref [
            label,
            fmt.money(revenue)!,
            fmt.count(rows[i]!.2)!,
            change(previous, revenue)!,
        ], ref widths)!)
        set previous = revenue
        set i +!= 1
    end loop
    ret ok out
end fun

// The change from one amount to the next, as a signed percentage.
fun change(before: u64, after: u64): !string
    if before == 0
        ret ok "-"
    end if
    if after >= before
        var out = "+"
        call string.push_str(mut out, ref fmt.percent(after -! before, before)!)
        ret ok out
    end if
    var out = "-"
    call string.push_str(mut out, ref fmt.percent(before -! after, before)!)
    ret ok out
end fun

fun products(): ![string]
    let rows = sales.top_products(10)!
    var out = heading(ref "Best sellers")
    let widths: [index] = [30, 10, 8, 14]
    call list.push(mut out, text_row(ref ["product", "category", "units", "revenue"], ref widths, 2)!)
    var i: index = 0
    loop while i .< list.len(ref rows)
        let product = rows[i]!.1
        call list.push(mut out, text_row(ref [
            db.product_name(product)!,
            db.product_category(product)!,
            fmt.count(rows[i]!.0)!,
            fmt.money(rows[i]!.2)!,
        ], ref widths, 2)!)
        set i +!= 1
    end loop
    ret ok out
end fun

fun customers(): ![string]
    let rows = people.top_customers(10)!
    let loyalty = people.loyalty()!
    var out = heading(ref "Best customers")
    let widths: [index] = [20, 12, 7, 14]
    call list.push(mut out, text_row(ref ["customer", "city", "orders", "spent"], ref widths, 2)!)
    var i: index = 0
    loop while i .< list.len(ref rows)
        let customer = rows[i]!.1
        call list.push(mut out, text_row(ref [
            db.customer_name(customer)!,
            db.customer_city(customer)!,
            fmt.count(rows[i]!.2)!,
            fmt.money(rows[i]!.0)!,
        ], ref widths, 2)!)
        set i +!= 1
    end loop
    call list.push(mut out, "")
    let widths: [index] = [36, 10]
    call list.push(mut out, row(ref ["customers who bought", fmt.count(loyalty.0)!], ref widths)!)
    call list.push(mut out, row(ref ["  of whom came back", fmt.percent(loyalty.1, loyalty.0)!], ref widths)!)
    call list.push(mut out, row(ref ["days between orders, on average", fmt.ratio(loyalty.2, loyalty.3)!], ref widths)!)
    ret ok out
end fun

fun countries(): ![string]
    let rows = people.by_country()!
    var out = heading(ref "Countries")
    let widths: [index] = [8, 16, 8, 14]
    call list.push(mut out, row(ref ["country", "revenue", "buyers", "per buyer"], ref widths)!)
    var i: index = 0
    loop while i .< list.len(ref rows)
        call list.push(mut out, row(ref [
            rows[i]!.1@,
            fmt.money(rows[i]!.0)!,
            fmt.count(rows[i]!.2)!,
            fmt.money(rows[i]!.0 /! rows[i]!.2)!,
        ], ref widths)!)
        set i +!= 1
    end loop
    ret ok out
end fun

fun tiers(): ![string]
    let rows = people.by_tier()!
    var out = heading(ref "Tiers")
    let widths: [index] = [8, 10, 10, 16, 14]
    call list.push(mut out, row(ref ["tier", "members", "bought", "spent", "per buyer"], ref widths)!)
    var i: index = 0
    loop while i .< list.len(ref rows)
        call list.push(mut out, row(ref [
            rows[i]!.0@,
            fmt.count(rows[i]!.1)!,
            fmt.percent(rows[i]!.2, rows[i]!.1)!,
            fmt.money(rows[i]!.3)!,
            fmt.money(rows[i]!.3 /! rows[i]!.2)!,
        ], ref widths)!)
        set i +!= 1
    end loop
    ret ok out
end fun

fun baskets(): ![string]
    let pairs = basket.top_pairs(10)!
    let multi = basket.multi_item()!
    var out = heading(ref "Bought together")
    let widths: [index] = [30, 30, 6]
    call list.push(mut out, text_row(ref ["product", "with", "times"], ref widths, 2)!)
    var i: index = 0
    loop while i .< list.len(ref pairs)
        call list.push(mut out, text_row(ref [
            db.product_name(pairs[i]!.1)!,
            db.product_name(pairs[i]!.2)!,
            fmt.count(pairs[i]!.0)!,
        ], ref widths, 2)!)
        set i +!= 1
    end loop
    call list.push(mut out, "")
    let widths: [index] = [36, 10]
    call list.push(mut out, row(ref ["sales of more than one product", fmt.percent(multi.0, multi.1)!], ref widths)!)
    ret ok out
end fun
