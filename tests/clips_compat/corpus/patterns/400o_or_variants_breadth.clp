(deffacts seed (a 1) (b 1))
(defrule seq-or (or (a ?x) (b ?x)) => (printout t seq-or crlf))
(defrule seq-or2 (a ?x) => (printout t seq-or2 crlf))
