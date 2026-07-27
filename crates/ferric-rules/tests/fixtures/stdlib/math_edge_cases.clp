; Math edge cases: min/max with multiple args, mixed numeric operations.
(deffacts startup (run-math))

(defrule math-edges
    (run-math)
    =>
    (printout t "min3: " (min 3 1 2) crlf)
    (printout t "max3: " (max 3 1 2) crlf)
    (printout t "neg-abs: " (abs -7) crlf)
    (printout t "div-trunc: " (div 10 3) crlf)
    (printout t "mod-neg: " (mod 10 3) crlf))
