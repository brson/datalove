// The store's data, and the one place that knows its shape.
//
// A type alias cannot be named from another module, so the rest of the store
// reaches the data through these functions rather than through the types.
// Money is in cents throughout.

require module sys/std/list
require module sys/std/map
require module sys/std/u64

type Product: {id: u32, name: string, category: string, price: u32, cost: u32}
type Tier: enum { atom Bronze, atom Silver, atom Gold }
type Customer: {id: u32, name: string, city: string, country: string, joined: u32, tier: Tier}
type Status: enum { atom Delivered, atom Shipped, atom Returned, atom Cancelled }
type Line: {product: u32, qty: u32, discount: u32}
type Order: {id: u32, customer: u32, day: u32, status: Status, lines: [Line]}

require data local/store/products: [Product]
require data local/store/customers: [Customer]
require data local/store/orders: [Order]

// Where each product and customer is, by id. Worked out at compile time from
// the data, so a lookup is one map probe.
const PRODUCT_SLOTS: %{u32 = index} = product_slots()
const CUSTOMER_SLOTS: %{u32 = index} = customer_slots()

fun next(i: index): index
    ret icall add_wrapping_index(i, : index / 1)
end fun

// A const cannot early-return, so the result is unwrapped here. An error from
// the build would be a bug in this module, and there is no panic to say so
// with, so it gives an empty index, which every lookup then reports.
fun product_slots(): %{u32 = index}
    if build_product_slots() |slots|
        ret slots
    else |e|
        ret %{}
    end if
end fun

fun build_product_slots(): !%{u32 = index}
    var slots: %{u32 = index} = %{}
    var i: index = 0
    loop while i .< list.len(ref products)
        call map.insert(mut slots, products[i]!.id, i)
        set i = next(i)
    end loop
    ret ok slots
end fun

fun customer_slots(): %{u32 = index}
    if build_customer_slots() |slots|
        ret slots
    else |e|
        ret %{}
    end if
end fun

fun build_customer_slots(): !%{u32 = index}
    var slots: %{u32 = index} = %{}
    var i: index = 0
    loop while i .< list.len(ref customers)
        call map.insert(mut slots, customers[i]!.id, i)
        set i = next(i)
    end loop
    ret ok slots
end fun

// Products.

fun product_count(): index
    ret list.len(ref products)
end fun

fun product_id_at(slot: index): !u32
    ret ok products[slot]!.id
end fun

fun product_slot(id: u32): !index
    ret ok PRODUCT_SLOTS[id]!
end fun

fun product_name(id: u32): !string
    ret ok products[product_slot(id)!]!.name@
end fun

fun product_category(id: u32): !string
    ret ok products[product_slot(id)!]!.category@
end fun

fun product_price(id: u32): !u64
    ret ok products[product_slot(id)!]!.price@
end fun

fun product_cost(id: u32): !u64
    ret ok products[product_slot(id)!]!.cost@
end fun

// Customers.

fun customer_count(): index
    ret list.len(ref customers)
end fun

fun customer_id_at(slot: index): !u32
    ret ok customers[slot]!.id
end fun

fun customer_slot(id: u32): !index
    ret ok CUSTOMER_SLOTS[id]!
end fun

fun customer_name(id: u32): !string
    ret ok customers[customer_slot(id)!]!.name@
end fun

fun customer_country(id: u32): !string
    ret ok customers[customer_slot(id)!]!.country@
end fun

fun customer_city(id: u32): !string
    ret ok customers[customer_slot(id)!]!.city@
end fun

fun customer_joined(id: u32): !u32
    ret ok customers[customer_slot(id)!]!.joined
end fun

fun customer_tier(id: u32): !string
    match customers[customer_slot(id)!]!.tier
    case atom Bronze
        ret ok "bronze"
    case atom Silver
        ret ok "silver"
    case atom Gold
        ret ok "gold"
    end match
end fun

// Orders.

fun order_count(): index
    ret list.len(ref orders)
end fun

fun order_id(o: index): !u32
    ret ok orders[o]!.id
end fun

fun order_customer(o: index): !u32
    ret ok orders[o]!.customer
end fun

fun order_day(o: index): !u32
    ret ok orders[o]!.day
end fun

fun line_count(o: index): !index
    ret ok list.len(ref orders[o]!.lines)
end fun

// Whether an order is revenue: it went out and did not come back.
fun is_sale(o: index): !bool
    match orders[o]!.status
    case atom Delivered
        ret ok true
    case atom Shipped
        ret ok true
    case atom Returned
        ret ok false
    case atom Cancelled
        ret ok false
    end match
end fun

fun is_returned(o: index): !bool
    match orders[o]!.status
    case atom Returned
        ret ok true
    case atom Delivered
        ret ok false
    case atom Shipped
        ret ok false
    case atom Cancelled
        ret ok false
    end match
end fun

fun line_product(o: index, l: index): !u32
    ret ok orders[o]!.lines[l]!.product
end fun

fun line_qty(o: index, l: index): !u32
    ret ok orders[o]!.lines[l]!.qty
end fun

// What a line brought in, after its discount, rounded down to the cent.
fun line_revenue(o: index, l: index): !u64
    let price = product_price(orders[o]!.lines[l]!.product)!
    let qty: u64 = orders[o]!.lines[l]!.qty@
    let kept: u64 = (: u32 / 100 -! orders[o]!.lines[l]!.discount)@
    ret ok (price *! qty *! kept /! 100)
end fun

// What a line's goods cost the store.
fun line_cost(o: index, l: index): !u64
    let cost = product_cost(orders[o]!.lines[l]!.product)!
    let qty: u64 = orders[o]!.lines[l]!.qty@
    ret ok (cost *! qty)
end fun
