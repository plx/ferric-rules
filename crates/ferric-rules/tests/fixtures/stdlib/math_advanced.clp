; Advanced math operations: integer division, type conversion, abs.
(deffacts startup (compute))

(defrule math
    (compute)
    =>
    (printout t "int-div: " (div 17 5) crlf)
    (printout t "float: " (float 42) crlf)
    (printout t "integer: " (integer 3) crlf)
    (printout t "abs-neg: " (abs -99) crlf))
