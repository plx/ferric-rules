(deffacts f (b 1) (a 1))
(defrule r2 (a ?x) (test (> ?x -1)) => (printout t r2 crlf))
(defrule r1 (a ?x) (test (> ?x 0)) (b ?y) (test (> ?y 0)) => (printout t r1 crlf))
