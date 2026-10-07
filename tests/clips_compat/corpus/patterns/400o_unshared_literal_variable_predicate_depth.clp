(deffacts seed (item 1))
(defrule r1 (item 1) => (printout t r1 crlf))
(defrule r2 (item ?x) => (printout t r2 crlf))
(defrule r3 (item ?x&:(> ?x 0)) => (printout t r3 crlf))
