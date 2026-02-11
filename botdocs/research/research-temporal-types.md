# Temporal Types for Datalove Datalit

**Status:** Research / design proposal.

Design space for date, time, duration, and timestamp types in datalove datalit.
All proposed types are Copy, fixed-size, no heap allocation,
nanosecond precision, proleptic Gregorian calendar, no leap seconds.

Primary reference library: [jiff](https://docs.rs/jiff).
Secondary references: TC39 Temporal proposal, Apache Arrow temporal types.

## Recommended Types

Six types in three categories.

### Civil (wall-clock, no timezone)

| Type       | Size | Align | Description                    |
|------------|------|-------|--------------------------------|
| `date`     | 4    | 2     | Calendar date (year/month/day) |
| `time`     | 8    | 4     | Wall-clock time (h/m/s/nanos)  |
| `datetime` | 12   | 4     | Date + time combined           |

These carry no timezone or offset information.
A `datetime` is a reading on a wall clock, not an absolute moment.

### Instant (absolute, UTC)

| Type        | Size | Align | Description                      |
|-------------|------|-------|----------------------------------|
| `timestamp` | 16   | 8     | Exact point in time, epoch-based |

Represents an unambiguous instant on the timeline.
Internally stored as seconds + nanoseconds since Unix epoch (UTC).

### Duration

| Type       | Size | Align | Description                              |
|------------|------|-------|------------------------------------------|
| `duration` | 16   | 8     | Flat signed elapsed nanoseconds          |
| `span`     | 32   | 4     | Calendar-aware (years/months/days/h/m/s) |

`duration` is an exact physical elapsed time.
`span` is a calendar-relative offset whose absolute length depends on context
(e.g. "1 month" from January is 31 days, from February is 28 or 29).

## Cross-Reference with Prior Art

| Datalove    | jiff               | TC39 Temporal    | Arrow                    |
|-------------|---------------------|------------------|--------------------------|
| `date`      | `civil::Date`      | `PlainDate`      | `Date32`                 |
| `time`      | `civil::Time`      | `PlainTime`      | `Time64(ns)`             |
| `datetime`  | `civil::DateTime`  | `PlainDateTime`  | `Timestamp(ns, None)`    |
| `timestamp` | `Timestamp`        | `Instant`        | `Timestamp(ns, "UTC")`   |
| `duration`  | `SignedDuration`   | --               | `Duration(ns)`           |
| `span`      | `Span`             | `Duration`       | `Interval(MonthDayNano)` |

TC39 Temporal has no flat-nanosecond duration; its `Duration` is calendar-aware
like our `span`. Arrow's `Interval(MonthDayNano)` stores months, days, and
nanoseconds as three separate i32/i64 fields, similar in spirit to `span`.

## In-Memory Layouts

```rust
#[repr(C)]
pub struct Date {
    pub year: i16,          // -32768..32767
    pub month: u8,          // 1..12
    pub day: u8,            // 1..31 (validated per month)
}

#[repr(C)]
pub struct Time {
    pub hour: u8,           // 0..23
    pub minute: u8,         // 0..59
    pub second: u8,         // 0..59
    pub _pad: u8,
    pub subsec_nanos: u32,  // 0..999_999_999
}

#[repr(C)]
pub struct DateTime {
    pub date: Date,         // 4 bytes at offset 0
    pub time: Time,         // 8 bytes at offset 4
}

#[repr(C)]
pub struct Timestamp {
    pub unix_seconds: i64,  // seconds since 1970-01-01T00:00:00Z
    pub subsec_nanos: i32,  // 0..999_999_999 (always non-negative)
}

#[repr(C)]
pub struct Duration {
    pub secs: i64,          // signed total seconds
    pub nanos: i32,         // 0..999_999_999 (sign carried by secs)
}

#[repr(C)]
pub struct Span {
    pub years: i32,
    pub months: i32,
    pub days: i32,
    pub hours: i32,
    pub minutes: i32,
    pub seconds: i32,
    pub nanos: i32,         // 0..999_999_999
    pub sign: i8,           // +1 or -1
    pub _pad: [u8; 3],
}
```

Notes on layout choices:

- `Date` uses `i16` for year, supporting astronomical year numbering
  including negative years (BCE). Range -32768..32767 covers all practical use.
- `Time` pads to 8 bytes for natural alignment of `subsec_nanos`.
- `Timestamp` and `Duration` split seconds/nanos rather than storing a
  single i128 of nanoseconds. This matches jiff's `SignedDuration` layout
  and avoids i128 alignment issues on some targets.
- `Duration.nanos` is always non-negative; the sign is carried by `secs`.
  For zero seconds, negative durations use `secs = -1` with appropriate nanos
  adjustment (matching `std::time::Duration` convention adapted for signed).
- `Span` uses a single `sign` field rather than per-field signs. See open
  questions below for alternatives.

## Literal Syntax

Keyword-prefix form, matching the existing `atom`/`term` pattern:

```datalove
date 2024-06-19
time 14:30:00
time 14:30:00.123456789
datetime 2024-06-19T14:30:00
datetime 2024-06-19T14:30:00.5
timestamp 2024-06-19T14:30:00Z
timestamp 2024-06-19T14:30:00.123456789Z
duration PT1H30M
duration -PT5S
span P1Y2M3DT4H5M6S
span -P1Y
```

The value portion after each keyword follows ISO 8601 formatting:

- `date`: `YYYY-MM-DD`
- `time`: `HH:MM:SS[.fractional]`
- `datetime`: date `T` time (no timezone suffix)
- `timestamp`: date `T` time `Z` (UTC required in output)
- `duration` / `span`: ISO 8601 duration format `P[nY][nM][nD][T[nH][nM][nS]]`

Type annotations use the same keywords:

```datalove
: date
: [timestamp]
```

Pretty-printing always uses keyword-prefix form.
Subsecond digits are trimmed to the minimum needed
(e.g. `.5` not `.500000000`).

## Timezone Handling

No timezone-aware type is proposed. Rationale:

- Datalit requires Copy types; IANA timezone names are variable-length strings.
- jiff's `Zoned` type uses `Arc` internally, explicitly not Copy.
- `timestamp` records the absolute moment; timezone is presentation-layer context.
- When a timezone is needed, compose as `{ ts: timestamp, tz: string }`.
- A future `zoned` type with interned timezone IDs could be added later
  as a purely additive change.

Timestamp literals accept UTC offsets and normalize on parse:

```datalove
timestamp 2024-06-19T14:30:00-04:00
```

parses and normalizes to:

```datalove
timestamp 2024-06-19T18:30:00Z
```

The offset is consumed during parsing and not stored.
Output always uses `Z` suffix.

## What We're Not Including

- **PlainYearMonth / PlainMonthDay** -- TC39 Temporal includes these for
  partial-date use cases (credit card expiry, recurring birthdays).
  Too niche for a core type; use structs.
- **Configurable precision** -- Arrow offers microsecond/millisecond/nanosecond
  timestamp variants. We use nanosecond everywhere; lower precision just has
  trailing zeros. Avoids combinatorial type explosion.
- **Leap seconds** -- Second range is 0..59, following jiff and TC39 Temporal.
  UTC leap seconds are silently absorbed (a leap second timestamp is
  indistinguishable from the last normal second).
- **Non-Gregorian calendars** -- Proleptic Gregorian only. TC39 Temporal
  supports pluggable calendars; we don't need this complexity.
- **Weeks in span** -- ISO 8601 allows `P2W` but mixing weeks with other
  fields creates normalization ambiguity (is 1W = 7D always?).
  Weeks are converted to days on parse.
- **Unsigned duration** -- Signed only, matching jiff's `SignedDuration`.
  Rust's `std::time::Duration` is unsigned but this is widely considered
  a design mistake for general-purpose use.

## Open Design Questions

### Duration/Span boundary

Where to draw the line between `duration` (flat nanoseconds) and `span`
(calendar-aware fields).

**Option A: Strict separation.**
`duration` only accepts hours/minutes/seconds/nanos.
Any literal mentioning days, months, or years requires `span`.
Rationale: days are not always 24 hours (DST transitions),
so `duration P3D` is misleading.

**Option B: Permissive duration.**
`duration` accepts days (converting 1 day = 86400 seconds) but not
months or years. This matches common intuition and jiff's
`SignedDuration::from_hours(24 * 3)` pattern.

**Option C: No duration type at all.**
Use `span` for everything. Simplifies the type system but loses the
semantic distinction between "exact elapsed time" and "calendar offset."

### Span total ordering

`span` values have no natural total order because their absolute length
depends on a reference point (P1M could be 28-31 days).

**Option A: Not orderable.**
`span` does not implement `Ord` or `<`/`>`. Comparison is a type error.
This is the TC39 Temporal approach. Cleanest semantically but may
surprise users.

**Option B: Lexicographic ordering.**
Compare fields top-down: years, then months, then days, then hours, etc.
This gives a total order but it is not calendar-aware
(P1M < P31D even though 1 month can be 31 days).

**Option C: Approximate normalization.**
Normalize to an approximate nanosecond count using fixed conversion
factors (1 month = 30.4375 days, 1 year = 365.25 days) and compare.
Gives a "usually right" total order but can produce surprising results
at boundary cases.

### Span sign representation

How to represent the sign of a span.

**Option A: Single overall sign.**
One `sign: i8` field applies to the whole span. All magnitude fields
are non-negative. This is the proposed layout above. Matches jiff's
`Span` which has a single sign. Simplest invariant.

**Option B: Per-field signs.**
Each field (years, months, days, etc.) independently signed.
Allows mixed-sign spans like "1 year minus 2 months."
More expressive but harder to normalize and reason about.
TC39 Temporal requires all fields to have the same sign
(a "balanced" span), effectively Option A.

**Option C: Signed fields, normalized.**
Per-field signs allowed in construction, but always normalized to
a canonical form where all fields share the same sign.
Middle ground: accepts mixed input, stores uniform sign.
