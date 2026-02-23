; User-defined function called from rule RHS.
; deffunction body is a pure expression; printout happens in the rule RHS.
(deffunction double (?x) (* ?x 2))
(deffacts startup (value 21))

(defrule compute
    (value ?v)
    =>
    (printout t "double: " (double ?v) crlf))
