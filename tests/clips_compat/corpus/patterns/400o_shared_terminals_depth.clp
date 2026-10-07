(deffacts seed (item 1) (item 2))
(defrule r1 (item ?x) => (printout t "r1 " ?x crlf))
(defrule r2 (item ?x) => (printout t "r2 " ?x crlf))
(defrule r3 (item ?x) => (printout t "r3 " ?x crlf))
