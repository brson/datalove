"""The store's report computed independently, to check datalove's against.

    uv run check.py > expected.txt

Reads the same `.dlt` files `gen.py` writes and prints the report exactly as
`report.dfs` lays it out, one line per `debuglog`, quoted the way `debuglog`
quotes a string.
"""

import re
from pathlib import Path

DATA = Path(__file__).parent / "local" / "store"


def records(name):
    """Each top-level record of a data file, as the text of one line."""
    return [line.strip() for line in open(DATA / f"{name}.dlt") if line.strip().startswith("{")]


def field(record, name, pattern=r"\d+"):
    return re.search(rf"\b{name} = ({pattern})", record)[1]


products = {}
for r in records("products"):
    products[int(field(r, "id"))] = {
        "name": field(r, "name", r'"[^"]*"')[1:-1],
        "category": field(r, "category", r'"[^"]*"')[1:-1],
        "price": int(field(r, "price")),
        "cost": int(field(r, "cost")),
    }

customers = {}
customer_order = []
for r in records("customers"):
    cid = int(field(r, "id"))
    customer_order.append(cid)
    customers[cid] = {
        "name": field(r, "name", r'"[^"]*"')[1:-1],
        "city": field(r, "city", r'"[^"]*"')[1:-1],
        "country": field(r, "country", r'"[^"]*"')[1:-1],
        "tier": field(r, "tier", r"atom \w+").split()[1].lower(),
    }

orders = []
for r in records("orders"):
    orders.append({
        "customer": int(field(r, "customer")),
        "day": int(field(r, "day")),
        "status": field(r, "status", r"atom \w+").split()[1],
        "lines": [(int(p), int(q), int(d)) for p, q, d in
                  re.findall(r"product = (\d+), qty = (\d+), discount = (\d+)", r)],
    })


def is_sale(o):
    return o["status"] in ("Delivered", "Shipped")


def line_revenue(line):
    product, qty, discount = line
    return products[product]["price"] * qty * (100 - discount) // 100


def line_cost(line):
    product, qty, _ = line
    return products[product]["cost"] * qty


def order_revenue(o):
    return sum(line_revenue(l) for l in o["lines"])


# Formatting, as `fmt.dfm` does it.

def count(n):
    return f"{n:,}"


def money(cents):
    return f"${cents // 100:,}.{cents % 100:02}"


def percent(part, whole):
    if whole == 0:
        return "-"
    tenths = (part * 1000 + whole // 2) // whole
    return f"{tenths // 10}.{tenths % 10}%"


def ratio(num, den):
    if den == 0:
        return "-"
    tenths = (num * 10 + den // 2) // den
    return f"{tenths // 10}.{tenths % 10}"


def left(text, width):
    if len(text) > width:
        return text[:width - 1] + "~"
    return text.ljust(width)


def right(text, width):
    return text.rjust(width)


def row(cells, widths, texts=1):
    return "  ".join(left(c, w) if i < texts else right(c, w)
                     for i, (c, w) in enumerate(zip(cells, widths)))


def heading(title):
    return ["", title, "-" * len(title)]


def descending(rows):
    return sorted(rows, reverse=True)


out = []

# Overview.
sales = [o for o in orders if is_sale(o)]
returns = sum(1 for o in orders if o["status"] == "Returned")
units = sum(q for o in sales for _, q, _ in o["lines"])
revenue = sum(line_revenue(l) for o in sales for l in o["lines"])
cost = sum(line_cost(l) for o in sales for l in o["lines"])
margin = revenue - cost
out += heading("Overview")
w = [24, 16]
out += [row(["orders", count(len(orders))], w), row(["  sold", count(len(sales))], w),
        row(["  returned", count(returns)], w), row(["units sold", count(units)], w),
        row(["revenue", money(revenue)], w), row(["gross margin", money(margin)], w),
        row(["margin rate", percent(margin, revenue)], w),
        row(["average sale", money(revenue // len(sales))], w),
        row(["return rate", percent(returns, len(orders))], w)]

# Categories.
by_cat = {}
for o in sales:
    for l in o["lines"]:
        c = products[l[0]]["category"]
        r, k, u = by_cat.get(c, (0, 0, 0))
        by_cat[c] = (r + line_revenue(l), k + line_cost(l), u + l[1])
ret = {}
for o in orders:
    for l in o["lines"]:
        c = products[l[0]]["category"]
        rt, sh = ret.get(c, (0, 0))
        if o["status"] == "Returned":
            rt += 1
        if o["status"] == "Returned" or is_sale(o):
            sh += 1
        ret[c] = (rt, sh)
out += heading("Categories")
w = [12, 16, 8, 8, 10]
out.append(row(["category", "revenue", "share", "margin", "returns"], w))
for r, c, k, u in descending([(r, c, k, u) for c, (r, k, u) in by_cat.items()]):
    rt, sh = ret.get(c, (0, 0))
    out.append(row([c, money(r), percent(r, revenue), percent(r - k, r), percent(rt, sh)], w))

# Months.
by_month = {}
for o in sales:
    m = o["day"] // 30
    r, s = by_month.get(m, (0, 0))
    by_month[m] = (r + order_revenue(o), s + 1)
out += heading("Months")
w = [8, 16, 8, 10]
out.append(row(["month", "revenue", "sales", "change"], w))
previous = 0
for m in sorted(by_month):
    r, s = by_month[m]
    if previous == 0:
        change = "-"
    elif r >= previous:
        change = "+" + percent(r - previous, previous)
    else:
        change = "-" + percent(previous - r, previous)
    out.append(row([f"m{m + 1}", money(r), count(s), change], w))
    previous = r

# Best sellers.
units_by, rev_by = {}, {}
for o in sales:
    for l in o["lines"]:
        units_by[l[0]] = units_by.get(l[0], 0) + l[1]
        rev_by[l[0]] = rev_by.get(l[0], 0) + line_revenue(l)
out += heading("Best sellers")
w = [30, 10, 8, 14]
out.append(row(["product", "category", "units", "revenue"], w, 2))
for u, p, r in descending([(units_by[p], p, rev_by[p]) for p in units_by])[:10]:
    out.append(row([products[p]["name"], products[p]["category"], count(u), money(r)], w, 2))

# Best customers.
spent, norders = {}, {}
for o in sales:
    spent[o["customer"]] = spent.get(o["customer"], 0) + order_revenue(o)
    norders[o["customer"]] = norders.get(o["customer"], 0) + 1
out += heading("Best customers")
w = [20, 12, 7, 14]
out.append(row(["customer", "city", "orders", "spent"], w, 2))
for s, c, n in descending([(spent[c], c, norders[c]) for c in spent])[:10]:
    out.append(row([customers[c]["name"], customers[c]["city"], count(n), money(s)], w, 2))
visits = sorted((o["customer"], o["day"]) for o in sales)
buyers = repeat = gap_days = gaps = 0
i = 0
while i < len(visits):
    j = i + 1
    while j < len(visits) and visits[j][0] == visits[i][0]:
        gap_days += visits[j][1] - visits[j - 1][1]
        gaps += 1
        j += 1
    buyers += 1
    if j - i > 1:
        repeat += 1
    i = j
out.append("")
w = [36, 10]
out += [row(["customers who bought", count(buyers)], w),
        row(["  of whom came back", percent(repeat, buyers)], w),
        row(["days between orders, on average", ratio(gap_days, gaps)], w)]

# Countries.
c_rev, c_buyers = {}, {}
for o in sales:
    country = customers[o["customer"]]["country"]
    c_rev[country] = c_rev.get(country, 0) + order_revenue(o)
    c_buyers.setdefault(country, set()).add(o["customer"])
out += heading("Countries")
w = [8, 16, 8, 14]
out.append(row(["country", "revenue", "buyers", "per buyer"], w))
for r, c, b in descending([(c_rev[c], c, len(c_buyers[c])) for c in c_rev]):
    out.append(row([c, money(r), count(b), money(r // b)], w))

# Tiers.
members, t_spent, t_buying, seen = {}, {}, {}, set()
for c in customer_order:
    t = customers[c]["tier"]
    members[t] = members.get(t, 0) + 1
for o in sales:
    t = customers[o["customer"]]["tier"]
    t_spent[t] = t_spent.get(t, 0) + order_revenue(o)
    if o["customer"] not in seen:
        seen.add(o["customer"])
        t_buying[t] = t_buying.get(t, 0) + 1
out += heading("Tiers")
w = [8, 10, 10, 16, 14]
out.append(row(["tier", "members", "bought", "spent", "per buyer"], w))
for t in ["gold", "silver", "bronze"]:
    m, b, s = members.get(t, 0), t_buying.get(t, 0), t_spent.get(t, 0)
    out.append(row([t, count(m), percent(b, m), money(s), money(s // b)], w))

# Bought together.
together = {}
for o in sales:
    ls = o["lines"]
    for a in range(len(ls)):
        for b in range(a + 1, len(ls)):
            pair = tuple(sorted((ls[a][0], ls[b][0])))
            together[pair] = together.get(pair, 0) + 1
multi = sum(1 for o in sales if len(o["lines"]) > 1)
out += heading("Bought together")
w = [30, 30, 6]
out.append(row(["product", "with", "times"], w, 2))
for n, a, b in descending([(n, a, b) for (a, b), n in together.items()])[:10]:
    out.append(row([products[a]["name"], products[b]["name"], count(n)], w, 2))
out.append("")
w = [36, 10]
out.append(row(["sales of more than one product", percent(multi, len(sales))], w))

for line in out:
    print('"' + line.replace("\\", "\\\\").replace('"', '\\"') + '"')
