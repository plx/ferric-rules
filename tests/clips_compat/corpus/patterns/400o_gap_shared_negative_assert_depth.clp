(deffacts seed (item 1))
(defrule r1 (item ?x) (not (blocker)) => (printout t r1 crlf))
(defrule r2 (item ?x) => (printout t r2 crlf))
(defrule r3 (item ?x) (not (blocker)) => (printout t r3 crlf))

