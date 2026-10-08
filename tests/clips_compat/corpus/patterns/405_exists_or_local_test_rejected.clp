(defrule r (exists (or (a ?x) (b ?x))) (test (> ?x 0)) => (printout t "fired" crlf))
