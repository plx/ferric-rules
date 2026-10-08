(deffacts d (seed) (a 1) (a -1) (b 1) (b 2) (c 1))
(defrule inner (seed) (not (and (c ?) (not (a ?x)) (b ?y) (test (> ?y 0)))) => (printout t "inner" crlf))
(defrule both (or (a ?x) (b ?x)) (test (> ?x 0)) => (printout t "both " ?x crlf))
