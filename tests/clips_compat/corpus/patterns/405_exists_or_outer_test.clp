(deffacts d (a 1) (a 2) (a -1) (b 1) (c 1) (b -1) (c 3))
(defrule r (a ?x) (exists (or (b ?x) (c ?x))) (test (> ?x 0)) => (printout t "fired " ?x crlf))
(defrule s (a ?x) (not (b ?x)) (test (> ?x 0)) => (printout t "unmatched " ?x crlf))
