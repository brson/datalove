"""Generate the store's data: products, customers and orders, as datalit.

    uv run gen.py [ORDERS]

Writes `local/store/{products,customers,orders}.dlt`. The same order count
always gives the same data. Popularity is skewed the way a real store's is: a
few products and customers account for much of the business.
"""

import random
import sys
from pathlib import Path

CATEGORIES = {
    "kitchen": (["Steel", "Cast Iron", "Bamboo", "Ceramic", "Copper"],
                ["Skillet", "Kettle", "Cutting Board", "Mixing Bowl", "Ladle", "Colander"]),
    "garden": (["Cedar", "Galvanized", "Folding", "Heavy Duty", "Compact"],
               ["Planter", "Trowel", "Hose Reel", "Pruner", "Wheelbarrow", "Rake"]),
    "outdoor": (["Waterproof", "Ultralight", "Insulated", "Trail", "Alpine"],
                ["Tent", "Backpack", "Headlamp", "Sleeping Bag", "Stove", "Water Filter"]),
    "office": (["Ergonomic", "Walnut", "Minimal", "Adjustable", "Wireless"],
               ["Desk Lamp", "Monitor Stand", "Keyboard", "Notebook", "Chair Mat", "Pen Set"]),
    "toys": (["Wooden", "Magnetic", "Plush", "Glow", "Classic"],
             ["Blocks", "Train Set", "Puzzle", "Kite", "Robot", "Marble Run"]),
    "audio": (["Studio", "Portable", "Noise Cancelling", "Bluetooth", "Vintage"],
              ["Headphones", "Speaker", "Turntable", "Microphone", "Amplifier", "Earbuds"]),
    "bath": (["Organic", "Linen", "Lavender", "Teak", "Marble"],
             ["Towel Set", "Soap Dish", "Bath Mat", "Shower Caddy", "Robe", "Diffuser"]),
    "pets": (["Orthopedic", "Chew Proof", "Reflective", "Washable", "Cozy"],
             ["Dog Bed", "Leash", "Cat Tree", "Feeder", "Carrier", "Toy Bundle"]),
}

# Price ranges in cents, by category.
PRICES = {
    "kitchen": (899, 15999), "garden": (499, 24999), "outdoor": (1299, 49999),
    "office": (399, 39999), "toys": (599, 8999), "audio": (1999, 89999),
    "bath": (699, 12999), "pets": (799, 19999),
}

CITIES = [
    ("Portland", "US"), ("Austin", "US"), ("Chicago", "US"), ("Denver", "US"),
    ("Boston", "US"), ("Seattle", "US"), ("Toronto", "CA"), ("Vancouver", "CA"),
    ("Montreal", "CA"), ("London", "GB"), ("Manchester", "GB"), ("Edinburgh", "GB"),
    ("Berlin", "DE"), ("Hamburg", "DE"), ("Munich", "DE"), ("Lyon", "FR"),
    ("Paris", "FR"), ("Madrid", "ES"), ("Lisbon", "PT"), ("Melbourne", "AU"),
    ("Sydney", "AU"), ("Auckland", "NZ"), ("Osaka", "JP"), ("Tokyo", "JP"),
]

FIRST = ["Ada", "Ben", "Chloe", "Dev", "Elena", "Femi", "Grace", "Hiro", "Ines", "Jonas",
         "Kai", "Lena", "Mateo", "Nora", "Omar", "Priya", "Quinn", "Rosa", "Sam", "Tara",
         "Uma", "Viktor", "Wen", "Ximena", "Yusuf", "Zoe"]
LAST = ["Abara", "Berg", "Costa", "Dubois", "Eriksen", "Fischer", "Garcia", "Haddad",
        "Ito", "Jensen", "Kowalski", "Lopez", "Moreau", "Nakamura", "Okafor", "Patel",
        "Quist", "Rossi", "Silva", "Tanaka", "Ulrich", "Varga", "Weber", "Young", "Zhang"]

PRODUCTS = 400
CUSTOMERS = 4000
DAYS = 720  # Two years of trading, as 24 months of 30 days.


def skewed(rng, n):
    """An index in [0, n), small ones far more often: the smaller of three draws."""
    return min(rng.randrange(n), rng.randrange(n), rng.randrange(n))


def quote(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def products(rng):
    rows = []
    ids = rng.sample(range(1000, 9999), PRODUCTS)
    for i, pid in enumerate(sorted(ids)):
        category = list(CATEGORIES)[i % len(CATEGORIES)]
        adjectives, nouns = CATEGORIES[category]
        model = f"{chr(ord('A') + rng.randrange(26))}{rng.randrange(10, 99)}"
        name = f"{rng.choice(adjectives)} {rng.choice(nouns)} {model}"
        low, high = PRICES[category]
        price = rng.randrange(low, high)
        cost = price * rng.randrange(35, 80) // 100
        rows.append((pid, name, category, price, cost))
    return rows


def customers(rng):
    rows = []
    ids = rng.sample(range(100000, 999999), CUSTOMERS)
    for cid in sorted(ids):
        name = f"{rng.choice(FIRST)} {rng.choice(LAST)}"
        city, country = CITIES[skewed(rng, len(CITIES))]
        joined = rng.randrange(DAYS)
        tier = rng.choices(["Bronze", "Silver", "Gold"], weights=[70, 22, 8])[0]
        rows.append((cid, name, city, country, joined, tier))
    return rows


def orders(rng, count, product_rows, customer_rows):
    # Popularity is fixed per product and customer, then drawn from skewed.
    by_popularity = product_rows[:]
    rng.shuffle(by_popularity)
    shoppers = customer_rows[:]
    rng.shuffle(shoppers)
    rows = []
    for i in range(count):
        customer = shoppers[skewed(rng, len(shoppers))]
        day = rng.randrange(customer[4], DAYS)
        status = rng.choices(["Delivered", "Shipped", "Returned", "Cancelled"],
                             weights=[82, 8, 6, 4])[0]
        lines = []
        seen = set()
        for _ in range(rng.choices([1, 2, 3, 4, 5, 6], weights=[30, 28, 20, 12, 6, 4])[0]):
            product = by_popularity[skewed(rng, len(by_popularity))]
            if product[0] in seen:
                continue
            seen.add(product[0])
            qty = rng.choices([1, 2, 3, 4, 6, 10], weights=[60, 20, 9, 6, 3, 2])[0]
            discount = rng.choices([0, 5, 10, 15, 25], weights=[70, 12, 10, 5, 3])[0]
            lines.append((product[0], qty, discount))
        rows.append((50_000 + i, customer[0], day, status, lines))
    return rows


def main():
    count = int(sys.argv[1]) if len(sys.argv) > 1 else 20_000
    rng = random.Random(count)
    out = Path(__file__).parent / "local" / "store"
    out.mkdir(parents=True, exist_ok=True)

    product_rows = products(rng)
    customer_rows = customers(rng)
    order_rows = orders(rng, count, product_rows, customer_rows)

    with open(out / "products.dlt", "w") as f:
        f.write("[\n")
        for pid, name, category, price, cost in product_rows:
            f.write(f"  {{id = {pid}, name = {quote(name)}, category = {quote(category)}, "
                    f"price = {price}, cost = {cost}}},\n")
        f.write("]\n")

    with open(out / "customers.dlt", "w") as f:
        f.write("[\n")
        for cid, name, city, country, joined, tier in customer_rows:
            f.write(f"  {{id = {cid}, name = {quote(name)}, city = {quote(city)}, "
                    f"country = {quote(country)}, joined = {joined}, tier = atom {tier}}},\n")
        f.write("]\n")

    with open(out / "orders.dlt", "w") as f:
        f.write("[\n")
        for oid, customer, day, status, lines in order_rows:
            items = ", ".join(f"{{product = {p}, qty = {q}, discount = {d}}}" for p, q, d in lines)
            f.write(f"  {{id = {oid}, customer = {customer}, day = {day}, "
                    f"status = atom {status}, lines = [{items}]}},\n")
        f.write("]\n")

    print(f"{len(product_rows)} products, {len(customer_rows)} customers, {len(order_rows)} orders")


if __name__ == "__main__":
    main()
