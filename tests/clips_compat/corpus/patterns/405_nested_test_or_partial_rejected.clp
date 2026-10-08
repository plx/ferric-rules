(deffacts d (seed) (a 1) (a -1) (b 1) (b 2) (c 1))
(defrule r (seed) (or (a ?x) (b ?y)) (test (> ?x 0)) => (printout t "fired" crlf))
