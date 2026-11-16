# math-run's warm-up, compiled once when the environment is precompiled:
# a small problem of each op, as math-run's Julia adapter gives them, so that
# Julia's package images keep the native code of the commands the problems
# use, and a session starts in seconds, not minutes. (math-run starts a new
# session after each problem that runs out of time.) Nothing of it runs when
# the package is loaded.
module MathRunWarmup

using PrecompileTools: @setup_workload, @compile_workload
import Symbolics, SpecialFunctions, LinearAlgebra, Nemo, Groebner, SymbolicIntegration

function ivp(x, D)
    g = only(Symbolics.@syms y(x)::Real)
    Symbolics.laplace_solve_ode((D^2)(g(x)) + g(x) ~ 0, g, x, [1, 0])
    Symbolics.laplace_solve_ode(D(g(x)) + g(x) ~ x, g, x, [1])
end

function transforms(t, s)
    Symbolics.@syms mr_f(t)::Real mr_F(s)::Real
    Symbolics.laplace(exp(-t) * sin(t), mr_f, t, mr_F, s)
    Symbolics.inverse_laplace(1 / (s^2 + 3 * s + 2), mr_F, s, mr_f, t)
end

function workload()
    x = Symbolics.variable(:x)
    t = Symbolics.variable(:t)
    s = Symbolics.variable(:s)
    a = Symbolics.variable(:a)
    D = Symbolics.Differential(x)
    U = Symbolics.unwrap
    try_(f) = try
        f()
    catch
        nothing
    end
    try_(() -> SymbolicIntegration.integrate(x * exp(x) + 1 / (1 + x^2) + sqrt(x + 1), x))
    try_(() -> SymbolicIntegration.integrate(2 * x * exp(x^2) + sin(x) / x, x))
    y = Symbolics.variable(:y)
    try_(() -> Symbolics.symbolic_solve_ode(Symbolics.Differential(x)(y) + y ~ x, y, x))
    try_(() -> Symbolics.symbolic_solve_ode((Symbolics.Differential(x)^2)(y) + y ~ 0, y, x))
    try_(() -> ivp(x, D))
    try_(() -> Symbolics.limit(U((exp(x) - 1) / x), U(x), 0, :both))
    try_(() -> Symbolics.limit(U(log(x) / x), U(x), Inf))
    try_(() -> Symbolics.taylor(exp(x) * sin(x), x, 0, 0:2))
    try_(() -> transforms(t, s))
    try_(() -> Symbolics.symbolic_solve(x^2 - 3 * x + 2 ~ 0, x))
    try_(() -> Symbolics.symbolic_solve(exp(2 * x) ~ 3, x))
    try_(() -> Symbolics.symbolic_solve([x + y ~ 3, x * y ~ 2], [x, y]))
    try_(() -> Symbolics.simplify(sin(x)^2 + cos(x)^2 + (x^2 - 1) / (x - 1)))
    try_(() -> Symbolics.simplify(Symbolics.expand_derivatives(D(x^3) + SpecialFunctions.gamma(Symbolics.Num(1 // 2)))))
    try_(() -> Symbolics.partial_frac_decomposition((x + 3) / (x^3 - x), x))
    try_(() -> Symbolics.groebner_basis([x^2 + y, x * y - 1], ordering = Groebner.Lex(x, y)))
    m = [Symbolics.Num(1) a; Symbolics.Num(2) Symbolics.Num(3)]
    try_(() -> LinearAlgebra.det(m))
    try_(() -> LinearAlgebra.inv(m))
    v = [Symbolics.variable(Symbol("mr_v", j)) for j in 1:2]
    try_(() -> Symbolics.symbolic_linear_solve(m * v .~ [3, 1], v))
    R, (X, A) = Nemo.polynomial_ring(Nemo.QQ, [:x, :a])
    try_(() -> Nemo.factor(X^4 - A^4))
    try_(() -> reduce(Nemo.gcd, [X^2 - 1, X^2 + 2X + 1]))
    S, Z = Nemo.polynomial_ring(Nemo.polynomial_ring(Nemo.QQ, [:a])[1], :x)
    try_(() -> Nemo.resultant(3Z - 9, Z^2 - 5Z))
    try_(() -> Nemo.discriminant(Z^2 + 3Z + 1))
    K = Nemo.algebraic_closure(Nemo.QQ)
    P, _ = Nemo.polynomial_ring(Nemo.QQ, :x)
    try_(() -> Nemo.minpoly(P, sqrt(K(2)) + Nemo.root(K(3), 3) + Nemo.cospi(K(2 // 7))))
    try_(() -> Nemo.rank(Nemo.matrix(Nemo.QQ, [1 2; 2 4])))
    try_(() -> Nemo.nullspace(Nemo.matrix(Nemo.QQ, [1 2; 2 4])))
    nothing
end

@setup_workload begin
    @compile_workload begin
        workload()
    end
end

end
