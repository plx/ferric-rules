(deffacts seed (p 1) (p 2) (item))
(defrule r1 (p ?x) (item) => (printout t "r1 " ?x crlf))
(defrule r2 (p ?x) (item) => (printout t "r2 " ?x crlf))
