// N-body benchmark - the Jovian planets orbiting the sun, 250,000 steps of
// 0.01, several times over. From Are We Fast Yet
// (github.com/smarr/are-we-fast-yet), whose NBody at 250,000 steps ends with
// an energy of -0.1690859889909308, and before it the Computer Language
// Benchmarks Game. Prints the sum of the final energies.

public class Nbody {
    static final int ITERATIONS = 6;
    static final int STEPS = 250000;

    static final double PI = 3.141592653589793;
    static final double SOLAR_MASS = 4 * PI * PI;
    static final double DAYS_PER_YEAR = 365.24;

    static final class Body {
        double x, y, z, vx, vy, vz;
        final double mass;

        Body(double x, double y, double z, double vx, double vy, double vz, double mass) {
            this.x = x;
            this.y = y;
            this.z = z;
            this.vx = vx * DAYS_PER_YEAR;
            this.vy = vy * DAYS_PER_YEAR;
            this.vz = vz * DAYS_PER_YEAR;
            this.mass = mass * SOLAR_MASS;
        }
    }

    static Body[] createBodies() {
        Body[] bodies = {
            new Body(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0),
            new Body(4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01,
                     1.66007664274403694e-03, 7.69901118419740425e-03, -6.90460016972063023e-05,
                     9.54791938424326609e-04),
            new Body(8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01,
                     -2.76742510726862411e-03, 4.99852801234917238e-03, 2.30417297573763929e-05,
                     2.85885980666130812e-04),
            new Body(1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01,
                     2.96460137564761618e-03, 2.37847173959480950e-03, -2.96589568540237556e-05,
                     4.36624404335156298e-05),
            new Body(1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01,
                     2.68067772490389322e-03, 1.62824170038242295e-03, -9.51592254519715870e-05,
                     5.15138902046611451e-05),
        };
        double px = 0.0, py = 0.0, pz = 0.0;
        for (Body b : bodies) {
            px += b.vx * b.mass;
            py += b.vy * b.mass;
            pz += b.vz * b.mass;
        }
        bodies[0].vx = 0.0 - (px / SOLAR_MASS);
        bodies[0].vy = 0.0 - (py / SOLAR_MASS);
        bodies[0].vz = 0.0 - (pz / SOLAR_MASS);
        return bodies;
    }

    static void advance(Body[] bodies, double dt) {
        for (int i = 0; i < bodies.length; ++i) {
            Body iBody = bodies[i];
            for (int j = i + 1; j < bodies.length; ++j) {
                Body jBody = bodies[j];
                double dx = iBody.x - jBody.x;
                double dy = iBody.y - jBody.y;
                double dz = iBody.z - jBody.z;
                double dSquared = dx * dx + dy * dy + dz * dz;
                double distance = Math.sqrt(dSquared);
                double mag = dt / (dSquared * distance);
                iBody.vx = iBody.vx - (dx * jBody.mass * mag);
                iBody.vy = iBody.vy - (dy * jBody.mass * mag);
                iBody.vz = iBody.vz - (dz * jBody.mass * mag);
                jBody.vx = jBody.vx + (dx * iBody.mass * mag);
                jBody.vy = jBody.vy + (dy * iBody.mass * mag);
                jBody.vz = jBody.vz + (dz * iBody.mass * mag);
            }
        }
        for (Body body : bodies) {
            body.x = body.x + dt * body.vx;
            body.y = body.y + dt * body.vy;
            body.z = body.z + dt * body.vz;
        }
    }

    static double energy(Body[] bodies) {
        double e = 0.0;
        for (int i = 0; i < bodies.length; ++i) {
            Body iBody = bodies[i];
            e += 0.5 * iBody.mass * (iBody.vx * iBody.vx + iBody.vy * iBody.vy + iBody.vz * iBody.vz);
            for (int j = i + 1; j < bodies.length; ++j) {
                Body jBody = bodies[j];
                double dx = iBody.x - jBody.x;
                double dy = iBody.y - jBody.y;
                double dz = iBody.z - jBody.z;
                double distance = Math.sqrt(dx * dx + dy * dy + dz * dz);
                e -= (iBody.mass * jBody.mass) / distance;
            }
        }
        return e;
    }

    public static void main(String[] args) {
        double total = 0.0;
        for (int n = 0; n < ITERATIONS; n++) {
            Body[] bodies = createBodies();
            for (int step = 0; step < STEPS; step++) {
                advance(bodies, 0.01);
            }
            total += energy(bodies);
        }
        System.out.println(total);
    }
}
