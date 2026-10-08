(deffacts d (item 1))
(defrule r2 (item ?x) (test (> ?x 0)) => (printout t r2 crlf))
(defrule r3 (item ?x) (test (> ?x 0)) (test (< ?x 5)) => (printout t r3 crlf))
