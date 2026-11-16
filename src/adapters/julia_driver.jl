# The Julia driver of math-run: a loop that reads one request per line from
# standard input, runs it with Symbolics.jl, and prints the protocol lines of
# the runner (see src/adapters/mod.rs). The runner writes the code of each
# problem; this only runs it in a fresh module, times it, and prints the
# result as a tree, which the runner reads into the neutral syntax.
#
# A request is one line, a call with string arguments:
#   mr_start("nonce")                      warm up, print the version and the ready line
#   mr_problem("nonce", setup, task, answer)
#                                          run the setup (the symbols) in a new module,
#                                          time the task (which sets `result`), and print
#                                          the value of `answer` as a tree
#   mr_quit()
#
# The tree is JSON, each node an array: ["i", "12"] an integer, ["q", "1",
# "2"] a rational, ["f", "0.5"] a float, ["c", re, im] a complex number,
# ["k", "π"] a constant, ["b", "true"], ["s", "x"] a symbol, ["call",
# "Base.sin", args...] a call of a function, ["fn", "y", args...] of a
# symbolic function, ["d", var, order, expr] a derivative, ["int", var, lo,
# hi, expr] an integral, ["eq", lhs, rhs], ["rel", "<", lhs, rhs], ["l",
# items...] a list or a tuple, ["m", rows...] a matrix, ["nothing"].

import Symbolics
import SpecialFunctions
import LinearAlgebra
import Nemo
import Groebner
import SymbolicIntegration
const SymbolicUtils = Symbolics.SymbolicUtils
# the warm-up compiled into the environment's package images, where it is
# installed (julia/MathRunWarmup): its native code makes the start fast
try
    @eval import MathRunWarmup
catch
end

const MR_OUT = stdout

mr_say(line) = (print(MR_OUT, line, "\n"); nothing)

# the CPU time of the process, in seconds: C's clock(), whose
# CLOCKS_PER_SEC is 10^6 on Linux and macOS
mr_cpu() = ccall(:clock, Int64, ()) / 1.0e6

function mr_one_line(e)
    s = try
        sprint(showerror, e)
    catch
        string(typeof(e))
    end
    s = join(split(s), " ")
    length(s) > 1500 ? first(s, 1500) : s
end

# the error itself, not include_string's LoadError around it
function mr_error_text(e)
    while e isa LoadError
        e = e.error
    end
    string(nameof(typeof(e)), ": ", mr_one_line(e))
end

# --- the tree of a result -------------------------------------------------

function mr_json_string(io, s::AbstractString)
    print(io, '"')
    for c in s
        if c == '"'
            print(io, "\\\"")
        elseif c == '\\'
            print(io, "\\\\")
        elseif c < ' '
            print(io, "\\u", string(UInt32(c), base = 16, pad = 4))
        else
            print(io, c)
        end
    end
    print(io, '"')
end

function mr_node(io, tag, parts...)
    print(io, '[')
    mr_json_string(io, tag)
    for p in parts
        print(io, ',')
        if p isa AbstractString
            mr_json_string(io, p)
        else
            mr_tree(io, p)
        end
    end
    print(io, ']')
end

struct MRUnreadable <: Exception
    what::String
end
Base.showerror(io::IO, e::MRUnreadable) = print(io, e.what)

mr_fname(f) = try
    string(parentmodule(f), ".", nameof(f))
catch
    string(f)
end

mr_tree(io, x::Symbolics.Num) = mr_tree(io, Symbolics.unwrap(x))
mr_tree(io, x::Symbolics.Equation) = mr_node(io, "eq", x.lhs, x.rhs)
mr_tree(io, x::Bool) = mr_node(io, "b", string(x))
mr_tree(io, x::Integer) = mr_node(io, "i", string(x))
mr_tree(io, x::Rational) = mr_node(io, "q", string(numerator(x)), string(denominator(x)))
mr_tree(io, x::AbstractFloat) = mr_node(io, "f", string(x))
mr_tree(io, x::Complex) = mr_node(io, "c", real(x), imag(x))
mr_tree(io, x::AbstractIrrational) = mr_node(io, "k", string(x))
mr_tree(io, ::Nothing) = mr_node(io, "nothing")
mr_tree(io, x::AbstractMatrix) = mr_node(io, "m", [collect(x[i, :]) for i in axes(x, 1)]...)
mr_tree(io, x::Union{AbstractVector, Tuple}) = mr_node(io, "l", x...)
mr_tree(io, x) = mr_symbolic(io, x)

# Nemo's numbers, polynomials (of its rings, over the rationals or over a
# ring of polynomials), factorizations and matrices, written term by term
const AA = Nemo.AbstractAlgebra

struct MRCall
    f::String
    args::Vector{Any}
end
mr_tree(io, c::MRCall) = mr_node(io, "call", c.f, c.args...)
struct MRSym
    name::String
end
mr_tree(io, s::MRSym) = mr_node(io, "s", s.name)

mr_tree(io, x::Nemo.ZZRingElem) = mr_node(io, "i", string(x))
mr_tree(io, x::Nemo.QQFieldElem) = mr_tree(io, Rational{BigInt}(x))
mr_tree(io, m::AA.MatElem) = mr_node(io, "m", [[m[i, j] for j in 1:AA.ncols(m)] for i in 1:AA.nrows(m)]...)

# a polynomial of its terms, each a coefficient and powers of variables
function mr_terms(terms)
    parts = Any[]
    for (c, powers) in terms
        fs = Any[MRCall("Base.^", Any[MRSym(string(v)), e]) for (v, e) in powers if e > 0]
        fs = Any[f.args[2] == 1 ? f.args[1] : f for f in fs]
        if isempty(fs)
            push!(parts, c)
        elseif isone(c)
            push!(parts, length(fs) == 1 ? fs[1] : MRCall("Base.*", fs))
        else
            push!(parts, MRCall("Base.*", Any[c; fs]))
        end
    end
    isempty(parts) ? 0 : length(parts) == 1 ? parts[1] : MRCall("Base.+", parts)
end

mr_tree(io, p::AA.PolyRingElem) = mr_tree(io, mr_terms(
    [(AA.coeff(p, i), [(AA.var(AA.parent(p)), i)]) for i in AA.degree(p):-1:0 if !iszero(AA.coeff(p, i))]))
mr_tree(io, p::AA.MPolyRingElem) = mr_tree(io, mr_terms(
    [(c, collect(zip(AA.symbols(AA.parent(p)), e)))
     for (c, e) in zip(AA.coefficients(p), AA.exponent_vectors(p))]))
function mr_tree(io, f::AA.Fac)
    fs = Any[e == 1 ? p : MRCall("Base.^", Any[p, e]) for (p, e) in f]
    u = AA.unit(f)
    isone(u) || pushfirst!(fs, u)
    mr_tree(io, isempty(fs) ? 1 : length(fs) == 1 ? fs[1] : MRCall("Base.*", fs))
end

function mr_symbolic(io, x)
    if x isa SymbolicUtils.BasicSymbolic
        if SymbolicUtils.isconst(x)
            return mr_tree(io, SymbolicUtils.unwrap_const(x))
        elseif SymbolicUtils.iscall(x)
            op = SymbolicUtils.operation(x)
            args = SymbolicUtils.arguments(x)
            if op isa Symbolics.Differential
                return mr_node(io, "d", op.x, op.order, args[1])
            elseif op isa Symbolics.Integral
                d = op.domain
                return mr_node(io, "int", d.variables, Symbolics.infimum(d.domain),
                               Symbolics.supremum(d.domain), args[1])
            elseif op isa SymbolicUtils.BasicSymbolic
                return mr_node(io, "fn", string(nameof(op)), args...)
            elseif op isa Function
                return mr_node(io, "call", mr_fname(op), args...)
            end
            throw(MRUnreadable("the operation $(op) of type $(typeof(op))"))
        elseif SymbolicUtils.issym(x)
            return mr_node(io, "s", mr_name(string(nameof(x))))
        end
    end
    throw(MRUnreadable("a value of type $(typeof(x))"))
end

# symbolic_solve's integer of a periodic family of roots (`x ~ finv(y) +
# n*period`) is an anonymous variable, as `##396`: named k, k2, ... in the
# answer, each bound in its solution by element(k, integers)
const MR_NAMES = Dict{String, String}()
mr_name(s) = startswith(s, "##") ?
    get!(() -> length(MR_NAMES) == 0 ? "k" : "k" * string(length(MR_NAMES) + 1), MR_NAMES, s) : s
struct MRElement
    var::Any
end
mr_tree(io, e::MRElement) = mr_node(io, "fn", "element", e.var, MRSym("integers"))
mr_anonymous(r) = try
    [v for v in Symbolics.get_variables(r) if startswith(string(nameof(v)), "##")]
catch
    []
end

function mr_tree_string(x)
    empty!(MR_NAMES)
    io = IOBuffer()
    mr_tree(io, x)
    String(take!(io))
end

# --- helpers of the problems' code ----------------------------------------

# A rational number, computed in big integers: as an Int where it is one.
mr_q(x::Integer) = typemin(Int) <= x <= typemax(Int) ? Int(x) : x
mr_q(x::Rational) = denominator(x) == 1 ? mr_q(numerator(x)) :
    (typemin(Int) < numerator(x) <= typemax(Int) && denominator(x) <= typemax(Int) ?
     Rational{Int}(x) : x)

# A power of numbers by another than a whole number, as `2^(1//3)`: a term
# of Symbolics, as Julia's ^ computes it in floats.
mr_pow(a, b) = Symbolics.Num(SymbolicUtils.term(^, Symbolics.unwrap(a), Symbolics.unwrap(b)))

# A matrix of Symbolics' numbers, from its rows; and one of Nemo's rationals.
mr_matrix(rows) = [Symbolics.Num(rows[i][j]) for i in eachindex(rows), j in eachindex(rows[1])]
mr_qmatrix(rows) = Nemo.matrix(Nemo.QQ, [Rational{BigInt}(rows[i][j]) for i in eachindex(rows), j in eachindex(rows[1])])

# The basis of a null space, Nemo's columns, as a list of vectors.
mr_columns(n) = [[n[i, j] for i in 1:AA.nrows(n)] for j in 1:AA.ncols(n)]

# The solutions of symbolic_solve, as lists of equations var ~ value: a
# root of the one variable, or a Dict of the variables' values. `nothing`,
# its decline, as it is.
mr_solutions(result::Nothing, vars) = nothing
function mr_solutions(result, vars)
    map(collect(result)) do r
        sol = if r isa AbstractDict
            Any[v ~ r[v] for v in vars if haskey(r, v)]
        else
            Any[vars[1] ~ r]
        end
        values = r isa AbstractDict ? collect(Base.values(r)) : [r]
        ks = unique(vcat([mr_anonymous(v) for v in values]...))
        vcat(sol, Any[MRElement(k) for k in ks])
    end
end

# The solution of an ODE, y(x) = ...: a list of them where Symbolics gives
# several.
mr_ode_solutions(y, result::Nothing) = nothing
mr_ode_solutions(y, result::AbstractVector) = [y ~ r for r in result]
mr_ode_solutions(y, result) = y ~ result

# --- the loop -------------------------------------------------------------

function mr_module()
    m = Module(:MRProblem, true)
    Core.eval(m, :(using Symbolics, SpecialFunctions, LinearAlgebra))
    Core.eval(m, :(import SymbolicIntegration, Nemo, Groebner))
    m
end

function mr_problem(nonce, setup, task, answer; quiet = false)
    say(line) = quiet || mr_say(line)
    m = mr_module()
    try
        include_string(m, setup, "setup")
    catch e
        say("@@MR-BEGIN " * nonce)
        say("@@MR-ERROR the setup failed: " * mr_error_text(e))
        say("@@MR-END " * nonce)
        quiet || flush(MR_OUT)
        return
    end
    say("@@MR-BEGIN " * nonce)
    quiet || flush(MR_OUT)
    ok, err = true, ""
    cpu0, wall0 = mr_cpu(), time_ns()
    try
        include_string(m, task, "task")
    catch e
        ok, err = false, mr_error_text(e)
    end
    cpu, wall = mr_cpu() - cpu0, (time_ns() - wall0) / 1.0e9
    say("@@MR-TIME " * string(round(cpu, digits = 6)) * " " * string(round(wall, digits = 6)))
    if ok
        text = try
            mr_tree_string(include_string(m, answer, "answer"))
        catch e
            say("@@MR-ERROR the runner cannot read the answer: " * mr_error_text(e))
            nothing
        end
        if text !== nothing
            say("@@MR-ANSWER " * nonce)
            say(text)
            say("@@MR-ANSWER-END " * nonce)
        end
    else
        say("@@MR-ERROR " * err)
    end
    say("@@MR-END " * nonce)
    quiet || flush(MR_OUT)
    nothing
end

# a problem to warm up with, its output left out (its time on the standard
# error, which the runner shows where the start fails)
function mr_warm(setup, task, answer)
    t = @elapsed mr_problem("", setup, task, answer; quiet = true)
    println(stderr, "warm-up ", round(t, digits = 1), " s: ", first(task, 80))
end

function mr_start(nonce)
    mr_say("@@MR-VERSION " * string(VERSION) * ", Symbolics " *
           string(pkgversion(Symbolics)) * ", SymbolicIntegration " *
           string(pkgversion(SymbolicIntegration)))
    mr_say("@@MR-READY " * nonce)
    flush(MR_OUT)
end

const MR_REQUESTS = (:mr_start, :mr_warm, :mr_problem, :mr_quit)

function mr_main()
    for line in eachline(stdin)
        isempty(strip(line)) && continue
        ex = Meta.parse(line)
        if !(ex isa Expr && ex.head == :call && ex.args[1] in MR_REQUESTS &&
             all(a -> a isa String, ex.args[2:end]))
            mr_say("@@MR-ERROR not a request of the runner: " * first(line, 200))
            flush(MR_OUT)
            continue
        end
        ex.args[1] == :mr_quit && return
        f = Dict(:mr_start => mr_start, :mr_warm => mr_warm, :mr_problem => mr_problem)[ex.args[1]]
        Base.invokelatest(f, ex.args[2:end]...)
    end
end

mr_main()
