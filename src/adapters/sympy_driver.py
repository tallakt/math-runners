# The SymPy driver of math-run: a loop that reads one request per line of
# JSON from standard input, runs it in SymPy, and prints the protocol lines
# of the runner (see src/adapters/mod.rs). The runner makes the code of each
# problem; this only runs it in a fresh namespace, with SymPy's cache
# cleared, and times it.
#
# A request: {"nonce": ..., "start": true} prints the version and the ready
# line; {"nonce": ..., "setup": code, "task": code, "answer": code} runs the
# setup (the symbols), times the task (which sets `result`), and prints the
# answer, `str` of the expression `answer`.

import json
import sys
import time

import sympy as sp
from sympy.core.cache import clear_cache


def say(line):
    sys.stdout.write(line + "\n")


def warm_up():
    # SymPy imports much of itself at the first call of each solver; done
    # here, at the start, so that it is not timed as the first problem's.
    x, n, s = sp.symbols("x n s")
    f = sp.Function("f")
    sp.integrate(x * sp.exp(x), x)
    sp.integrate(sp.exp(-x), (x, 0, sp.oo))
    sp.summation(1 / n**2, (n, 1, sp.oo))
    sp.limit(sp.sin(x) / x, x, 0)
    sp.series(sp.exp(x), x, 0, 3)
    sp.dsolve(f(x).diff(x) - f(x), f(x))
    sp.rsolve(f(n + 1) - 2 * f(n), f(n))
    sp.solve(x**2 - 1, x)
    sp.solveset(x**2 - 1, x)
    sp.simplify(sp.sin(x) ** 2 + sp.cos(x) ** 2)
    sp.laplace_transform(sp.exp(-x), x, s, noconds=True)
    sp.factor(x**2 - 1)
    sp.Matrix([[1, 2], [3, 4]]).eigenvals()
    clear_cache()


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        req = json.loads(line)
        nonce = req["nonce"]
        if req.get("start"):
            warm_up()
            say("@@MR-VERSION " + sp.__version__)
            say("@@MR-READY " + nonce)
            sys.stdout.flush()
            continue
        if req.get("quit"):
            return
        clear_cache()
        ns = {"sp": sp}
        try:
            exec(compile(req["setup"], "<setup>", "exec"), ns)
        except Exception as ex:
            say("@@MR-BEGIN " + nonce)
            say("@@MR-ERROR the setup failed: %s: %s" % (type(ex).__name__, one_line(ex)))
            say("@@MR-END " + nonce)
            sys.stdout.flush()
            continue
        say("@@MR-BEGIN " + nonce)
        sys.stdout.flush()
        task = compile(req["task"], "<task>", "exec")
        cpu0, wall0 = time.process_time(), time.perf_counter()
        try:
            exec(task, ns)
            ok = True
        except Exception as ex:
            ok = False
            error = "%s: %s" % (type(ex).__name__, one_line(ex))
        cpu, wall = time.process_time() - cpu0, time.perf_counter() - wall0
        say("@@MR-TIME %.6f %.6f" % (cpu, wall))
        if ok:
            try:
                # a matrix prints on several lines, which read back as one
                text = str(eval(compile(req["answer"], "<answer>", "eval"), ns)).replace("\n", "")
                say("@@MR-ANSWER " + nonce)
                say(text)
                say("@@MR-ANSWER-END " + nonce)
            except Exception as ex:
                say("@@MR-ERROR the answer could not be printed: %s: %s" % (type(ex).__name__, one_line(ex)))
        else:
            say("@@MR-ERROR " + error)
        say("@@MR-END " + nonce)
        sys.stdout.flush()


def one_line(ex):
    return " ".join(str(ex).split())[:1500]


if __name__ == "__main__":
    # big integers print whole (Python 3.11 limits them to 4300 digits)
    if hasattr(sys, "set_int_max_str_digits"):
        sys.set_int_max_str_digits(0)
    main()
