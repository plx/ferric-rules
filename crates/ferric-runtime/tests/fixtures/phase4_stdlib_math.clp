;; Phase 4: Comprehensive math/predicate/type builtin surface fixture.
;; Exercises Section 10.2 predicate, math, and type conversion functions.

(defrule test-math
    (compute)
    =>
    (printout t "abs(-5): " (abs -5) crlf)
    (printout t "min(3,1,2): " (min 3 1 2) crlf)
    (printout t "max(3,1,2): " (max 3 1 2) crlf)
    (printout t "mod(10,3): " (mod 10 3) crlf)
    (printout t "div(10,3): " (div 10 3) crlf)
    (printout t "5+3: " (+ 5 3) crlf)
    (printout t "10/4: " (/ 10 4) crlf)
    (printout t "integerp(42): " (integerp 42) crlf)
    (printout t "floatp(3.14): " (floatp 3.14) crlf)
    (printout t "numberp(42): " (numberp 42) crlf)
    (printout t "symbolp(abc): " (symbolp abc) crlf)
    (printout t "evenp(4): " (evenp 4) crlf)
    (printout t "oddp(3): " (oddp 3) crlf)
    (printout t "integer(3.7): " (integer 3.7) crlf)
    (printout t "float(5): " (float 5) crlf))

(deffacts startup (compute))
