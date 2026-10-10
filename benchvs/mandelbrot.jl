#!/usr/bin/env julia
# Mandelbrot benchmark - the Mandelbrot set at 750 by 750, folded into a byte
# checksum, several times over. From Are We Fast Yet
# (github.com/smarr/are-we-fast-yet), whose Mandelbrot at 750 gives 50, and
# before it the Computer Language Benchmarks Game. Prints the sum of the
# checksums.

const ITERATIONS = 4
const SIZE = 750

function mandelbrot(size::Int)::Int
    sum = 0
    byte_acc = 0
    bit_num = 0
    y = 0
    while y < size
        ci = (2.0 * y / size) - 1.0
        x = 0
        while x < size
            zrzr = 0.0
            zi = 0.0
            zizi = 0.0
            cr = (2.0 * x / size) - 1.5
            z = 0
            not_done = true
            escape = 0
            while not_done && z < 50
                zr = zrzr - zizi + cr
                zi = 2.0 * zr * zi + ci
                zrzr = zr * zr
                zizi = zi * zi
                if zrzr + zizi > 4.0
                    not_done = false
                    escape = 1
                end
                z += 1
            end
            byte_acc = (byte_acc << 1) + escape
            bit_num += 1
            if bit_num == 8
                sum = xor(sum, byte_acc)
                byte_acc = 0
                bit_num = 0
            elseif x == size - 1
                byte_acc <<= 8 - bit_num
                sum = xor(sum, byte_acc)
                byte_acc = 0
                bit_num = 0
            end
            x += 1
        end
        y += 1
    end
    return sum
end

function main()
    total = 0
    for _ in 1:ITERATIONS
        total += mandelbrot(SIZE)
    end
    println(total)
end

main()
