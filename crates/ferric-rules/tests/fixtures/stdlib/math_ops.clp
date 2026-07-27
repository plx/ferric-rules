; Basic arithmetic operations.
; Note: division always returns float in Ferric (e.g. (/ 100 4) => 25.0).
(deffacts startup (compute))

(defrule math-test
    (compute)
    =>
    (printout t "add: " (+ 10 20) crlf)
    (printout t "sub: " (- 50 8) crlf)
    (printout t "mul: " (* 6 7) crlf)
    (printout t "div: " (/ 100 4) crlf)
    (printout t "mod: " (mod 17 5) crlf)
    (printout t "abs: " (abs -42) crlf)
    (printout t "min: " (min 3 7 1 9) crlf)
    (printout t "max: " (max 3 7 1 9) crlf))
