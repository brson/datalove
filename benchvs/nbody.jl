#!/usr/bin/env julia
# N-body benchmark - the Jovian planets orbiting the sun, 250,000 steps of
# 0.01, several times over. From Are We Fast Yet
# (github.com/smarr/are-we-fast-yet), whose NBody at 250,000 steps ends with
# an energy of -0.1690859889909308, and before it the Computer Language
# Benchmarks Game. Prints the sum of the final energies.

const ITERATIONS = 6
const STEPS = 250000

const PI = 3.141592653589793
const SOLAR_MASS = 4 * PI * PI
const DAYS_PER_YEAR = 365.24

mutable struct Body
    x::Float64
    y::Float64
    z::Float64
    vx::Float64
    vy::Float64
    vz::Float64
    mass::Float64
end

body(x, y, z, vx, vy, vz, mass) =
    Body(x, y, z, vx * DAYS_PER_YEAR, vy * DAYS_PER_YEAR, vz * DAYS_PER_YEAR, mass * SOLAR_MASS)

function create_bodies()
    bodies = [
        body(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0),
        body(4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01,
             1.66007664274403694e-03, 7.69901118419740425e-03, -6.90460016972063023e-05,
             9.54791938424326609e-04),
        body(8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01,
             -2.76742510726862411e-03, 4.99852801234917238e-03, 2.30417297573763929e-05,
             2.85885980666130812e-04),
        body(1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01,
             2.96460137564761618e-03, 2.37847173959480950e-03, -2.96589568540237556e-05,
             4.36624404335156298e-05),
        body(1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01,
             2.68067772490389322e-03, 1.62824170038242295e-03, -9.51592254519715870e-05,
             5.15138902046611451e-05),
    ]
    px = 0.0
    py = 0.0
    pz = 0.0
    for b in bodies
        px += b.vx * b.mass
        py += b.vy * b.mass
        pz += b.vz * b.mass
    end
    bodies[1].vx = 0.0 - (px / SOLAR_MASS)
    bodies[1].vy = 0.0 - (py / SOLAR_MASS)
    bodies[1].vz = 0.0 - (pz / SOLAR_MASS)
    return bodies
end

function advance(bodies::Vector{Body}, dt::Float64)
    n = length(bodies)
    for i in 1:n
        ib = bodies[i]
        for j in i+1:n
            jb = bodies[j]
            dx = ib.x - jb.x
            dy = ib.y - jb.y
            dz = ib.z - jb.z
            d_squared = dx * dx + dy * dy + dz * dz
            distance = sqrt(d_squared)
            mag = dt / (d_squared * distance)
            ib.vx = ib.vx - (dx * jb.mass * mag)
            ib.vy = ib.vy - (dy * jb.mass * mag)
            ib.vz = ib.vz - (dz * jb.mass * mag)
            jb.vx = jb.vx + (dx * ib.mass * mag)
            jb.vy = jb.vy + (dy * ib.mass * mag)
            jb.vz = jb.vz + (dz * ib.mass * mag)
        end
    end
    for b in bodies
        b.x = b.x + dt * b.vx
        b.y = b.y + dt * b.vy
        b.z = b.z + dt * b.vz
    end
end

function energy(bodies::Vector{Body})::Float64
    e = 0.0
    n = length(bodies)
    for i in 1:n
        ib = bodies[i]
        e += 0.5 * ib.mass * (ib.vx * ib.vx + ib.vy * ib.vy + ib.vz * ib.vz)
        for j in i+1:n
            jb = bodies[j]
            dx = ib.x - jb.x
            dy = ib.y - jb.y
            dz = ib.z - jb.z
            distance = sqrt(dx * dx + dy * dy + dz * dz)
            e -= (ib.mass * jb.mass) / distance
        end
    end
    return e
end

function main()
    total = 0.0
    for _ in 1:ITERATIONS
        bodies = create_bodies()
        for _ in 1:STEPS
            advance(bodies, 0.01)
        end
        total += energy(bodies)
    end
    println(total)
end

main()
